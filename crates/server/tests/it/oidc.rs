use crate::common;

use std::{
    collections::HashMap,
    fs,
    sync::{Arc, Mutex},
};

use axum::{
    Form, Json, Router,
    extract::State,
    http::{StatusCode, header},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::{Browser, TestApp, json, start};
use compact_company::config::Config;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::Sha256;

const SECRET: &str = "test-client-secret";

/// A minimal OpenID provider: discovery, and a token endpoint issuing HS256 ID tokens for codes
/// the test hands out.
#[derive(Clone)]
struct Provider {
    issuer: String,
    /// code → (subject, nonce)
    codes: Arc<Mutex<HashMap<String, (String, String)>>>,
}

async fn provider() -> Provider {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}/realm", listener.local_addr().unwrap());
    let provider = Provider {
        issuer,
        codes: Arc::default(),
    };
    let routes = Router::new()
        .route("/realm/.well-known/openid-configuration", get(discovery))
        .route("/realm/jwks", get(async || Json(json!({ "keys": [] }))))
        .route("/realm/token", post(token))
        .with_state(provider.clone());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    provider
}

async fn discovery(State(p): State<Provider>) -> Json<Value> {
    Json(json!({
        "issuer": p.issuer,
        "authorization_endpoint": format!("{}/authorize", p.issuer),
        "token_endpoint": format!("{}/token", p.issuer),
        "jwks_uri": format!("{}/jwks", p.issuer),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["HS256"],
    }))
}

async fn token(
    State(p): State<Provider>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    let (subject, nonce) = p
        .codes
        .lock()
        .unwrap()
        .remove(&form["code"])
        .ok_or(StatusCode::BAD_REQUEST)?;
    let now = compact_company::auth::now();
    let claims = json!({
        "iss": p.issuer, "sub": subject, "aud": "compact-company",
        "iat": now, "exp": now + 300, "nonce": nonce,
    });
    let encode = |v: &Value| URL_SAFE_NO_PAD.encode(v.to_string());
    let signed = format!(
        "{}.{}",
        encode(&json!({ "alg": "HS256", "typ": "JWT" })),
        encode(&claims)
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(SECRET.as_bytes()).unwrap();
    mac.update(signed.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    Ok(Json(json!({
        "access_token": "access", "token_type": "Bearer", "expires_in": 300,
        "id_token": format!("{signed}.{signature}"),
    })))
}

async fn app_with(provider: &Provider) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("secret");
    fs::write(&secret, SECRET).unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        format!(
            "[oidc]\nissuer = {:?}\nclient_id = \"compact-company\"\nclient_secret_file = {secret:?}\nname = \"Test IdP\"",
            provider.issuer
        ),
    )
    .unwrap();
    start(Config::load(&path).unwrap()).await
}

/// Goes through the provider as `subject` and returns where the callback sends the browser.
async fn sign_in_as(
    app: &TestApp,
    provider: &Provider,
    browser: &mut Browser,
    subject: &str,
    invitation: Option<&str>,
) -> String {
    let start = app
        .as_browser(
            browser,
            "POST",
            "/api/oidc/start",
            json!({ "invitation": invitation }),
        )
        .await;
    assert_eq!(start.status(), StatusCode::OK);
    let url = url::Url::parse(json(start).await["url"].as_str().unwrap()).unwrap();
    assert!(
        url.as_str()
            .starts_with(&format!("{}/authorize", provider.issuer))
    );
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    let code = format!("code-{subject}");
    provider
        .codes
        .lock()
        .unwrap()
        .insert(code.clone(), (subject.into(), query["nonce"].clone()));
    let callback = app
        .as_browser(
            browser,
            "GET",
            &format!("/api/oidc/callback?state={}&code={code}", query["state"]),
            json!({}),
        )
        .await;
    assert_eq!(callback.status(), StatusCode::SEE_OTHER);
    callback.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn an_invited_user_signs_up_and_in_through_the_provider() {
    let provider = provider().await;
    let app = app_with(&provider).await;
    let session = json(app.get("/api/session").await).await;
    assert_eq!(session["oidc"], "Test IdP");

    let mut owner = app.setup_owner().await;
    let token = app.invite(&mut owner, "Alex", "staff").await;
    let mut alex = Browser::new();
    let location = sign_in_as(&app, &provider, &mut alex, "alex", Some(&token)).await;
    assert_eq!(location, "/");
    let session = json(
        app.as_browser(&mut alex, "GET", "/api/session", json!({}))
            .await,
    )
    .await;
    assert_eq!(session["user"]["name"], "Alex");
    alex.csrf_token = session["csrf_token"].as_str().map(str::to_owned);
    let settings = app
        .as_browser(&mut alex, "GET", "/api/business", json!({}))
        .await;
    assert_eq!(settings.status(), StatusCode::FORBIDDEN);

    let mut later = Browser::new();
    assert_eq!(
        sign_in_as(&app, &provider, &mut later, "alex", None).await,
        "/"
    );
    let session = json(
        app.as_browser(&mut later, "GET", "/api/session", json!({}))
            .await,
    )
    .await;
    assert_eq!(session["user"]["name"], "Alex");
}

#[tokio::test]
async fn an_unknown_identity_creates_no_user() {
    let provider = provider().await;
    let app = app_with(&provider).await;
    let mut owner = app.setup_owner().await;
    let mut stranger = Browser::new();
    let location = sign_in_as(&app, &provider, &mut stranger, "mallory", None).await;
    assert_eq!(location, "/sign-in#error.oidc_unknown_identity");
    assert!(!stranger.cookies.contains_key("session"));
    let users = json(
        app.as_browser(&mut owner, "GET", "/api/users", json!({}))
            .await,
    )
    .await;
    assert_eq!(users.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn an_identity_links_to_one_user_only() {
    let provider = provider().await;
    let app = app_with(&provider).await;
    let mut owner = app.setup_owner().await;
    let first = app.invite(&mut owner, "Alex", "staff").await;
    let second = app.invite(&mut owner, "Alex again", "staff").await;
    sign_in_as(&app, &provider, &mut Browser::new(), "alex", Some(&first)).await;
    let location = sign_in_as(&app, &provider, &mut Browser::new(), "alex", Some(&second)).await;
    assert_eq!(location, "/sign-in#error.oidc_identity_taken");
}

#[tokio::test]
async fn a_callback_from_another_browser_is_refused() {
    let provider = provider().await;
    let app = app_with(&provider).await;
    let mut owner = app.setup_owner().await;
    let token = app.invite(&mut owner, "Mallory", "staff").await;
    let mut mallory = Browser::new();
    sign_in_as(&app, &provider, &mut mallory, "mallory", Some(&token)).await;

    // Mallory starts a sign-in as herself, then hands the callback link to Alex
    let start = json(
        app.as_browser(&mut mallory, "POST", "/api/oidc/start", json!({}))
            .await,
    )
    .await;
    let url = url::Url::parse(start["url"].as_str().unwrap()).unwrap();
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    provider.codes.lock().unwrap().insert(
        "code-trap".into(),
        ("mallory".into(), query["nonce"].clone()),
    );
    let mut alex = Browser::new();
    let callback = app
        .as_browser(
            &mut alex,
            "GET",
            &format!("/api/oidc/callback?state={}&code=code-trap", query["state"]),
            json!({}),
        )
        .await;
    assert_eq!(
        callback.headers()[header::LOCATION],
        "/sign-in#error.oidc_failed"
    );
    assert!(!alex.cookies.contains_key("session"));
}
