mod common;

use axum::http::{StatusCode, header};
use common::{json as body, start};
use compact_company::config::Config;
use http_body_util::BodyExt;
use serde_json::json;

#[tokio::test]
async fn a_years_journal_exports_as_a_datev_batch_whose_rows_balance_with_it() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    app.ok(
        &mut owner,
        "PUT",
        "/api/business",
        json!({
            "name": "swilk UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
            "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
            "taxation": "accrual", "iban": "DE89370400440532013000",
            "datev_consultant_number": 29098, "datev_client_number": 10042
        }),
    )
    .await;
    let invalid = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "datev_consultant_number": 12 }),
        )
        .await;
    assert_eq!(
        body(invalid).await["fields"]["datev_consultant_number"],
        "invalid_format"
    );
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": "2026-01-01" }),
    )
    .await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Wartung", "quantity": "1", "unit": "piece", "unit_price": 10000 }
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
    let date = invoice["issue_date"].as_str().unwrap().to_owned();
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/documents/{}/payments", invoice["id"]),
        json!({ "date": date, "amount": 11_900 }),
    )
    .await;

    let year = &date[..4];
    let path = format!("/api/exports/datev?from={year}-01-01&to={year}-12-31");
    let mut accountant = app.with_role(&mut owner, "accountant").await;
    let response = app
        .as_browser(&mut accountant, "GET", &path, json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/csv; charset=windows-1252"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text: String = bytes.iter().map(|b| char::from(*b)).collect();
    let lines: Vec<&str> = text.trim_end().split("\r\n").collect();
    assert!(lines[0].contains(&format!(";29098;10042;{year}0101;4;{year}0101;{year}1231;")));
    assert!(lines[1].starts_with("Umsatz (ohne Soll/Haben-Kz);"));
    let rows: Vec<Vec<&str>> = lines[2..].iter().map(|l| l.split(';').collect()).collect();
    let cents = |f: &str| f.replace(',', "").parse::<i64>().unwrap();
    let pairs: Vec<(&str, &str, i64)> = rows.iter().map(|r| (r[6], r[7], cents(r[0]))).collect();
    assert_eq!(
        pairs,
        vec![
            ("1200", "4400", 10_000),
            ("1200", "3806", 1_900),
            ("1800", "1200", 11_900)
        ]
    );
    assert_eq!(
        rows[0][8], "\"40\"",
        "the revenue account books no VAT itself"
    );
    assert_eq!(rows[1][8], "");
    let number = invoice["number"].as_str().unwrap();
    assert!(rows.iter().all(|r| r[10] == format!("\"{number}\"")));
    assert_eq!(rows[0][9], format!("{}{}", &date[8..10], &date[5..7]));

    let journal = app.ok(&mut owner, "GET", "/api/postings", json!({})).await;
    let debits: i64 = journal
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["lines"].as_array().unwrap().clone())
        .map(|l| l["amount"].as_i64().unwrap().max(0))
        .sum();
    assert_eq!(pairs.iter().map(|p| p.2).sum::<i64>(), debits);

    let spanning = app
        .as_browser(
            &mut owner,
            "GET",
            "/api/exports/datev?from=2026-12-01&to=2027-01-31",
            json!({}),
        )
        .await;
    assert_eq!(body(spanning).await["fields"]["to"], "invalid_format");
    let mut staff = app.with_role(&mut owner, "staff").await;
    let refused = app.as_browser(&mut staff, "GET", &path, json!({})).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
}
