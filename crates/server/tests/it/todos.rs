use crate::common;

use axum::http::StatusCode;
use common::{Browser, TestApp, json, start};
use compact_company::config::Config;
use serde_json::{Value, json};

async fn ok(app: &TestApp, who: &mut Browser, method: &str, path: &str, body: Value) -> Value {
    let response = app.as_browser(who, method, path, body).await;
    assert_eq!(response.status(), StatusCode::OK, "{method} {path}");
    json(response).await
}

async fn project(app: &TestApp, owner: &mut Browser) -> i64 {
    let customer = ok(
        app,
        owner,
        "POST",
        "/api/customers",
        json!({ "name": "Krume", "language": "de" }),
    )
    .await;
    let project = ok(
        app,
        owner,
        "POST",
        "/api/projects",
        json!({ "customer_id": customer["id"], "name": "Relaunch" }),
    )
    .await;
    project["id"].as_i64().unwrap()
}

#[tokio::test]
async fn staff_move_a_todo_across_the_board_and_comment_and_the_owner_sees_both() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut staff = app.with_role(&mut owner, "staff").await;
    let project = project(&app, &mut owner).await;

    let assignees = ok(&app, &mut staff, "GET", "/api/assignees", json!({})).await;
    let me = assignees
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] != "Samuel")
        .unwrap()["id"]
        .clone();
    let todo = ok(&app, &mut staff, "POST", "/api/todos", json!({ "project_id": project, "title": "Startseite", "assignee_id": me, "due_date": "2026-11-12", "priority": "high" })).await;
    assert_eq!(todo["status_id"], 1);
    assert_eq!(todo["customer_name"], "Krume");
    let id = todo["id"].as_i64().unwrap();

    let moved = ok(
        &app,
        &mut staff,
        "POST",
        &format!("/api/todos/{id}/move"),
        json!({ "status_id": 2 }),
    )
    .await;
    assert_eq!(moved["status_id"], 2);
    ok(
        &app,
        &mut staff,
        "POST",
        &format!("/api/todos/{id}/comments"),
        json!({ "body": "Entwurf steht" }),
    )
    .await;

    let seen = ok(
        &app,
        &mut owner,
        "GET",
        &format!("/api/todos/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(seen["todo"]["status_id"], 2);
    assert_eq!(seen["comments"][0]["body"], "Entwurf steht");
    assert_eq!(seen["todo"]["comments"], 1);
    let log = ok(
        &app,
        &mut owner,
        "GET",
        &format!("/api/audit?subject=todo:{id}"),
        json!({}),
    )
    .await;
    let actions: Vec<_> = log
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["todo.comment", "todo.update", "todo.create"]);

    let mine = ok(
        &app,
        &mut staff,
        "GET",
        &format!("/api/todos?assignee_id={me}&due_before=2026-11-13"),
        json!({}),
    )
    .await;
    assert_eq!(mine.as_array().unwrap().len(), 1);
    let done = ok(&app, &mut staff, "GET", "/api/todos?done=true", json!({})).await;
    assert!(done.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn moving_on_the_board_keeps_the_order_people_drop_into() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let project = project(&app, &mut owner).await;
    let mut ids = Vec::new();
    for title in ["a", "b", "c"] {
        ids.push(
            ok(
                &app,
                &mut owner,
                "POST",
                "/api/todos",
                json!({ "project_id": project, "title": title }),
            )
            .await["id"]
                .clone(),
        );
    }
    ok(
        &app,
        &mut owner,
        "POST",
        &format!("/api/todos/{}/move", ids[2]),
        json!({ "status_id": 1, "before_id": ids[0] }),
    )
    .await;
    let order: Vec<_> = ok(&app, &mut owner, "GET", "/api/todos?status_id=1", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(order, ["c", "a", "b"]);
}

#[tokio::test]
async fn the_status_list_changes_but_never_loses_a_status_in_use() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let project = project(&app, &mut owner).await;
    ok(
        &app,
        &mut owner,
        "POST",
        "/api/todos",
        json!({ "project_id": project, "title": "a", "status_id": 3 }),
    )
    .await;

    let statuses = ok(&app, &mut owner, "PUT", "/api/statuses", json!([
        { "id": 1, "name": "Neu" }, { "name": "Review" }, { "id": 3, "name": "Wartet" }, { "id": 4, "name": "Fertig", "done": true }
    ])).await;
    let names: Vec<_> = statuses
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Neu", "Review", "Wartet", "Fertig"]);

    let refused = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/statuses",
            json!([{ "id": 1, "name": "Neu" }, { "id": 4, "name": "Fertig", "done": true }]),
        )
        .await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    assert_eq!(json(refused).await["code"], "status_in_use");
    let blank = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/statuses",
            json!([{ "id": 1, "name": " " }]),
        )
        .await;
    assert_eq!(json(blank).await["fields"]["0.name"], "required");
    let twice = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/statuses",
            json!([{ "id": 1, "name": "A" }, { "id": 1, "name": "B" }]),
        )
        .await;
    assert_eq!(twice.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn todos_need_an_open_project_a_real_day_and_an_assignable_user() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let accountant = app.with_role(&mut owner, "accountant").await;
    drop(accountant);
    let project = project(&app, &mut owner).await;
    let bad = app.as_browser(&mut owner, "POST", "/api/todos", json!({ "project_id": project, "title": "a", "due_date": "2026-02-30", "assignee_id": 2 })).await;
    assert_eq!(
        json(bad).await["fields"],
        json!({ "due_date": "invalid_format", "assignee_id": "invalid_format" })
    );

    let path = format!("/api/projects/{project}");
    let project_now = ok(&app, &mut owner, "GET", &path, json!({})).await;
    ok(
        &app,
        &mut owner,
        "PUT",
        &path,
        json!({ "customer_id": project_now["customer_id"], "name": "Relaunch", "archived": true }),
    )
    .await;
    let closed = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/todos",
            json!({ "project_id": project, "title": "a" }),
        )
        .await;
    assert_eq!(json(closed).await["code"], "archived");
}
