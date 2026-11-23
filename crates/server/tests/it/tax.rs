use crate::common;

use std::collections::BTreeMap;

use axum::http::StatusCode;
use common::{Browser, TestApp, json as body, start};
use compact_company::config::Config;
use serde_json::{Value, json};

const UBL: &[u8] = include_bytes!("../fixtures/xrechnung-ubl.xml");

fn day(offset: i64) -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::get("Europe/Berlin").unwrap())
        .date()
        .checked_add(jiff::Span::new().days(offset))
        .unwrap()
        .to_string()
}

async fn set_taxation(app: &TestApp, owner: &mut Browser, taxation: &str) {
    app.ok(
        owner,
        "PUT",
        "/api/business",
        json!({
            "name": "Beispiel UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
            "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
            "seller_identifier": "DE123456789-00001",
            "taxation": taxation, "iban": "DE89370400440532013000"
        }),
    )
    .await;
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

async fn account(app: &TestApp, owner: &mut Browser, number: &str) -> Value {
    app.ok(owner, "GET", "/api/accounts", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == number)
        .unwrap()
        .clone()
}

/// Amounts by key.
fn amounts(figures: &Value) -> BTreeMap<String, i64> {
    figures
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["key"].as_str().unwrap().to_owned(),
                f["amount"].as_i64().unwrap(),
            )
        })
        .collect()
}

fn expect(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().map(|(k, a)| ((*k).to_owned(), *a)).collect()
}

fn figure<'a>(figures: &'a Value, key: &str) -> &'a Value {
    figures
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == key)
        .unwrap()
}

#[tokio::test]
async fn a_part_paid_invoice_and_a_paid_receipt_show_in_the_vat_return_and_the_euer() {
    for taxation in ["accrual", "cash", "small_business"] {
        let app = start(Config::default()).await;
        let mut owner = app.setup_owner().await;
        let customer = app.ready_to_issue(&mut owner).await;
        set_taxation(&app, &mut owner, taxation).await;
        app.ok(
            &mut owner,
            "POST",
            "/api/books",
            json!({ "chart": "skr04", "start_date": "2026-01-01" }),
        )
        .await;
        let small = taxation == "small_business";

        let invoice = invoice(&app, &mut owner, customer).await;
        let gross: i64 = if small { 110_000 } else { 129_700 };
        let today = day(0);
        app.ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/payments", invoice["id"]),
            json!({ "date": today, "amount": gross / 2 }),
        )
        .await;
        let response = app
            .multipart(
                &mut owner,
                "POST",
                "/api/receipts",
                &[("file", Some("x.xml"), UBL)],
            )
            .await;
        let receipt = body(response).await;
        app.ok(
            &mut owner,
            "POST",
            &format!("/api/receipts/{}/book", receipt["id"]),
            json!({}),
        )
        .await;
        app.ok(
            &mut owner,
            "POST",
            &format!("/api/receipts/{}/payments", receipt["id"]),
            json!({ "date": today, "amount": 14_040 }),
        )
        .await;
        let bank = account(&app, &mut owner, "1800").await["id"].clone();
        let fees = account(&app, &mut owner, "6855").await["id"].clone();
        let prepayments = account(&app, &mut owner, "3820").await["id"].clone();
        for (other, amount, description) in [
            (&fees, 490, "Kontoführung"),
            (&prepayments, 5_000, "USt-Vorauszahlung"),
        ] {
            app.ok(
                &mut owner,
                "POST",
                "/api/postings",
                json!({ "date": today, "description": description, "lines": [
                    { "account_id": other, "amount": amount },
                    { "account_id": bank, "amount": -amount }
                ] }),
            )
            .await;
        }

        let vat = app
            .ok(
                &mut owner,
                "GET",
                "/api/tax/vat-return?from=2026-01-01&to=2099-12-31",
                json!({}),
            )
            .await;
        match taxation {
            "accrual" => {
                assert_eq!(vat["basis"], "accrual");
                assert_eq!(
                    amounts(&vat["figures"]),
                    expect(&[
                        ("81", 100_000),
                        ("81_tax", 19_000),
                        ("86", 10_000),
                        ("86_tax", 700),
                        ("66", 2_040),
                        ("83", 17_660)
                    ])
                );
                let base = figure(&vat["figures"], "81");
                assert_eq!(base["entered"], "1000");
                assert_eq!(
                    base["sources"][0]["description"].as_str().unwrap()[..8],
                    *"Rechnung"
                );
                assert_eq!(figure(&vat["figures"], "83")["entered"], "176.60");
            }
            "cash" => {
                assert_eq!(vat["basis"], "cash");
                assert_eq!(
                    amounts(&vat["figures"]),
                    expect(&[
                        ("81", 50_000),
                        ("81_tax", 9_500),
                        ("86", 5_000),
                        ("86_tax", 350),
                        ("66", 2_040),
                        ("83", 7_810)
                    ])
                );
                let base = figure(&vat["figures"], "81");
                assert_eq!(base["sources"].as_array().unwrap().len(), 1);
                assert_eq!(
                    base["sources"][0]["description"].as_str().unwrap()[..7],
                    *"Zahlung"
                );
            }
            _ => {
                assert_eq!(vat["basis"], Value::Null);
                assert_eq!(vat["figures"], json!([]));
            }
        }

        let year = &today[..4];
        let euer = app
            .ok(
                &mut owner,
                "GET",
                &format!("/api/tax/euer?year={year}"),
                json!({}),
            )
            .await;
        let lines = amounts(&euer["lines"]);
        if small {
            assert_eq!(
                lines,
                expect(&[("12", 55_000), ("58", 5_000), ("60", 14_040 + 490)])
            );
        } else {
            assert_eq!(
                lines,
                expect(&[
                    ("15", 55_000),
                    ("17", 9_850),
                    ("57", 2_040),
                    ("58", 5_000),
                    ("60", 12_000 + 490)
                ]),
                "{taxation}"
            );
        }
        let income = gross / 2;
        let expenses = 14_040 + 490 + 5_000;
        assert_eq!(euer["income"], income);
        assert_eq!(euer["expenses"], expenses);
        assert_eq!(euer["profit"], income - expenses);
        let paid = figure(&euer["lines"], "60");
        assert_eq!(paid["sources"].as_array().unwrap().len(), 2, "{paid}");
    }
}

