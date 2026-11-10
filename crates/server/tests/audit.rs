mod common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::config::Config;
use serde_json::{Value, json};

async fn entries(app: &common::TestApp, browser: &mut common::Browser, query: &str) -> Vec<Value> {
    let response = app
        .as_browser(browser, "GET", &format!("/api/audit{query}"), json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    json(response).await.as_array().unwrap().clone()
}

#[tokio::test]
async fn business_and_role_changes_are_logged_with_old_and_new_values() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    app.as_browser(
        &mut owner,
        "PUT",
        "/api/business",
        json!({ "name": "Wilk IT" }),
    )
    .await;
    app.as_browser(
        &mut owner,
        "PUT",
        "/api/business",
        json!({ "name": "Wilk IT GmbH" }),
    )
    .await;
    let mut alex = app.with_role(&mut owner, "staff").await;
    let users = json(
        app.as_browser(&mut owner, "GET", "/api/users", json!({}))
            .await,
    )
    .await;
    let alex_id = users[1]["id"].as_i64().unwrap();
    app.as_browser(
        &mut owner,
        "PATCH",
        &format!("/api/users/{alex_id}"),
        json!({ "role": "accountant" }),
    )
    .await;

    let business = entries(&app, &mut owner, "?subject=business").await;
    assert_eq!(business.len(), 2);
    assert_eq!(business[0]["action"], "business.update");
    assert_eq!(business[0]["user_name"], "Samuel");
    assert_eq!(business[0]["before"], json!({ "name": "Wilk IT" }));
    assert_eq!(business[0]["after"], json!({ "name": "Wilk IT GmbH" }));
    assert_eq!(business[1]["before"], json!({ "name": null }));

    let alex_log = entries(&app, &mut alex, &format!("?subject=user:{alex_id}")).await;
    assert_eq!(alex_log[0]["action"], "user.update");
    assert_eq!(alex_log[0]["before"], json!({ "role": "staff" }));
    assert_eq!(alex_log[0]["after"], json!({ "role": "accountant" }));
    assert_eq!(alex_log[1]["action"], "user.create");
}

#[tokio::test]
async fn a_failed_change_leaves_no_entry() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let before = entries(&app, &mut owner, "").await.len();

    let users = json(
        app.as_browser(&mut owner, "GET", "/api/users", json!({}))
            .await,
    )
    .await;
    let id = users[0]["id"].as_i64().unwrap();
    let refused = app
        .as_browser(
            &mut owner,
            "PATCH",
            &format!("/api/users/{id}"),
            json!({ "role": "staff" }),
        )
        .await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    let invalid = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "name": "Wilk IT", "vat_id": "x" }),
        )
        .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(entries(&app, &mut owner, "").await.len(), before);
}

#[tokio::test]
async fn an_unchanged_save_leaves_no_entry() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    for _ in 0..2 {
        app.as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "name": "Wilk IT" }),
        )
        .await;
    }
    assert_eq!(
        entries(&app, &mut owner, "?action=business.update")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn entries_cannot_be_changed_or_deleted() {
    let app = start(Config::default()).await;
    app.setup_owner().await;
    let update = sqlx::query("UPDATE audit SET action = 'forged'")
        .execute(&app.app.db)
        .await;
    assert!(
        update
            .unwrap_err()
            .to_string()
            .contains("cannot be changed")
    );
    let delete = sqlx::query("DELETE FROM audit").execute(&app.app.db).await;
    assert!(
        delete
            .unwrap_err()
            .to_string()
            .contains("cannot be deleted")
    );
}

#[tokio::test]
async fn setup_records_the_owner_and_their_passkey() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let actions: Vec<_> = entries(&app, &mut owner, "")
        .await
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actions, ["passkey.add", "user.create"]);
}
