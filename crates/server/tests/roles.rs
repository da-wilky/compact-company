mod common;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use common::{Browser, TestApp, json, start};
use compact_company::config::Config;
use serde_json::json;

const PUBLIC: &[(&str, &str)] = &[
    ("GET", "/api/health"),
    ("GET", "/api/session"),
    ("POST", "/api/setup/start"),
    ("POST", "/api/setup/finish"),
    ("POST", "/api/sign-in/start"),
    ("POST", "/api/sign-in/finish"),
    ("POST", "/api/invitation"),
    ("POST", "/api/invitation/passkey/start"),
    ("POST", "/api/invitation/passkey/finish"),
    ("POST", "/api/oidc/start"),
    ("GET", "/api/oidc/callback"),
];

const ANY: &[&str] = &["owner", "staff", "accountant"];
const OWNER: &[&str] = &["owner"];
const OWNER_ACCOUNTANT: &[&str] = &["owner", "accountant"];

/// Every endpoint that needs a session, with the roles that may call it.
const SIGNED_IN: &[(&str, &str, &[&str])] = &[
    ("POST", "/api/sign-out", ANY),
    ("GET", "/api/passkeys", ANY),
    ("POST", "/api/passkeys/start", ANY),
    ("POST", "/api/passkeys/finish", ANY),
    ("DELETE", "/api/passkeys/{id}", ANY),
    ("GET", "/api/business", OWNER_ACCOUNTANT),
    ("PUT", "/api/business", OWNER),
    ("GET", "/api/users", OWNER),
    ("PATCH", "/api/users/{id}", OWNER),
    ("GET", "/api/invitations", OWNER),
    ("POST", "/api/invitations", OWNER),
    ("DELETE", "/api/invitations/{id}", OWNER),
    ("GET", "/api/audit", OWNER_ACCOUNTANT),
];

#[test]
fn every_endpoint_declares_its_roles() {
    let openapi = serde_json::to_value(compact_company::openapi()).unwrap();
    let documented: BTreeSet<(String, String)> = openapi["paths"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(path, item)| {
            item.as_object()
                .unwrap()
                .keys()
                .map(move |method| (method.to_uppercase(), path.clone()))
        })
        .collect();
    let declared: BTreeSet<(String, String)> = PUBLIC
        .iter()
        .map(|(m, p)| (m.to_string(), p.to_string()))
        .chain(
            SIGNED_IN
                .iter()
                .map(|(m, p, _)| (m.to_string(), p.to_string())),
        )
        .collect();
    assert_eq!(documented, declared);
}

async fn browsers(app: &TestApp) -> Vec<(&'static str, Browser)> {
    let mut owner = app.setup_owner().await;
    let staff = app.with_role(&mut owner, "staff").await;
    let accountant = app.with_role(&mut owner, "accountant").await;
    vec![
        ("owner", owner),
        ("staff", staff),
        ("accountant", accountant),
    ]
}

#[tokio::test]
async fn each_role_is_refused_where_it_is_not_allowed() {
    let app = start(Config::default()).await;
    let mut browsers = browsers(&app).await;
    for (method, path, allowed) in SIGNED_IN {
        let path = path.replace("{id}", "1");
        let response = app.send_json(method, &path, json!({})).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
        for (role, browser) in browsers.iter_mut().filter(|(r, _)| !allowed.contains(r)) {
            let response = app.as_browser(browser, method, &path, json!({})).await;
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "{role} may not {method} {path}"
            );
            assert_eq!(json(response).await["code"], "forbidden");
        }
    }
}
