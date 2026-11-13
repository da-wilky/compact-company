use crate::common;

use axum::http::StatusCode;
use common::{Browser, json, start};
use compact_company::config::Config;
use serde_json::json;

#[tokio::test]
async fn a_fresh_instance_requires_setup() {
    let app = start(Config::default()).await;
    let session = json(app.get("/api/session").await).await;
    assert_eq!(
        session,
        json!({ "user": null, "csrf_token": null, "setup_required": true, "oidc": null })
    );
}

#[tokio::test]
async fn setup_needs_the_printed_token() {
    let app = start(Config::default()).await;
    let response = app
        .send_json(
            "POST",
            "/api/setup/start",
            json!({ "token": "guess", "name": "Mallory" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json(response).await["code"], "invalid_setup_token");
}

#[tokio::test]
async fn setup_creates_the_owner_once() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let session = json(
        app.as_browser(&mut owner, "GET", "/api/session", json!({}))
            .await,
    )
    .await;
    assert_eq!(session["user"]["name"], "Samuel");
    assert_eq!(session["user"]["role"], "owner");
    assert_eq!(session["setup_required"], false);

    let again = app
        .send_json(
            "POST",
            "/api/setup/start",
            json!({ "token": app.app.setup_token, "name": "Mallory" }),
        )
        .await;
    assert_eq!(json(again).await["code"], "setup_done");
}

#[tokio::test]
async fn owner_signs_out_and_back_in_and_edits_the_business() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let old_cookies = owner.cookies.clone();

    let response = app
        .as_browser(&mut owner, "POST", "/api/sign-out", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(owner.cookies.is_empty());

    let mut stale = Browser::new();
    stale.cookies = old_cookies;
    let response = app
        .as_browser(&mut stale, "GET", "/api/business", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app.sign_in(&mut owner).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "name": "Wilk IT" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["values"]["name"], "Wilk IT");
}

#[tokio::test]
async fn an_unknown_passkey_cannot_sign_in() {
    let app = start(Config::default()).await;
    app.setup_owner().await;
    let mut stranger = Browser::new();
    let other = start(Config::default()).await;
    let mut other_owner = other.setup_owner().await;
    stranger.authenticator =
        std::mem::replace(&mut other_owner.authenticator, Browser::new().authenticator);
    stranger.credential_ids = other_owner.credential_ids.clone();
    let response = app.sign_in(&mut stranger).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(response).await["code"], "passkey_rejected");
}

#[tokio::test]
async fn state_changes_need_the_csrf_token() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    owner.csrf_token = Some("forged".into());
    let response = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "name": "Evil" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json(response).await["code"], "csrf_token");
}

#[tokio::test]
async fn requests_without_a_session_are_unauthenticated() {
    let app = start(Config::default()).await;
    app.setup_owner().await;
    let response = app.get("/api/business").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json(response).await["code"], "unauthenticated");
}

#[tokio::test]
async fn a_ceremony_is_used_once() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let start = json(app.send_json("POST", "/api/sign-in/start", json!({})).await).await;
    let credential = owner.authenticate(&start["options"]);
    let body = json!({ "ceremony": start["ceremony"], "credential": credential });
    let first = app
        .send_json("POST", "/api/sign-in/finish", body.clone())
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let replay = app.send_json("POST", "/api/sign-in/finish", body).await;
    assert_eq!(json(replay).await["code"], "ceremony_expired");
}

#[tokio::test]
async fn passkeys_are_added_and_removed_but_never_the_last() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;

    let start = json(
        app.as_browser(&mut owner, "POST", "/api/passkeys/start", json!({}))
            .await,
    )
    .await;
    let credential = owner.register(&start["options"]);
    let list = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/passkeys/finish",
            json!({ "ceremony": start["ceremony"], "passkey_name": "Phone", "credential": credential }),
        )
        .await;
    let list = json(list).await;
    let names: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].clone())
        .collect();
    assert_eq!(names, [json!("Laptop"), json!("Phone")]);

    let first = list[0]["id"].as_i64().unwrap();
    let second = list[1]["id"].as_i64().unwrap();
    let response = app
        .as_browser(
            &mut owner,
            "DELETE",
            &format!("/api/passkeys/{first}"),
            json!({}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .as_browser(
            &mut owner,
            "DELETE",
            &format!("/api/passkeys/{second}"),
            json!({}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json(response).await["code"], "last_passkey");
}

#[tokio::test]
async fn passkey_names_are_required() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let start = json(
        app.as_browser(&mut owner, "POST", "/api/passkeys/start", json!({}))
            .await,
    )
    .await;
    let credential = owner.register(&start["options"]);
    let response = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/passkeys/finish",
            json!({ "ceremony": start["ceremony"], "passkey_name": "  ", "credential": credential }),
        )
        .await;
    assert_eq!(
        json(response).await["fields"],
        json!({ "passkey_name": "required" })
    );
}
