mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::{TestApp, json as body, start};
use compact_company::config::Config;
use serde_json::{Value, json};

async fn with_token(
    app: &TestApp,
    token: &str,
    method: &str,
    path: &str,
    payload: Value,
) -> axum::response::Response {
    app.request(
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap(),
    )
    .await
}

#[tokio::test]
async fn a_work_token_tracks_time_and_is_refused_issuing_and_booking() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let project = app
        .ok(
            &mut owner,
            "POST",
            "/api/projects",
            json!({ "customer_id": customer, "name": "Relaunch" }),
        )
        .await;
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

    let none = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/tokens",
            json!({ "name": "x", "scopes": [] }),
        )
        .await;
    assert_eq!(body(none).await["fields"]["scopes"], "required");
    let created = app
        .ok(
            &mut owner,
            "POST",
            "/api/tokens",
            json!({ "name": "Zeitimport", "scopes": ["work:write", "work:read", "documents:write"] }),
        )
        .await;
    assert_eq!(
        created["scopes"],
        json!(["work:read", "work:write", "documents:write"])
    );
    let token = created["secret"].as_str().unwrap().to_owned();
    assert!(token.starts_with("cc_"));
    let listed = app.ok(&mut owner, "GET", "/api/tokens", json!({})).await;
    assert_eq!(listed[0]["secret"], Value::Null, "shown once");

    let start_at = 1_790_000_000;
    let entry = with_token(
        &app,
        &token,
        "POST",
        "/api/time",
        json!({ "project_id": project["id"], "started_at": start_at, "ended_at": start_at + 3600 }),
    )
    .await;
    assert_eq!(
        entry.status(),
        StatusCode::OK,
        "no CSRF token for a bearer token"
    );
    let entries = with_token(
        &app,
        &token,
        "GET",
        &format!("/api/time?from={start_at}&to={}", start_at + 7200),
        json!({}),
    )
    .await;
    assert_eq!(body(entries).await.as_array().unwrap().len(), 1);
    let drafted = with_token(
        &app,
        &token,
        "PUT",
        &format!("/api/documents/{}", draft["id"]),
        json!({ "kind": "invoice", "customer_id": customer, "lines": [
            { "description": "Wartung", "quantity": "2", "unit": "piece", "unit_price": 10000 }
        ] }),
    )
    .await;
    assert_eq!(drafted.status(), StatusCode::OK);

    for (method, path) in [
        ("POST", format!("/api/documents/{}/issue", draft["id"])),
        ("POST", format!("/api/documents/{}/send", draft["id"])),
        ("POST", "/api/postings".to_owned()),
        ("GET", "/api/postings".to_owned()),
        ("GET", "/api/users".to_owned()),
        ("GET", "/api/tokens".to_owned()),
        ("PUT", "/api/business".to_owned()),
    ] {
        let refused = with_token(&app, &token, method, &path, json!({})).await;
        assert_eq!(refused.status(), StatusCode::FORBIDDEN, "{method} {path}");
    }

    let mut staff = app.with_role(&mut owner, "staff").await;
    let books = app
        .ok(
            &mut staff,
            "POST",
            "/api/tokens",
            json!({ "name": "Bericht", "scopes": ["books:read"] }),
        )
        .await;
    let refused = with_token(
        &app,
        books["secret"].as_str().unwrap(),
        "GET",
        "/api/postings",
        json!({}),
    )
    .await;
    assert_eq!(
        refused.status(),
        StatusCode::FORBIDDEN,
        "a token has no more than its user's role"
    );

    let foreign = app
        .as_browser(
            &mut staff,
            "DELETE",
            &format!("/api/tokens/{}", created["id"]),
            json!({}),
        )
        .await;
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    app.ok(
        &mut owner,
        "DELETE",
        &format!("/api/tokens/{}", created["id"]),
        json!({}),
    )
    .await;
    let revoked = with_token(&app, &token, "GET", "/api/customers", json!({})).await;
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
}
