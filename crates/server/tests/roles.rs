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
    ("GET", "/api/calendar/{file}"),
];

const ANY: &[&str] = &["owner", "staff", "accountant"];
const OWNER: &[&str] = &["owner"];
const OWNER_ACCOUNTANT: &[&str] = &["owner", "accountant"];
const OWNER_STAFF: &[&str] = &["owner", "staff"];

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
    ("GET", "/api/books", OWNER_ACCOUNTANT),
    ("POST", "/api/books", OWNER),
    ("GET", "/api/accounts", OWNER_ACCOUNTANT),
    ("POST", "/api/accounts", OWNER),
    ("PUT", "/api/accounts/{id}", OWNER),
    ("GET", "/api/postings", OWNER_ACCOUNTANT),
    ("POST", "/api/postings", OWNER),
    ("POST", "/api/postings/{id}/reverse", OWNER),
    ("GET", "/api/assets", OWNER_ACCOUNTANT),
    ("POST", "/api/assets", OWNER),
    ("PUT", "/api/assets/{id}", OWNER),
    ("DELETE", "/api/assets/{id}", OWNER),
    ("POST", "/api/assets/depreciation", OWNER),
    ("POST", "/api/assets/{id}/dispose", OWNER),
    ("GET", "/api/assets/register", OWNER_ACCOUNTANT),
    ("GET", "/api/suppliers", OWNER_ACCOUNTANT),
    ("POST", "/api/suppliers", OWNER),
    ("PUT", "/api/suppliers/{id}", OWNER),
    ("GET", "/api/receipts", OWNER_ACCOUNTANT),
    ("POST", "/api/receipts", OWNER),
    ("GET", "/api/receipts/{id}", OWNER_ACCOUNTANT),
    ("PUT", "/api/receipts/{id}", OWNER),
    ("DELETE", "/api/receipts/{id}", OWNER),
    ("POST", "/api/receipts/{id}/book", OWNER),
    ("POST", "/api/receipts/{id}/reverse", OWNER),
    ("GET", "/api/receipts/{id}/file", OWNER_ACCOUNTANT),
    ("POST", "/api/receipts/{id}/payments", OWNER),
    ("POST", "/api/receipt-payments/{id}/reverse", OWNER),
    ("GET", "/api/bank-accounts", OWNER_ACCOUNTANT),
    ("POST", "/api/bank-accounts", OWNER),
    ("PUT", "/api/bank-accounts/{id}", OWNER),
    ("POST", "/api/bank-accounts/{id}/import", OWNER),
    ("GET", "/api/bank-transactions", OWNER_ACCOUNTANT),
    ("GET", "/api/bank-transactions/{id}", OWNER_ACCOUNTANT),
    ("POST", "/api/bank-transactions/{id}/match", OWNER),
    ("POST", "/api/bank-transactions/{id}/unmatch", OWNER),
    ("GET", "/api/open-items", OWNER_ACCOUNTANT),
    ("GET", "/api/tax/vat-return", OWNER_ACCOUNTANT),
    ("GET", "/api/tax/euer", OWNER_ACCOUNTANT),
    ("GET", "/api/exports/datev", OWNER_ACCOUNTANT),
    ("GET", "/api/deadlines", OWNER_ACCOUNTANT),
    ("GET", "/api/calendar-feed", OWNER),
    ("POST", "/api/calendar-feed", OWNER),
    ("DELETE", "/api/calendar-feed", OWNER),
    ("GET", "/api/years", OWNER_ACCOUNTANT),
    ("POST", "/api/years/{year}/close", OWNER),
    ("GET", "/api/years/{year}/archive", OWNER_ACCOUNTANT),
    ("GET", "/api/tokens", ANY),
    ("POST", "/api/tokens", ANY),
    ("DELETE", "/api/tokens/{id}", ANY),
    ("GET", "/api/currencies", ANY),
    ("GET", "/api/customers", ANY),
    ("POST", "/api/customers", OWNER),
    ("GET", "/api/customers/{id}", ANY),
    ("PUT", "/api/customers/{id}", OWNER),
    ("GET", "/api/projects", ANY),
    ("POST", "/api/projects", OWNER),
    ("GET", "/api/projects/{id}", ANY),
    ("PUT", "/api/projects/{id}", OWNER),
    ("GET", "/api/activities", ANY),
    ("POST", "/api/activities", OWNER),
    ("PUT", "/api/activities/{id}", OWNER),
    ("GET", "/api/statuses", OWNER_STAFF),
    ("PUT", "/api/statuses", OWNER),
    ("GET", "/api/assignees", OWNER_STAFF),
    ("GET", "/api/todos", OWNER_STAFF),
    ("POST", "/api/todos", OWNER_STAFF),
    ("GET", "/api/todos/{id}", OWNER_STAFF),
    ("PUT", "/api/todos/{id}", OWNER_STAFF),
    ("DELETE", "/api/todos/{id}", OWNER_STAFF),
    ("POST", "/api/todos/{id}/move", OWNER_STAFF),
    ("POST", "/api/todos/{id}/comments", OWNER_STAFF),
    ("GET", "/api/time", OWNER_STAFF),
    ("POST", "/api/time", OWNER_STAFF),
    ("PUT", "/api/time/{id}", OWNER_STAFF),
    ("DELETE", "/api/time/{id}", OWNER_STAFF),
    ("POST", "/api/time/{id}/stop", OWNER_STAFF),
    ("GET", "/api/timers", OWNER_STAFF),
    ("GET", "/api/documents", OWNER_ACCOUNTANT),
    ("POST", "/api/documents", OWNER),
    ("GET", "/api/documents/{id}", OWNER_ACCOUNTANT),
    ("PUT", "/api/documents/{id}", OWNER),
    ("DELETE", "/api/documents/{id}", OWNER),
    ("POST", "/api/documents/{id}/issue", OWNER),
    ("GET", "/api/documents/{id}/pdf", OWNER_ACCOUNTANT),
    ("GET", "/api/documents/{id}/xml", OWNER_ACCOUNTANT),
    ("POST", "/api/documents/{id}/send", OWNER),
    ("POST", "/api/documents/{id}/payments", OWNER),
    ("POST", "/api/payments/{id}/reverse", OWNER),
    ("GET", "/api/overview", ANY),
    ("POST", "/api/documents/{id}/duplicate", OWNER),
    ("POST", "/api/documents/{id}/follow-up", OWNER),
    ("POST", "/api/documents/{id}/outcome", OWNER),
    ("POST", "/api/documents/{id}/time", OWNER),
    ("GET", "/api/unbilled", OWNER_ACCOUNTANT),
    ("GET", "/api/snippets", OWNER),
    ("POST", "/api/snippets", OWNER),
    ("PUT", "/api/snippets/{id}", OWNER),
    ("DELETE", "/api/snippets/{id}", OWNER),
    ("GET", "/api/layouts", OWNER),
    ("POST", "/api/layouts", OWNER),
    ("PUT", "/api/layouts/{id}", OWNER),
    ("DELETE", "/api/layouts/{id}", OWNER),
    ("GET", "/api/layouts/{id}/sample", OWNER),
    ("GET", "/api/layout-choices", OWNER),
    ("PUT", "/api/layout-choices", OWNER),
    ("GET", "/api/packages", OWNER),
    ("POST", "/api/packages", OWNER),
    ("POST", "/api/timers", OWNER_STAFF),
    ("GET", "/api/recurring-invoices", OWNER_ACCOUNTANT),
    ("POST", "/api/recurring-invoices", OWNER),
    ("GET", "/api/recurring-invoices/{id}", OWNER_ACCOUNTANT),
    ("PUT", "/api/recurring-invoices/{id}", OWNER),
    ("DELETE", "/api/recurring-invoices/{id}", OWNER),
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
