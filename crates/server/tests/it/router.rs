use crate::common;

use axum::http::{StatusCode, header};
use common::{json, start, text};
use compact_company::config::Config;
use serde_json::json;

fn content_type(response: &axum::response::Response) -> &str {
    response.headers()[header::CONTENT_TYPE].to_str().unwrap()
}

#[tokio::test]
async fn health_reports_ok() {
    let app = start(Config::default()).await;
    let response = app.get("/api/health").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await, json!({ "status": "ok" }));
}

#[tokio::test]
async fn unknown_api_path_is_a_not_found_error() {
    let app = start(Config::default()).await;
    let response = app.get("/api/nope").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(content_type(&response), "application/json");
    assert_eq!(
        json(response).await,
        json!({ "code": "not_found", "message": "error.not_found" })
    );
}

#[tokio::test]
async fn root_serves_the_client() {
    let app = start(Config::default()).await;
    let response = app.get("/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(content_type(&response).starts_with("text/html"));
    assert!(text(response).await.contains("/_app/"));
}

#[tokio::test]
async fn client_routes_fall_back_to_the_client() {
    let app = start(Config::default()).await;
    let response = app.get("/customers/42").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.contains("/_app/"));
}

#[tokio::test]
async fn client_assets_are_served_with_their_type() {
    let app = start(Config::default()).await;
    let page = text(app.get("/").await).await;
    let start = page.find("/_app/immutable/").unwrap();
    let end = start + page[start..].find(".js").unwrap() + 3;
    let response = app.get(&page[start..end]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(content_type(&response), "text/javascript");
}

#[tokio::test]
async fn malformed_json_is_an_api_error() {
    let app = start(Config::default()).await;
    let response = app
        .send_json("POST", "/api/setup/start", json!({ "token": 1 }))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(response).await["code"], "malformed_request");
}

#[tokio::test]
async fn responses_carry_security_headers() {
    let app = start(Config::default()).await;
    let page = app.get("/").await;
    assert_eq!(page.headers()["x-frame-options"], "DENY");
    assert_eq!(page.headers()["x-content-type-options"], "nosniff");
    assert_eq!(page.headers()["cache-control"], "no-cache");
    assert!(
        text(page)
            .await
            .contains("http-equiv=\"content-security-policy\"")
    );
    let api = app.get("/api/health").await;
    assert_eq!(api.headers()["cache-control"], "no-store");
}
