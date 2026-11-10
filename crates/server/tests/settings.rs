mod common;

use axum::http::StatusCode;
use common::{json, start};

async fn owner_app(config: Config) -> (common::TestApp, common::Browser) {
    let app = start(config).await;
    let owner = app.setup_owner().await;
    (app, owner)
}
use compact_company::{config::Config, settings::Business};
use serde_json::json;

fn pinning(business: Business) -> Config {
    Config {
        business,
        ..Config::default()
    }
}

#[tokio::test]
async fn starts_on_an_empty_data_dir() {
    let app = start(Config::default()).await;
    assert!(app.dir.path().join("data/compact-company.db").exists());
    assert_eq!(
        app.app.settings.business().await.unwrap(),
        Business::default()
    );
}

#[tokio::test]
async fn business_is_stored_and_read_back() {
    let (app, mut owner) = owner_app(Config::default()).await;
    let response = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "name": " Wilk IT ", "vat_id": "de 123456789", "small_business": true }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let view = json(
        app.as_browser(&mut owner, "GET", "/api/business", json!({}))
            .await,
    )
    .await;
    assert_eq!(view["values"]["name"], "Wilk IT");
    assert_eq!(view["values"]["vat_id"], "DE123456789");
    assert_eq!(view["values"]["small_business"], true);
    assert_eq!(view["pinned"], json!([]));
}

#[tokio::test]
async fn invalid_input_reports_field_errors() {
    let (app, mut owner) = owner_app(Config::default()).await;
    let response = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "name": "x".repeat(201), "vat_id": "123" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json(response).await,
        json!({
            "code": "invalid_input",
            "message": "error.invalid_input",
            "fields": { "name": "too_long", "vat_id": "invalid_format" }
        })
    );
}

#[tokio::test]
async fn pinned_keys_win_and_cannot_be_changed() {
    let (app, mut owner) = owner_app(pinning(Business {
        name: Some("Pinned".into()),
        ..Business::default()
    }))
    .await;
    let response = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "name": "Other" }),
        )
        .await;
    assert_eq!(json(response).await["fields"], json!({ "name": "pinned" }));

    let response = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "name": "Pinned", "vat_id": "DE123456789" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let view = json(response).await;
    assert_eq!(view["values"]["name"], "Pinned");
    assert_eq!(view["values"]["vat_id"], "DE123456789");
    assert_eq!(view["pinned"], json!(["name"]));
}

#[tokio::test]
async fn a_save_may_leave_out_pinned_keys() {
    let (app, mut owner) = owner_app(pinning(Business {
        name: Some("Pinned".into()),
        ..Business::default()
    }))
    .await;
    let response = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "vat_id": "DE123456789" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["values"]["name"], "Pinned");
    let log = app
        .as_browser(&mut owner, "GET", "/api/audit?subject=business", json!({}))
        .await;
    assert_eq!(
        json(log).await[0]["after"],
        json!({ "vat_id": "DE123456789" })
    );
}