#[tokio::test]
async fn an_invoice_from_before_the_books_counts_when_paid_and_accounts_carry_their_euer_line() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    set_taxation(&app, &mut owner, "accrual").await;
    let invoice = invoice(&app, &mut owner, customer).await;
    let tomorrow = day(1);
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr03", "start_date": tomorrow }),
    )
    .await;
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/documents/{}/payments", invoice["id"]),
        json!({ "date": tomorrow, "amount": 129_700 }),
    )
    .await;
    let euer = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/tax/euer?year={}", &tomorrow[..4]),
            json!({}),
        )
        .await;
    assert_eq!(
        amounts(&euer["lines"]),
        expect(&[("15", 110_000), ("17", 19_700)])
    );

    assert_eq!(account(&app, &mut owner, "8400").await["euer_line"], "15");
    assert_eq!(
        account(&app, &mut owner, "1800").await["euer_line"],
        Value::Null
    );
    let created = app
        .ok(
            &mut owner,
            "POST",
            "/api/accounts",
            json!({ "number": "4960", "name": "Mieten für Einrichtungen", "type": "expense" }),
        )
        .await;
    assert_eq!(created["euer_line"], "60");
    let changed = app
        .ok(
            &mut owner,
            "PUT",
            &format!("/api/accounts/{}", created["id"]),
            json!({ "name": "Mieten für Einrichtungen", "euer_line": "39" }),
        )
        .await;
    assert_eq!(changed["euer_line"], "39");
    let unknown = app
        .as_browser(
            &mut owner,
            "PUT",
            &format!("/api/accounts/{}", created["id"]),
            json!({ "name": "x", "euer_line": "99" }),
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);

    let wrong = app
        .as_browser(
            &mut owner,
            "GET",
            "/api/tax/vat-return?from=2026-02-01&to=2026-01-01",
            json!({}),
        )
        .await;
    assert_eq!(body(wrong).await["fields"]["to"], "invalid_format");
    let mut staff = app.with_role(&mut owner, "staff").await;
    let refused = app
        .as_browser(&mut staff, "GET", "/api/tax/euer?year=2026", json!({}))
        .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn expenses_land_on_their_own_zeile_and_corrections_change_the_sum() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let today = day(0);
    let year = &today[..4];
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": format!("{year}-01-01") }),
    )
    .await;
    let bank = account(&app, &mut owner, "1800").await["id"].clone();
    let mut own = async |number: &str, line: &str| {
        app.ok(
            &mut owner,
            "POST",
            "/api/accounts",
            json!({ "number": number, "name": format!("Zeile {line}"), "type": "expense",
                    "euer_line": line }),
        )
        .await["id"]
            .clone()
    };
    let property = own("6350", "41").await;
    let repairs = own("6335", "42").await;
    let commute = own("6690", "72").await;
    let allowance = own("6691", "73").await;
    let telephone = account(&app, &mut owner, "6805").await;
    assert_eq!(telephone["euer_line"], "43");
    let advertising = account(&app, &mut owner, "6600").await["id"].clone();
    for (account, amount) in [
        (telephone["id"].clone(), 10_000),
        (advertising, 5_000),
        (property, 20_000),
        (repairs, 8_000),
        (commute, 3_000),
        (allowance, 4_000),
    ] {
        app.ok(
            &mut owner,
            "POST",
            "/api/postings",
            json!({ "date": today, "description": "Ausgabe", "lines": [
                { "account_id": account, "amount": amount },
                { "account_id": bank, "amount": -amount }
            ] }),
        )
        .await;
    }
    let euer = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/tax/euer?year={year}"),
            json!({}),
        )
        .await;
    assert_eq!(
        amounts(&euer["lines"]),
        expect(&[
            ("41", 28_000),
            ("42", 8_000),
            ("43", 10_000),
            ("54", 5_000),
            ("72", 3_000),
            ("73", 4_000)
        ])
    );
    assert_eq!(
        euer["expenses"],
        28_000 + 10_000 + 5_000 - 3_000 + 4_000,
        "Zeile 42 is part of 41, 72 is taken off and 73 added"
    );
    assert_eq!(euer["profit"], -44_000);
}

