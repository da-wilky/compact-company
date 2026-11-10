mod common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::config::Config;
use serde_json::json;

#[tokio::test]
async fn an_invited_staff_user_signs_in_and_is_refused_the_settings() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let token = app.invite(&mut owner, "Alex", "staff").await;

    let info = app
        .send_json("POST", "/api/invitation", json!({ "token": token }))
        .await;
    assert_eq!(json(info).await, json!({ "name": "Alex", "role": "staff" }));

    let mut staff = app.accept_with_passkey(&token).await.unwrap();
    let session = json(
        app.as_browser(&mut staff, "GET", "/api/session", json!({}))
            .await,
    )
    .await;
    assert_eq!(session["user"]["name"], "Alex");
    assert_eq!(session["user"]["role"], "staff");

    app.as_browser(&mut staff, "POST", "/api/sign-out", json!({}))
        .await;
    assert_eq!(app.sign_in(&mut staff).await.status(), StatusCode::OK);
    let settings = app
        .as_browser(&mut staff, "GET", "/api/business", json!({}))
        .await;
    assert_eq!(settings.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_invitation_is_used_once() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let token = app.invite(&mut owner, "Alex", "staff").await;
    app.accept_with_passkey(&token).await.unwrap();
    let again = *app.accept_with_passkey(&token).await.err().unwrap();
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(again).await["code"], "invitation_invalid");
}

#[tokio::test]
async fn a_revoked_invitation_cannot_be_accepted() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let token = app.invite(&mut owner, "Alex", "staff").await;
    let pending = json(
        app.as_browser(&mut owner, "GET", "/api/invitations", json!({}))
            .await,
    )
    .await;
    let id = pending[0]["id"].as_i64().unwrap();
    let revoked = app
        .as_browser(
            &mut owner,
            "DELETE",
            &format!("/api/invitations/{id}"),
            json!({}),
        )
        .await;
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    assert!(app.accept_with_passkey(&token).await.is_err());
}

#[tokio::test]
async fn the_owner_changes_roles_and_deactivates_users() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut alex = app.with_role(&mut owner, "staff").await;
    let users = json(
        app.as_browser(&mut owner, "GET", "/api/users", json!({}))
            .await,
    )
    .await;
    let id = users[1]["id"].as_i64().unwrap();
    assert_eq!(users[1]["passkeys"], 1);

    let changed = app
        .as_browser(
            &mut owner,
            "PATCH",
            &format!("/api/users/{id}"),
            json!({ "role": "accountant" }),
        )
        .await;
    assert_eq!(json(changed).await["role"], "accountant");
    let books = app
        .as_browser(&mut alex, "GET", "/api/business", json!({}))
        .await;
    assert_eq!(books.status(), StatusCode::OK);

    let changed = app
        .as_browser(
            &mut owner,
            "PATCH",
            &format!("/api/users/{id}"),
            json!({ "active": false }),
        )
        .await;
    assert_eq!(json(changed).await["active"], false);
    let refused = app
        .as_browser(&mut alex, "GET", "/api/business", json!({}))
        .await;
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    let sign_in = app.sign_in(&mut alex).await;
    assert_eq!(json(sign_in).await["code"], "passkey_rejected");
}

#[tokio::test]
async fn the_last_owner_stays_an_active_owner() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let users = json(
        app.as_browser(&mut owner, "GET", "/api/users", json!({}))
            .await,
    )
    .await;
    let id = users[0]["id"].as_i64().unwrap();
    for change in [json!({ "role": "staff" }), json!({ "active": false })] {
        let response = app
            .as_browser(&mut owner, "PATCH", &format!("/api/users/{id}"), change)
            .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(json(response).await["code"], "last_owner");
    }

    let mut second = app.with_role(&mut owner, "owner").await;
    let response = app
        .as_browser(
            &mut second,
            "PATCH",
            &format!("/api/users/{id}"),
            json!({ "role": "staff" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}
