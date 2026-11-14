use crate::common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::config::Config;
use serde_json::{Value, json};

#[tokio::test]
async fn a_quote_is_issued_accepted_confirmed_and_duplicated() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let quote = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({
                "kind": "quote", "customer_id": customer, "title": "Neue Website",
                "pricing": "effort", "intro": "Vielen Dank für Ihre Anfrage.",
                "lines": [{ "description": "Umsetzung", "quantity": "20", "unit": "hour",
                            "unit_price": 9000 }]
            }),
        )
        .await;
    assert_eq!(quote["pricing"], "effort");
    let id = quote["id"].as_i64().unwrap();
    let path = |action: &str| format!("/api/documents/{id}/{action}");

    let early = app
        .as_browser(
            &mut owner,
            "POST",
            &path("follow-up"),
            json!({ "kind": "order_confirmation" }),
        )
        .await;
    assert_eq!(
        early.status(),
        StatusCode::CONFLICT,
        "a draft has no follow-ups"
    );

    let issued = app.ok(&mut owner, "POST", &path("issue"), json!({})).await;
    assert_eq!(issued["quote_state"], "open");
    let issue_date: jiff::civil::Date = issued["issue_date"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        issued["valid_until"],
        issue_date
            .saturating_add(jiff::Span::new().days(30))
            .to_string()
            .as_str(),
        "a quote stands for 30 days unless set otherwise"
    );
    assert_eq!(issued["data"]["pricing"], "effort");

    let unaccepted = app
        .as_browser(
            &mut owner,
            "POST",
            &path("follow-up"),
            json!({ "kind": "order_confirmation" }),
        )
        .await;
    assert_eq!(json(unaccepted).await["code"], "quote_not_accepted");

    let accepted = app
        .ok(
            &mut owner,
            "POST",
            &path("outcome"),
            json!({ "outcome": "accepted" }),
        )
        .await;
    assert_eq!(accepted["quote_state"], "accepted");
    let again = app
        .as_browser(
            &mut owner,
            "POST",
            &path("outcome"),
            json!({ "outcome": "declined" }),
        )
        .await;
    assert_eq!(json(again).await["code"], "quote_decided");

    let confirmation = app
        .ok(
            &mut owner,
            "POST",
            &path("follow-up"),
            json!({ "kind": "order_confirmation" }),
        )
        .await;
    assert_eq!(confirmation["kind"], "order_confirmation");
    assert_eq!(confirmation["source_id"], id);
    assert_eq!(confirmation["title"], "Neue Website");
    assert_eq!(
        confirmation["intro"],
        Value::Null,
        "texts are written per document"
    );
    assert_eq!(confirmation["lines"][0]["unit_price"], 9000);
    let number = issued["number"].as_str().unwrap();
    assert_eq!(
        confirmation["data"]["reference"],
        format!("Zu Angebot {number}").as_str()
    );
    let confirmed = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", confirmation["id"]),
            json!({}),
        )
        .await;
    assert!(confirmed["number"].as_str().unwrap().starts_with("AB-"));

    let delivery = app
        .ok(
            &mut owner,
            "POST",
            &path("follow-up"),
            json!({ "kind": "delivery_note" }),
        )
        .await;
    assert_eq!(delivery["lines"][0]["unit_price"], Value::Null);
    assert_eq!(delivery["data"]["totals"], Value::Null);

    let quote = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/documents/{id}"),
            json!({}),
        )
        .await;
    assert_eq!(quote["follow_ups"].as_array().unwrap().len(), 2);

    let copy = app
        .ok(&mut owner, "POST", &path("duplicate"), json!({}))
        .await;
    assert_eq!(copy["status"], "draft");
    assert_eq!(copy["number"], Value::Null);
    assert_eq!(copy["valid_until"], Value::Null);
    assert_eq!(copy["intro"], "Vielen Dank für Ihre Anfrage.");
    assert_ne!(copy["id"], id);

    let list = app
        .ok(&mut owner, "GET", "/api/documents?kind=quote", json!({}))
        .await;
    let states: Vec<&Value> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|d| &d["quote_state"])
        .collect();
    assert_eq!(states, [&Value::Null, &json!("accepted")]);
}

#[tokio::test]
async fn validity_and_pricing_belong_to_quotes() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let response = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "valid_until": "2030-01-01",
                    "pricing": "fixed" }),
        )
        .await;
    let body = json(response).await;
    assert_eq!(body["fields"]["valid_until"], "invalid_format");
    assert_eq!(body["fields"]["pricing"], "invalid_format");

    let past = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "quote", "customer_id": customer, "valid_until": "2020-01-01",
                    "lines": [{ "description": "X", "quantity": "1", "unit": "piece", "unit_price": 100 }] }),
        )
        .await;
    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", past["id"]),
            json!({}),
        )
        .await;
    assert_eq!(
        json(refused).await["fields"]["valid_until"],
        "invalid_format"
    );
}

#[tokio::test]
async fn snippets_are_kept_per_language() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut accountant = app.with_role(&mut owner, "accountant").await;
    let snippet = app
        .ok(
            &mut owner,
            "POST",
            "/api/snippets",
            json!({ "name": "Gruß", "language": "de", "body": "Mit freundlichen Grüßen" }),
        )
        .await;
    let path = format!("/api/snippets/{}", snippet["id"]);
    app.ok(
        &mut owner,
        "PUT",
        &path,
        json!({ "name": "Gruß", "language": "de", "body": "Viele Grüße" }),
    )
    .await;
    let list = app.ok(&mut owner, "GET", "/api/snippets", json!({})).await;
    assert_eq!(list[0]["body"], "Viele Grüße");
    let forbidden = app
        .as_browser(&mut accountant, "GET", "/api/snippets", json!({}))
        .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    let deleted = app.as_browser(&mut owner, "DELETE", &path, json!({})).await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
}