#[tokio::test]
async fn bewirtung_is_split_70_30_and_only_deductible_gifts_count() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let today = day(0);
    let year = &today[..4];
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": format!("{year}-01-01") }),
    )
    .await;
    let bank = account(&app, &mut owner, "1800").await["id"].clone();
    let deposits = account(&app, &mut owner, "2180").await["id"].clone();
    let meals = account(&app, &mut owner, "6674").await;
    assert_eq!(meals["euer_line"], "64");
    for (number, from, amount) in [
        ("6640", &bank, 10_001),
        ("6610", &bank, 2_000),
        ("6620", &bank, 8_000),
        // lump sums aren't paid from the business account
        ("6674", &deposits, 2_800),
    ] {
        let id = account(&app, &mut owner, number).await["id"].clone();
        app.ok(
            &mut owner,
            "POST",
            "/api/postings",
            json!({ "date": today, "description": "Ausgabe", "lines": [
                { "account_id": id, "amount": amount },
                { "account_id": from, "amount": -amount }
            ] }),
        )
        .await;
    }
    let euer = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/tax/euer?year={year}"),
            json!({}),
        )
        .await;
    assert_eq!(
        amounts(&euer["lines"]),
        expect(&[
            ("62", 2_000),
            ("62_nd", 8_000),
            ("63", 7_001),
            ("63_nd", 3_000),
            ("64", 2_800)
        ])
    );
    let kennzahlen: Vec<_> = euer["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["kennzahl"].as_str().unwrap())
        .collect();
    assert_eq!(kennzahlen, ["174", "164", "175", "165", "171"]);
    assert_eq!(euer["expenses"], 2_000 + 7_001 + 2_800);
    assert_eq!(euer["profit"], -11_801);

    let entries: Vec<_> = euer["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["key"].as_str().unwrap(),
                e["number"].as_str().unwrap(),
                e["amount"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        entries,
        [
            ("62", "6610", 2_000),
            ("62_nd", "6620", 8_000),
            ("63", "6640", 7_001),
            ("63_nd", "6640", 3_000),
            ("64", "6674", 2_800)
        ]
    );
    assert_eq!(euer["deposits"], 2_800);
    assert_eq!(euer["withdrawals"], 0);
}
