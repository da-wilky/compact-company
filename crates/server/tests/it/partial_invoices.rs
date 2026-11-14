use crate::common;

use std::collections::BTreeMap;

use common::{Browser, TestApp, json as body, start, text};
use compact_company::config::Config;
use serde_json::{Value, json};

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

/// An accepted quote over 10,000.00 net.
async fn accepted_quote(app: &TestApp, owner: &mut Browser, customer: i64) -> i64 {
    let draft = app
        .ok(
            owner,
            "POST",
            "/api/documents",
            json!({ "kind": "quote", "customer_id": customer, "lines": [
                { "description": "Website", "quantity": "1", "unit": "lump_sum",
                  "unit_price": 1_000_000, "vat_rate": "19" }
            ] }),
        )
        .await;
    let id = draft["id"].as_i64().unwrap();
    app.ok(
        owner,
        "POST",
        &format!("/api/documents/{id}/issue"),
        json!({}),
    )
    .await;
    app.ok(
        owner,
        "POST",
        &format!("/api/documents/{id}/outcome"),
        json!({ "outcome": "accepted" }),
    )
    .await;
    id
}

async fn follow_up(app: &TestApp, owner: &mut Browser, quote: i64, input: Value) -> Value {
    app.ok(
        owner,
        "POST",
        &format!("/api/documents/{quote}/follow-up"),
        input,
    )
    .await
}

async fn issue(app: &TestApp, owner: &mut Browser, id: &Value) -> Value {
    app.ok(
        owner,
        "POST",
        &format!("/api/documents/{id}/issue"),
        json!({}),
    )
    .await
}

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

