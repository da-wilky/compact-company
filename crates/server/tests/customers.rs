mod common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::config::Config;
use serde_json::{Value, json};

fn customer(name: &str) -> Value {
    json!({ "name": name, "language": "de", "hourly_rate": 8500 })
}

#[tokio::test]
async fn customers_get_consecutive_permanent_numbers_and_are_archived_not_deleted() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut staff = app.with_role(&mut owner, "staff").await;

    let first = json(
        app.as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            customer("Bäckerei Krume"),
        )
        .await,
    )
    .await;
    let second = json(
        app.as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            customer("Hofmann Logistik"),
        )
        .await,
    )
    .await;
    assert_eq!(first["number"], "K-001");
    assert_eq!(second["number"], "K-002");
    assert_eq!(first["currency"], "EUR");

    let id = first["id"].as_i64().unwrap();
    let archived = app
        .as_browser(
            &mut owner,
            "PUT",
            &format!("/api/customers/{id}"),
            json!({ "name": "Bäckerei Krume", "language": "de", "archived": true }),
        )
        .await;
    assert_eq!(archived.status(), StatusCode::OK);
    assert_eq!(json(archived).await["number"], "K-001");

    let active = json(
        app.as_browser(&mut staff, "GET", "/api/customers", json!({}))
            .await,
    )
    .await;
    assert_eq!(active.as_array().unwrap().len(), 1);
    assert_eq!(active[0]["name"], "Hofmann Logistik");
    let old = json(
        app.as_browser(&mut staff, "GET", "/api/customers?archived=true", json!({}))
            .await,
    )
    .await;
    assert_eq!(old[0]["number"], "K-001");

    let third = json(
        app.as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            customer("Praxis Albers"),
        )
        .await,
    )
    .await;
    assert_eq!(third["number"], "K-003");

    let staff_edit = app
        .as_browser(
            &mut staff,
            "PUT",
            &format!("/api/customers/{id}"),
            customer("Anders"),
        )
        .await;
    assert_eq!(staff_edit.status(), StatusCode::FORBIDDEN);

    let log = json(
        app.as_browser(
            &mut owner,
            "GET",
            &format!("/api/audit?subject=customer:{id}"),
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(log[0]["action"], "customer.update");
    assert_eq!(
        log[0]["before"],
        json!({ "archived": false, "hourly_rate": 8500 })
    );
    assert_eq!(
        log[0]["after"],
        json!({ "archived": true, "hourly_rate": null })
    );
    assert_eq!(log[1]["action"], "customer.create");
}

#[tokio::test]
async fn invalid_customers_are_refused_per_field() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let response = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            json!({ "name": "  ", "language": "de", "email": "nope", "currency": "XYZ", "hourly_rate": -1 }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json(response).await["fields"],
        json!({ "name": "required", "email": "invalid_format", "currency": "invalid_format", "hourly_rate": "invalid_format" })
    );
    let huge = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            json!({ "name": "A", "language": "de", "hourly_rate": 100_000_001 }),
        )
        .await;
    assert_eq!(
        json(huge).await["fields"],
        json!({ "hourly_rate": "invalid_format" })
    );
    let missing = app
        .as_browser(&mut owner, "GET", "/api/customers/99", json!({}))
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn new_customers_use_the_business_currency() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let saved = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "currency": "CHF" }),
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let created = json(
        app.as_browser(&mut owner, "POST", "/api/customers", customer("Zürich AG"))
            .await,
    )
    .await;
    assert_eq!(created["currency"], "CHF");
}
