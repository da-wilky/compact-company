mod common;

use axum::http::StatusCode;
use common::{json as body, start};
use compact_company::config::Config;
use serde_json::{Value, json};

fn account(accounts: &Value, number: &str) -> i64 {
    accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == number)
        .unwrap_or_else(|| panic!("account {number}"))["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn the_owner_starts_skr04_books_and_books_and_reverses_by_hand() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut accountant = app.with_role(&mut owner, "accountant").await;

    let before = app.ok(&mut owner, "GET", "/api/books", json!({})).await;
    assert_eq!(before["books"], Value::Null);
    assert_eq!(before["taxation"], "accrual");
    let response = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/postings",
            json!({ "date": "2026-01-01", "description": "Eröffnung", "lines": [] }),
        )
        .await;
    assert_eq!(body(response).await["code"], "books_not_started");

    let started = app
        .ok(
            &mut owner,
            "POST",
            "/api/books",
            json!({ "chart": "skr04", "start_date": "2026-01-01" }),
        )
        .await;
    assert_eq!(started["books"]["chart"], "skr04");
    let again = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/books",
            json!({ "chart": "skr03", "start_date": "2026-01-01" }),
        )
        .await;
    assert_eq!(body(again).await["code"], "books_started");

    let accounts = app.ok(&mut owner, "GET", "/api/accounts", json!({})).await;
    let bank = account(&accounts, "1800");
    let capital = account(&accounts, "2900");
    let receivables = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["role"] == "receivables")
        .unwrap();
    assert_eq!(receivables["number"], "1200");
    assert_eq!(receivables["type"], "asset");

    let unbalanced = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/postings",
            json!({ "date": "2026-01-01", "description": "Eröffnung", "lines": [
                { "account_id": bank, "amount": 250_000 },
                { "account_id": capital, "amount": -240_000 }
            ] }),
        )
        .await;
    assert_eq!(body(unbalanced).await["fields"]["lines"], "invalid_format");
    let too_early = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/postings",
            json!({ "date": "2025-12-31", "description": "Eröffnung", "lines": [
                { "account_id": bank, "amount": 250_000 },
                { "account_id": capital, "amount": -250_000 }
            ] }),
        )
        .await;
    assert_eq!(body(too_early).await["fields"]["date"], "invalid_format");

    let opening = app
        .ok(
            &mut owner,
            "POST",
            "/api/postings",
            json!({ "date": "2026-01-01", "description": "Eröffnung", "lines": [
                { "account_id": bank, "amount": 250_000 },
                { "account_id": capital, "amount": -250_000 }
            ] }),
        )
        .await;
    assert_eq!(opening["lines"][0]["number"], "1800");
    assert_eq!(opening["user_name"], "Samuel");

    let reversal = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/postings/{}/reverse", opening["id"]),
            json!({ "date": "2026-01-02" }),
        )
        .await;
    assert_eq!(reversal["reverses_id"], opening["id"]);
    assert_eq!(reversal["lines"][0]["amount"], 250_000);
    assert_eq!(reversal["lines"][0]["number"], "2900");
    let twice = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/postings/{}/reverse", opening["id"]),
            json!({}),
        )
        .await;
    assert_eq!(body(twice).await["code"], "already_reversed");

    let journal = app
        .ok(&mut accountant, "GET", "/api/postings", json!({}))
        .await;
    assert_eq!(journal.as_array().unwrap().len(), 2);
    assert_eq!(journal[0]["reversed_by"], reversal["id"]);
    let accounts = app
        .ok(&mut accountant, "GET", "/api/accounts", json!({}))
        .await;
    assert!(
        accounts
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["balance"] == 0)
    );
    let forbidden = app
        .as_browser(
            &mut accountant,
            "POST",
            "/api/postings",
            json!({ "date": "2026-01-03", "description": "x", "lines": [] }),
        )
        .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let added = app
        .ok(
            &mut owner,
            "POST",
            "/api/accounts",
            json!({ "number": "6837", "name": "Aufwendungen für Software", "type": "expense" }),
        )
        .await;
    assert_eq!(added["role"], Value::Null);
    let taken = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/accounts",
            json!({ "number": "6837", "name": "Doppelt", "type": "expense" }),
        )
        .await;
    assert_eq!(body(taken).await["fields"]["number"], "taken");
    let locked = app
        .as_browser(
            &mut owner,
            "PUT",
            &format!("/api/accounts/{bank}"),
            json!({ "name": "Bank", "archived": true }),
        )
        .await;
    assert_eq!(body(locked).await["fields"]["archived"], "locked");

    let audit = app.ok(&mut owner, "GET", "/api/audit", json!({})).await;
    let actions: Vec<&str> = audit
        .as_array()
        .or_else(|| audit["entries"].as_array())
        .unwrap()
        .iter()
        .filter_map(|e| e["action"].as_str())
        .collect();
    assert!(actions.contains(&"books.start"));
    assert!(actions.contains(&"posting.reverse"));
}