#[tokio::test]
async fn two_partial_invoices_close_with_a_final_invoice_deducting_the_paid_one() {
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
        let vat = |net: i64| if small { 0 } else { net * 19 / 100 };
        let quote = accepted_quote(&app, &mut owner, customer).await;

        let first = follow_up(
            &app,
            &mut owner,
            quote,
            json!({ "kind": "invoice", "invoice_type": "partial", "percent": "30" }),
        )
        .await;
        assert_eq!(first["lines"][0]["unit_price"], 300_000);
        let first = issue(&app, &mut owner, &first["id"]).await;
        assert_eq!(first["data"]["invoice_type"], "partial");
        let xml = text(
            app.as_browser(
                &mut owner,
                "GET",
                &format!("/api/documents/{}/xml", first["id"]),
                json!({}),
            )
            .await,
        )
        .await;
        assert!(xml.contains("<ram:TypeCode>326</ram:TypeCode>"), "{xml}");
        // received before the work is done, in an earlier VAT period
        app.ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/payments", first["id"]),
            json!({ "date": "2026-01-15", "amount": 300_000 + vat(300_000) }),
        )
        .await;

        let second = follow_up(
            &app,
            &mut owner,
            quote,
            json!({ "kind": "invoice", "invoice_type": "partial", "percent": "20" }),
        )
        .await;
        let second = issue(&app, &mut owner, &second["id"]).await;
        let time = app
            .as_browser(
                &mut owner,
                "POST",
                &format!("/api/documents/{}/time", second["id"]),
                json!({ "group": "project" }),
            )
            .await;
        assert_eq!(body(time).await["code"], "issued");

        let draft = follow_up(
            &app,
            &mut owner,
            quote,
            json!({ "kind": "invoice", "invoice_type": "final" }),
        )
        .await;
        let states: Vec<_> = draft["partials"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["state"].as_str().unwrap())
            .collect();
        assert_eq!(
            states,
            ["deducted", "cancelled"],
            "the owner is told before issuing"
        );
        let fin = issue(&app, &mut owner, &draft["id"]).await;
        let gross = 1_000_000 + vat(1_000_000);
        let prepaid = 300_000 + vat(300_000);
        let major = |cents: i64| format!("{}.{:02}", cents / 100, cents % 100);
        assert_eq!(fin["data"]["totals"]["gross"], major(gross));
        assert_eq!(fin["data"]["totals"]["prepaid"], major(prepaid));
        assert_eq!(fin["data"]["totals"]["due"], major(gross - prepaid));
        let deduction = &fin["data"]["deductions"][0];
        assert_eq!(deduction["number"], first["number"]);
        assert_eq!(deduction["vat"][0]["net"], "3000.00");
        assert_eq!(fin["amount_due"]["minor"], gross - prepaid);

        let second = app
            .ok(
                &mut owner,
                "GET",
                &format!("/api/documents/{}", second["id"]),
                json!({}),
            )
            .await;
        assert_eq!(
            second["amount_due"]["minor"], 0,
            "the unpaid one is cancelled"
        );
        assert_eq!(second["follow_ups"][0]["kind"], "cancellation");
        assert_eq!(second["follow_ups"][0]["status"], "issued");

        let xml = text(
            app.as_browser(
                &mut owner,
                "GET",
                &format!("/api/documents/{}/xml", fin["id"]),
                json!({}),
            )
            .await,
        )
        .await;
        assert!(xml.contains("<ram:TypeCode>380</ram:TypeCode>"), "{xml}");
        assert!(xml.contains(&format!(
            "<ram:TotalPrepaidAmount>{}</ram:TotalPrepaidAmount>",
            major(prepaid)
        )));
        assert!(xml.contains(&format!(
            "<ram:DuePayableAmount>{}</ram:DuePayableAmount>",
            major(gross - prepaid)
        )));
        assert!(xml.contains(&format!(
            "<ram:IssuerAssignedID>{}</ram:IssuerAssignedID>",
            first["number"].as_str().unwrap()
        )));

        for id in [&first["id"], &fin["id"]] {
            let refused = app
                .as_browser(
                    &mut owner,
                    "POST",
                    &format!("/api/documents/{id}/follow-up"),
                    json!({ "kind": "cancellation" }),
                )
                .await;
            assert_eq!(body(refused).await["code"], "final_invoice_settled");
        }

        let today = fin["issue_date"].as_str().unwrap().to_owned();
        app.ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/payments", fin["id"]),
            json!({ "date": today, "amount": gross - prepaid }),
        )
        .await;
        let open = app
            .ok(&mut owner, "GET", "/api/open-items", json!({}))
            .await;
        assert_eq!(open["invoices"], json!([]), "{taxation}");

        let euer = app
            .ok(&mut owner, "GET", "/api/tax/euer?year=2026", json!({}))
            .await;
        let income = if small {
            expect(&[("12", 1_000_000)])
        } else {
            expect(&[("15", 1_000_000), ("17", 190_000)])
        };
        assert_eq!(amounts(&euer["lines"]), income, "{taxation}");

        let period = |from: &str, to: &str| format!("/api/tax/vat-return?from={from}&to={to}");
        let january = app
            .ok(
                &mut owner,
                "GET",
                &period("2026-01-01", "2026-01-31"),
                json!({}),
            )
            .await;
        let now = app
            .ok(&mut owner, "GET", &period(&today, &today), json!({}))
            .await;
        if small {
            assert_eq!(january["figures"], json!([]));
            continue;
        }
        let figures = |base: i64| {
            expect(&[
                ("81", base),
                ("81_tax", vat(base)),
                ("86", 0),
                ("86_tax", 0),
                ("66", 0),
                ("83", vat(base)),
            ])
        };
        assert_eq!(
            amounts(&january["figures"]),
            figures(300_000),
            "{taxation}: when received"
        );
        assert_eq!(
            amounts(&now["figures"]),
            figures(700_000),
            "{taxation}: the rest"
        );
    }
}

