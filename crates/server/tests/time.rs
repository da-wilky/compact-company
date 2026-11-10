mod common;

use axum::http::StatusCode;
use common::{Browser, TestApp, json, start};
use compact_company::{auth::now, config::Config};
use serde_json::{Value, json};

async fn ok(app: &TestApp, who: &mut Browser, method: &str, path: &str, body: Value) -> Value {
    let response = app.as_browser(who, method, path, body).await;
    let status = response.status();
    let body = json(response).await;
    assert_eq!(status, StatusCode::OK, "{method} {path}: {body}");
    body
}

async fn error(app: &TestApp, who: &mut Browser, method: &str, path: &str, body: Value) -> Value {
    let response = app.as_browser(who, method, path, body).await;
    assert_ne!(response.status(), StatusCode::OK, "{method} {path}");
    json(response).await
}

#[tokio::test]
async fn staff_track_time_on_a_todo_of_a_new_customers_project_with_two_timers() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut staff = app.with_role(&mut owner, "staff").await;
    let customer = ok(&app, &mut owner, "POST", "/api/customers", json!({ "name": "Krume", "language": "de", "hourly_rate": 8500 })).await;
    let project = ok(&app, &mut owner, "POST", "/api/projects", json!({ "customer_id": customer["id"], "name": "Relaunch" })).await;
    let todo = ok(&app, &mut staff, "POST", "/api/todos", json!({ "project_id": project["id"], "title": "Startseite" })).await;

    let first = ok(&app, &mut staff, "POST", "/api/timers", json!({ "project_id": project["id"], "todo_id": todo["id"] })).await;
    let second = ok(&app, &mut staff, "POST", "/api/timers", json!({ "project_id": project["id"], "description": "Abstimmung" })).await;
    assert_eq!(first["todo_title"], "Startseite");
    assert_eq!(first["billable"], true);
    assert_eq!(first["rate"], json!({ "minor": 8500, "currency": "EUR" }));
    assert_eq!(ok(&app, &mut staff, "GET", "/api/timers", json!({})).await.as_array().unwrap().len(), 2);

    let stopped = ok(&app, &mut staff, "POST", &format!("/api/time/{}/stop", second["id"]), json!({})).await;
    assert!(stopped["ended_at"].is_i64());
    let running = ok(&app, &mut staff, "GET", "/api/timers", json!({})).await;
    assert_eq!(running.as_array().unwrap().len(), 1);
    assert_eq!(running[0]["id"], first["id"]);

    let week = ok(&app, &mut staff, "GET", &format!("/api/time?from={}&to={}", now() - 3600, now() + 3600), json!({})).await;
    assert_eq!(week.as_array().unwrap().len(), 2);

    let owners_view = ok(&app, &mut owner, "GET", &format!("/api/time?from=0&to={}&user_id={}", now() + 60, first["user_id"]), json!({})).await;
    assert_eq!(owners_view.as_array().unwrap().len(), 2);
    let refused = app.as_browser(&mut staff, "GET", &format!("/api/time?from=0&to={}&user_id=1", now()), json!({})).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let deleted = error(&app, &mut staff, "DELETE", &format!("/api/todos/{}", todo["id"]), json!({})).await;
    assert_eq!(deleted["code"], "has_time");
}

#[tokio::test]
async fn manual_entries_are_checked_and_stay_private_to_their_user() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut staff = app.with_role(&mut owner, "staff").await;
    let customer = ok(&app, &mut owner, "POST", "/api/customers", json!({ "name": "Krume", "language": "de" })).await;
    let upkeep = ok(&app, &mut owner, "POST", "/api/projects", json!({ "customer_id": customer["id"], "name": "Wartung", "billable": false })).await;
    let other = ok(&app, &mut owner, "POST", "/api/projects", json!({ "customer_id": customer["id"], "name": "Relaunch" })).await;
    let todo = ok(&app, &mut owner, "POST", "/api/todos", json!({ "project_id": other["id"], "title": "Startseite" })).await;
    let start = now() - 7200;

    let entry = ok(&app, &mut owner, "POST", "/api/time", json!({ "project_id": upkeep["id"], "started_at": start, "ended_at": start + 3600, "billable_secs": 1800 })).await;
    assert_eq!(entry["billable"], false, "the project's setting applies");
    assert_eq!(entry["rate"], Value::Null);

    let bad = error(&app, &mut owner, "POST", "/api/time", json!({
        "project_id": upkeep["id"], "todo_id": todo["id"], "started_at": start, "ended_at": start - 1, "billable_secs": 10
    })).await;
    assert_eq!(bad["fields"], json!({ "todo_id": "invalid_format", "ended_at": "invalid_format", "billable_secs": "invalid_format" }));
    let long = error(&app, &mut owner, "POST", "/api/time", json!({ "project_id": upkeep["id"], "started_at": start - 90000, "ended_at": start })).await;
    assert_eq!(long["fields"]["ended_at"], "invalid_format");
    let open = error(&app, &mut owner, "POST", "/api/time", json!({ "project_id": upkeep["id"], "started_at": start })).await;
    assert_eq!(open["fields"]["ended_at"], "required");

    let path = format!("/api/time/{}", entry["id"]);
    let hidden = app.as_browser(&mut staff, "PUT", &path, json!({ "project_id": upkeep["id"], "started_at": start, "ended_at": start + 60 })).await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    let posing = app.as_browser(&mut staff, "POST", "/api/time", json!({ "project_id": upkeep["id"], "started_at": start, "ended_at": start + 60, "user_id": 1 })).await;
    assert_eq!(posing.status(), StatusCode::FORBIDDEN);

    let edited = ok(&app, &mut owner, "PUT", &path, json!({ "project_id": other["id"], "todo_id": todo["id"], "started_at": start, "ended_at": start + 5400 })).await;
    assert_eq!(edited["todo_title"], "Startseite");
    assert_eq!(edited["billable"], false, "an edit keeps the entry's own flag");
    let moved = error(&app, &mut owner, "PUT", &format!("/api/todos/{}", todo["id"]), json!({ "project_id": upkeep["id"], "title": "Startseite" })).await;
    assert_eq!(moved["fields"]["project_id"], "locked");

    let deleted = app.as_browser(&mut owner, "DELETE", &path, json!({})).await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let log = ok(&app, &mut owner, "GET", &format!("/api/audit?subject=time:{}", entry["id"]), json!({})).await;
    let actions: Vec<_> = log.as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["time.delete", "time.update", "time.create"]);
}
