mod common;

use std::collections::BTreeMap;

use common::{Browser, TestApp, json as body, start};
use compact_company::config::Config;
use serde_json::{Value, json};

fn today() -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::get("Europe/Berlin").unwrap())
        .date()
        .to_string()
}

async fn set_taxation(app: &TestApp, owner: &mut Browser, taxation: &str) {
    app.ok(
        owner,
        "PUT",
        "/api/business",
        json!({
            "name": "swilk UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
            "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
            "taxation": taxation, "iban": "DE89370400440532013000"
        }),
    )
    .await;
}

/// Balances by account number, EUR cents, debit positive; asserts the journal balances.
async fn balances(app: &TestApp, owner: &mut Browser) -> BTreeMap<String, i64> {
    let accounts = app.ok(owner, "GET", "/api/accounts", json!({})).await;
    let balances: BTreeMap<String, i64> = accounts
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["number"].as_str().unwrap().to_owned(),
                a["balance"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(balances.values().sum::<i64>(), 0, "the journal balances");
    balances
}

fn lines(posting: &Value) -> BTreeMap<String, i64> {
    posting["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["number"].as_str().unwrap().to_owned(),
                l["amount"].as_i64().unwrap(),
            )
        })
        .collect()
}

fn expect(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().map(|(n, a)| ((*n).to_owned(), *a)).collect()
}

async fn invoice(app: &TestApp, owner: &mut Browser, customer: i64) -> Value {
    let draft = app
        .ok(
            owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Entwicklung", "quantity": "10", "unit": "hour",
                  "unit_price": 10000, "vat_rate": "19" },
                { "description": "Fachbuch", "quantity": "1", "unit": "piece",
                  "unit_price": 10000, "vat_rate": "7" }
            ] }),
        )
        .await;
    app.ok(
        owner,
        "POST",
        &format!("/api/documents/{}/issue", draft["id"]),
        json!({}),
    )
    .await
}

async fn journal(app: &TestApp, owner: &mut Browser) -> Vec<Value> {
    app.ok(owner, "GET", "/api/postings", json!({}))
        .await
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn an_invoice_its_payments_and_a_cancellation_post_under_each_taxation() {
    for taxation in ["accrual", "cash", "small_business"] {
        let app = start(Config::default()).await;
        let mut owner = app.setup_owner().await;
        let customer = app.ready_to_issue(&mut owner).await;
        set_taxation(&app, &mut owner, taxation).await;
        app.ok(
            &mut owner,
            "POST",
            "/api/books",
            json!({ "chart": "skr03", "start_date": "2026-01-01" }),
        )
        .await;

        let invoice = invoice(&app, &mut owner, customer).await;
        let id = invoice["id"].as_i64().unwrap();
        let issued = journal(&app, &mut owner).await;
        assert_eq!(issued.len(), 1, "{taxation}");
        assert_eq!(issued[0]["document_id"], id);
        let expected = match taxation {
            "accrual" => expect(&[
                ("1400", 129_700),
                ("8400", -100_000),
                ("1776", -19_000),
                ("8300", -10_000),
                ("1771", -700),
            ]),
            "cash" => expect(&[
                ("1400", 129_700),
                ("8400", -100_000),
                ("1766", -19_000),
                ("8300", -10_000),
                ("1761", -700),
            ]),
            _ => expect(&[("1400", 110_000), ("8195", -110_000)]),
        };
        assert_eq!(lines(&issued[0]), expected, "{taxation}");
        let gross = expected["1400"];

        let half = gross / 2;
        app.ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/payments"),
            json!({ "date": today(), "amount": half }),
        )
        .await;
        let paid = journal(&app, &mut owner).await;
        let expected = match taxation {
            "cash" => expect(&[
                ("1200", half),
                ("1400", -half),
                ("1766", 9_500),
                ("1776", -9_500),
                ("1761", 350),
                ("1771", -350),
            ]),
            _ => expect(&[("1200", half), ("1400", -half)]),
        };
        assert_eq!(lines(&paid[1]), expected, "{taxation}");

        let cancellation = app
            .ok(
                &mut owner,
                "POST",
                &format!("/api/documents/{id}/follow-up"),
                json!({ "kind": "cancellation" }),
            )
            .await;
        let book = &cancellation["lines"][1];
        let path = format!("/api/documents/{}", cancellation["id"]);
        app.ok(
            &mut owner,
            "PUT",
            &path,
            json!({ "kind": "cancellation", "customer_id": customer, "lines": [
                { "description": "x", "quantity": "-1", "unit": "piece", "unit_price": 1,
                  "cancels_line_id": book["cancels_line_id"] }
            ] }),
        )
        .await;
        let cancelled = app
            .ok(&mut owner, "POST", &format!("{path}/issue"), json!({}))
            .await;
        let postings = journal(&app, &mut owner).await;
        assert_eq!(postings[2]["document_id"], cancelled["id"]);
        let book_gross = if taxation == "small_business" {
            10_000
        } else {
            10_700
        };
        assert_eq!(lines(&postings[2])["1400"], -book_gross, "{taxation}");

        let document = app
            .ok(
                &mut owner,
                "GET",
                &path.replace(&cancellation["id"].to_string(), &id.to_string()),
                json!({}),
            )
            .await;
        let rest = document["amount_due"]["minor"].as_i64().unwrap();
        assert_eq!(rest, gross - half - book_gross);
        app.ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/payments"),
            json!({ "date": today(), "amount": rest }),
        )
        .await;

        let totals = balances(&app, &mut owner).await;
        assert_eq!(totals["1400"], 0, "{taxation}: the receivable is cleared");
        assert_eq!(totals["1200"], gross - book_gross);
        match taxation {
            "accrual" => {
                assert_eq!(totals["1776"], -19_000);
                assert_eq!(totals["1771"], 0);
            }
            "cash" => {
                assert_eq!(totals["1766"], 0, "all VAT on the paid invoice is due");
                assert_eq!(totals["1776"], -19_000);
                assert_eq!(totals["1761"], 0);
                assert_eq!(totals["1771"], 0);
            }
            _ => assert_eq!(totals["8195"], -100_000),
        }

        let payment = app
            .ok(
                &mut owner,
                "GET",
                &format!("/api/documents/{id}"),
                json!({}),
            )
            .await["payments"][1]["id"]
            .clone();
        let response = app
            .as_browser(
                &mut owner,
                "POST",
                &format!("/api/payments/{payment}/reverse"),
                json!({}),
            )
            .await;
        assert_eq!(response.status(), 204);
        let after = balances(&app, &mut owner).await;
        assert_eq!(
            after["1400"], rest,
            "{taxation}: the reversed payment is due again"
        );
        let due = app
            .ok(
                &mut owner,
                "GET",
                &format!("/api/documents/{id}"),
                json!({}),
            )
            .await["amount_due"]["minor"]
            .clone();
        assert_eq!(due, rest);
    }
}