#[tokio::test]
async fn a_partly_paid_partial_invoice_holds_up_its_final_invoice() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let quote = accepted_quote(&app, &mut owner, customer).await;
    let partial = follow_up(
        &app,
        &mut owner,
        quote,
        json!({ "kind": "invoice", "invoice_type": "partial" }),
    )
    .await;
    assert_eq!(
        partial["lines"][0]["unit_price"],
        Value::Null,
        "waits for an amount"
    );
    assert_eq!(partial["missing"]["lines.0.unit_price"], "required");
    let path = format!("/api/documents/{}", partial["id"]);
    app.ok(
        &mut owner,
        "PUT",
        &path,
        json!({ "kind": "invoice", "customer_id": customer, "invoice_type": "partial",
                "lines": [{ "description": "Abschlag", "quantity": "1", "unit": "lump_sum",
                            "unit_price": 250_000 }] }),
    )
    .await;
    issue(&app, &mut owner, &partial["id"]).await;
    app.ok(
        &mut owner,
        "POST",
        &format!("{path}/payments"),
        json!({ "date": "2026-02-01", "amount": 100_000 }),
    )
    .await;
    let draft = follow_up(
        &app,
        &mut owner,
        quote,
        json!({ "kind": "invoice", "invoice_type": "final" }),
    )
    .await;
    assert_eq!(draft["partials"][0]["state"], "partly_paid");
    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", draft["id"]),
            json!({}),
        )
        .await;
    assert_eq!(body(refused).await["code"], "partial_invoice_partly_paid");

    app.ok(
        &mut owner,
        "POST",
        &format!("{path}/payments"),
        json!({ "date": "2026-02-02", "amount": 197_500 }),
    )
    .await;
    let final_path = format!("/api/documents/{}", draft["id"]);
    app.ok(
        &mut owner,
        "PUT",
        &final_path,
        json!({ "kind": "invoice", "customer_id": customer, "invoice_type": "final",
                "lines": [{ "description": "Leistung", "quantity": "1", "unit": "lump_sum",
                            "unit_price": 200_000 }] }),
    )
    .await;
    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("{final_path}/issue"),
            json!({}),
        )
        .await;
    assert_eq!(body(refused).await["code"], "deductions_exceed_invoice");

    app.ok(
        &mut owner,
        "PUT",
        &final_path,
        json!({ "kind": "invoice", "customer_id": customer, "invoice_type": "final",
                "lines": [{ "description": "Leistung", "quantity": "1", "unit": "lump_sum",
                            "unit_price": 1_000_000 }] }),
    )
    .await;
    issue(&app, &mut owner, &draft["id"]).await;
    let paid = app.ok(&mut owner, "GET", &path, json!({})).await;
    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/payments/{}/reverse", paid["payments"][0]["id"]),
            json!({}),
        )
        .await;
    assert_eq!(
        body(refused).await["code"],
        "final_invoice_settled",
        "its VAT counted as due when the final invoice deducted it"
    );

    let percent = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/documents/{quote}/follow-up"),
            json!({ "kind": "invoice", "invoice_type": "final", "percent": "10" }),
        )
        .await;
    assert_eq!(body(percent).await["fields"]["percent"], "invalid_format");
}

#[tokio::test]
async fn a_final_invoice_from_the_order_confirmation_deducts_the_quotes_partial_invoices() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let quote = accepted_quote(&app, &mut owner, customer).await;
    let partial = follow_up(
        &app,
        &mut owner,
        quote,
        json!({ "kind": "invoice", "invoice_type": "partial", "percent": "30" }),
    )
    .await;
    issue(&app, &mut owner, &partial["id"]).await;
    let confirmation = follow_up(
        &app,
        &mut owner,
        quote,
        json!({ "kind": "order_confirmation" }),
    )
    .await;
    issue(&app, &mut owner, &confirmation["id"]).await;
    let draft = follow_up(
        &app,
        &mut owner,
        confirmation["id"].as_i64().unwrap(),
        json!({ "kind": "invoice", "invoice_type": "final" }),
    )
    .await;
    assert_eq!(draft["partials"][0]["id"], partial["id"]);
}
