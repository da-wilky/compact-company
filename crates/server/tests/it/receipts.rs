use crate::common;

use std::collections::BTreeMap;

use axum::http::{StatusCode, header};
use common::{Browser, TestApp, json as body, start};
use compact_company::config::Config;
use http_body_util::BodyExt;
use serde_json::{Value, json};

const UBL: &[u8] = include_bytes!("../fixtures/xrechnung-ubl.xml");

async fn upload(app: &TestApp, owner: &mut Browser, name: &str, bytes: &[u8]) -> Value {
    let response = app
        .multipart(
            owner,
            "POST",
            "/api/receipts",
            &[("file", Some(name), bytes)],
        )
        .await;
    let status = response.status();
    let value = body(response).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}

async fn account(app: &TestApp, owner: &mut Browser, number: &str) -> i64 {
    app.ok(owner, "GET", "/api/accounts", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == number)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

async fn last_posting(app: &TestApp, owner: &mut Browser) -> BTreeMap<String, i64> {
    let journal = app.ok(owner, "GET", "/api/postings", json!({})).await;
    journal
        .as_array()
        .unwrap()
        .iter()
        .max_by_key(|p| p["id"].as_i64())
        .unwrap()["lines"]
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

async fn business(app: &TestApp, owner: &mut Browser, taxation: &str) {
    app.ok(
        owner,
        "PUT",
        "/api/business",
        json!({
            "name": "swilk UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
            "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
            "vat_id": "DE123456789", "taxation": taxation, "iban": "DE89370400440532013000"
        }),
    )
    .await;
}

#[tokio::test]
async fn e_invoices_and_a_photo_become_booked_receipts_with_their_input_vat() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    business(&app, &mut owner, "accrual").await;
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr03", "start_date": "2026-01-01" }),
    )
    .await;

    // a ZUGFeRD PDF, as this app issues them
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Wartung", "quantity": "2", "unit": "hour", "unit_price": 5000 }
            ] }),
        )
        .await;
    let invoice = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", draft["id"]),
            json!({}),
        )
        .await;
    let pdf = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/documents/{}/pdf", draft["id"]),
            json!({}),
        )
        .await
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let zugferd = upload(&app, &mut owner, "rechnung.pdf", &pdf).await;
    assert_eq!(zugferd["format"], "zugferd");
    assert_eq!(zugferd["number"], invoice["number"]);
    assert_eq!(zugferd["supplier_name"], "swilk UG (haftungsbeschränkt)");
    assert_eq!(zugferd["date"], invoice["issue_date"]);
    assert_eq!(zugferd["lines"][0]["net"], 10_000);
    assert_eq!(zugferd["lines"][0]["tax"], 1_900);
    assert_eq!(zugferd["lines"][0]["description"], "Wartung");
    assert_eq!(zugferd["missing"], json!({}));
    let id = zugferd["id"].as_i64().unwrap();
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/receipts/{id}/book"),
        json!({}),
    )
    .await;
    assert_eq!(
        last_posting(&app, &mut owner).await,
        expect(&[("4900", 10_000), ("1576", 1_900), ("1600", -11_900)])
    );
    let unchanged = app
        .as_browser(
            &mut owner,
            "PUT",
            &format!("/api/receipts/{id}"),
            json!({ "lines": [] }),
        )
        .await;
    assert_eq!(body(unchanged).await["code"], "issued");
    let paid = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/receipts/{id}/payments"),
            json!({ "date": zugferd["date"], "amount": 11_900 }),
        )
        .await;
    assert_eq!(paid["amount_due"], 0);
    assert_eq!(
        last_posting(&app, &mut owner).await,
        expect(&[("1600", 11_900), ("1200", -11_900)])
    );

    // an XRechnung in UBL from a supplier not known yet
    let ubl = upload(&app, &mut owner, "xrechnung.xml", UBL).await;
    assert_eq!(ubl["format"], "ubl");
    assert_eq!(ubl["number"], "HA-2026-0815");
    assert_eq!(ubl["date"], "2026-03-02");
    assert_eq!(ubl["due_date"], "2026-03-16");
    assert_eq!(ubl["gross"], 14_040);
    assert_eq!(ubl["lines"][1]["description"], "Handbuch Serverbetrieb");
    assert_eq!(ubl["lines"][1]["vat_rate"], "7");
    let suppliers = app.ok(&mut owner, "GET", "/api/suppliers", json!({})).await;
    let hosting = suppliers
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Hosting & Mehr AG")
        .unwrap();
    assert_eq!(hosting["vat_id"], "DE811907980");
    assert_eq!(hosting["iban"], "DE02120300000000202051");
    assert_eq!(hosting["city"], "Nürnberg");
    let again = upload(&app, &mut owner, "xrechnung.xml", UBL).await;
    assert_eq!(again["duplicate_of"], ubl["id"]);
    assert_eq!(again["supplier_id"], hosting["id"], "known by its VAT ID");
    let response = app
        .as_browser(
            &mut owner,
            "DELETE",
            &format!("/api/receipts/{}", again["id"]),
            json!({}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/receipts/{}/book", ubl["id"]),
        json!({}),
    )
    .await;
    assert_eq!(
        last_posting(&app, &mut owner).await,
        expect(&[
            ("4900", 12_000),
            ("1576", 1_900),
            ("1571", 140),
            ("1600", -14_040)
        ])
    );
    let reversed = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/receipts/{}/reverse", ubl["id"]),
            json!({}),
        )
        .await;
    assert_eq!(reversed["reversed"], true);
    assert_eq!(reversed["amount_due"], 0);

    // a photo of a restaurant bill, entered by hand and paid in cash; as a small business
    business(&app, &mut owner, "small_business").await;
    let photo = upload(
        &app,
        &mut owner,
        "bon.jpg",
        &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16],
    )
    .await;
    assert_eq!(photo["format"], Value::Null);
    assert_eq!(photo["missing"]["supplier_id"], "required");
    assert_eq!(photo["missing"]["lines"], "required");
    let file = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/receipts/{}/file", photo["id"]),
            json!({}),
        )
        .await;
    assert_eq!(file.headers()[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(file.headers()[header::X_FRAME_OPTIONS], "SAMEORIGIN");
    let restaurant = app
        .ok(
            &mut owner,
            "POST",
            "/api/suppliers",
            json!({ "name": "Gasthaus Linde", "city": "Dresden" }),
        )
        .await;
    let meals = account(&app, &mut owner, "4650").await;
    let path = format!("/api/receipts/{}", photo["id"]);
    let edited = app
        .ok(
            &mut owner,
            "PUT",
            &path,
            json!({ "supplier_id": restaurant["id"], "date": "2026-04-03", "lines": [
                { "description": "Geschäftsessen", "account_id": meals, "net": 4_000,
                  "vat_rate": "19" }
            ] }),
        )
        .await;
    assert_eq!(
        edited["lines"][0]["tax"], 760,
        "the tax at the rate when left out"
    );
    assert_eq!(edited["missing"], json!({}));
    let cash = account(&app, &mut owner, "1000").await;
    let booked = app
        .ok(
            &mut owner,
            "POST",
            &format!("{path}/book"),
            json!({ "paid_from": cash }),
        )
        .await;
    assert_eq!(booked["amount_due"], 0);
    let journal = app.ok(&mut owner, "GET", "/api/postings", json!({})).await;
    let mut journal = journal.as_array().unwrap().clone();
    journal.sort_by_key(|p| p["id"].as_i64());
    let lines = |posting: &Value| -> BTreeMap<String, i64> {
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
    };
    assert_eq!(
        lines(&journal[journal.len() - 2]),
        expect(&[("4650", 4_760), ("1600", -4_760)]),
        "a small business books the gross amount as expense"
    );
    assert_eq!(
        lines(&journal[journal.len() - 1]),
        expect(&[("1600", 4_760), ("1000", -4_760)])
    );
    let restaurant = app.ok(&mut owner, "GET", "/api/suppliers", json!({})).await;
    let restaurant = restaurant
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Gasthaus Linde")
        .unwrap()
        .clone();
    assert_eq!(
        restaurant["account_id"], meals,
        "remembered for the next receipt"
    );

    let not_an_invoice = app
        .multipart(
            &mut owner,
            "POST",
            "/api/receipts",
            &[(
                "file",
                Some("x.xml"),
                b"<html><body>hi</body></html>".as_slice(),
            )],
        )
        .await;
    assert_eq!(
        body(not_an_invoice).await["fields"]["file"],
        "invalid_format"
    );
    let open = app
        .ok(&mut owner, "GET", "/api/receipts?open=true", json!({}))
        .await;
    assert_eq!(open, json!([]));
}