#[tokio::test]
async fn foreign_currency_books_at_the_rate_given_and_the_difference_is_an_exchange_result() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    app.ready_to_issue(&mut owner).await;
    set_taxation(&app, &mut owner, "small_business").await;
    let customer = app
        .ok(
            &mut owner,
            "POST",
            "/api/customers",
            json!({ "name": "Atelier Zürich", "language": "de", "currency": "CHF",
                    "street": "Seestrasse 1", "postcode": "8002", "city": "Zürich",
                    "country": "CH" }),
        )
        .await["id"]
        .as_i64()
        .unwrap();
    let draft = |amount: i64| {
        json!({ "kind": "invoice", "customer_id": customer, "lines": [
            { "description": "Beratung", "quantity": "1", "unit": "piece", "unit_price": amount }
        ] })
    };

    let before = app
        .ok(&mut owner, "POST", "/api/documents", draft(100_000))
        .await;
    let earlier = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", before["id"]),
            json!({}),
        )
        .await;
    assert_eq!(earlier["currency"], "CHF");

    let start = json!({ "chart": "skr04", "start_date": "2026-01-01" });
    let missing = app
        .as_browser(&mut owner, "POST", "/api/books", start.clone())
        .await;
    let missing = body(missing).await;
    assert_eq!(
        missing["fields"][format!("exchange_rates.{}", before["id"])],
        "required",
        "{missing}"
    );
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": "2026-01-01",
                "exchange_rates": { before["id"].to_string(): "1.05" } }),
    )
    .await;
    let booked = journal(&app, &mut owner).await;
    assert_eq!(
        lines(&booked[0]),
        expect(&[("1200", 105_000), ("4185", -105_000)])
    );

    let next = app
        .ok(&mut owner, "POST", "/api/documents", draft(20_000))
        .await;
    let issue = format!("/api/documents/{}/issue", next["id"]);
    let refused = app.as_browser(&mut owner, "POST", &issue, json!({})).await;
    assert_eq!(body(refused).await["fields"]["exchange_rate"], "required");
    app.ok(
        &mut owner,
        "POST",
        &format!("{issue}?exchange_rate=1.1"),
        json!({}),
    )
    .await;

    let pay = format!("/api/documents/{}/payments", before["id"]);
    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            &pay,
            json!({ "date": today(), "amount": 100_000 }),
        )
        .await;
    assert_eq!(body(refused).await["fields"]["eur_amount"], "required");
    app.ok(
        &mut owner,
        "POST",
        &pay,
        json!({ "date": today(), "amount": 100_000, "eur_amount": 104_200 }),
    )
    .await;
    let postings = journal(&app, &mut owner).await;
    assert_eq!(
        lines(postings.last().unwrap()),
        expect(&[("1800", 104_200), ("1200", -105_000), ("6880", 800)]),
        "a loss of 8 EUR against the rate booked"
    );
    let totals = balances(&app, &mut owner).await;
    assert_eq!(totals["1200"], 22_000, "only the second invoice is open");
}
