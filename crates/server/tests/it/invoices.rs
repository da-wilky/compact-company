use crate::common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::config::Config;
use serde_json::{Value, json};

/// Unix seconds of a wall-clock time in Berlin.
fn at(day: &str, time: &str) -> i64 {
    format!("{day}T{time}[Europe/Berlin]")
        .parse::<jiff::Zoned>()
        .unwrap()
        .timestamp()
        .as_second()
}

#[tokio::test]
async fn an_invoice_bills_a_weeks_time_grouped_by_todo_and_rounded() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let project = app
        .ok(
            &mut owner,
            "POST",
            "/api/projects",
            json!({ "customer_id": customer, "name": "Website", "hourly_rate": 9000 }),
        )
        .await["id"]
        .as_i64()
        .unwrap();
    let todo =
        |title: &'static str| json!({ "project_id": project, "title": title, "status_id": 1 });
    let design = app
        .ok(&mut owner, "POST", "/api/todos", todo("Gestaltung"))
        .await["id"]
        .clone();
    let texts = app
        .ok(&mut owner, "POST", "/api/todos", todo("Texte"))
        .await["id"]
        .clone();
    let mut entry = async |todo: &Value, day: &str, from: &str, to: &str, billable: bool| -> i64 {
        app.ok(
            &mut owner,
            "POST",
            "/api/time",
            json!({ "project_id": project, "todo_id": todo, "started_at": at(day, from),
                    "ended_at": at(day, to), "billable": billable }),
        )
        .await["id"]
            .as_i64()
            .unwrap()
    };
    let first = entry(&design, "2026-03-02", "09:00", "09:50", true).await;
    entry(&design, "2026-03-04", "14:00", "14:20", true).await;
    entry(&texts, "2026-03-06", "10:00", "10:10", true).await;
    entry(&texts, "2026-03-05", "08:00", "09:00", false).await;
    entry(&design, "2026-02-20", "09:00", "10:00", true).await;

    let unbilled = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/unbilled?customer_id={customer}"),
            json!({}),
        )
        .await;
    assert_eq!(unbilled[0]["minutes"], 50 + 20 + 10 + 60);
    assert_eq!(unbilled[0]["amount"]["minor"], 21000);

    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "project_id": project,
                    "timesheet": true }),
        )
        .await;
    let id = draft["id"].as_i64().unwrap();
    let with_time = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/time"),
            json!({ "from": "2026-03-02", "to": "2026-03-08", "group": "todo", "rounding": 15 }),
        )
        .await;
    let lines = with_time["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["description"], "Gestaltung");
    assert_eq!(
        lines[0]["quantity"], "1.5",
        "50 and 20 minutes round up to 60 and 30"
    );
    assert_eq!(lines[1]["quantity"], "0.25");
    assert_eq!(
        lines[0]["unit_price"],
        Value::Null,
        "priced at the rate when issued"
    );
    assert_eq!(with_time["data"]["totals"]["gross"], "157.50");
    assert_eq!(with_time["data"]["service_from"], "2026-03-02");
    assert_eq!(with_time["data"]["service_to"], "2026-03-06");

    let again = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/time"),
            json!({ "from": "2026-03-02", "to": "2026-03-08", "group": "todo", "rounding": 15 }),
        )
        .await;
    assert_eq!(
        json(again).await["code"],
        "no_unbilled_time",
        "the draft holds that time"
    );

    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/issue"),
            json!({}),
        )
        .await;
    assert_eq!(issued["lines"][0]["unit_price"], 9000);
    let timesheet = issued["data"]["timesheet"].as_array().unwrap();
    assert_eq!(timesheet.len(), 3);
    assert_eq!(timesheet[0]["date"], "2026-03-02");
    assert_eq!(timesheet[0]["hours"], "1");
    assert_eq!(timesheet[0]["description"], "Gestaltung");
    let issue_date: jiff::civil::Date = issued["issue_date"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        issued["data"]["due_date"],
        issue_date
            .saturating_add(jiff::Span::new().days(14))
            .to_string()
            .as_str()
    );

    let unbilled = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/unbilled?customer_id={customer}"),
            json!({}),
        )
        .await;
    assert_eq!(
        unbilled[0]["minutes"], 60,
        "only the time before that week is left"
    );

    let edit = app
        .as_browser(
            &mut owner,
            "PUT",
            &format!("/api/time/{first}"),
            json!({ "project_id": project, "started_at": at("2026-03-02", "09:00"),
                    "ended_at": at("2026-03-02", "11:00") }),
        )
        .await;
    assert_eq!(edit.status(), StatusCode::CONFLICT);
    assert_eq!(json(edit).await["code"], "billed");
}

#[tokio::test]
async fn deleting_a_draft_frees_its_time_and_an_invoice_without_time_uses_the_issue_date() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let project = app
        .ok(
            &mut owner,
            "POST",
            "/api/projects",
            json!({ "customer_id": customer, "name": "Wartung" }),
        )
        .await["id"]
        .as_i64()
        .unwrap();
    app.ok(
        &mut owner,
        "POST",
        "/api/time",
        json!({ "project_id": project, "started_at": at("2026-03-02", "09:00"),
                "ended_at": at("2026-03-02", "09:07") }),
    )
    .await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer }),
        )
        .await;
    let id = draft["id"].as_i64().unwrap();
    let with_time = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/time"),
            json!({ "group": "entry", "rounding": 0 }),
        )
        .await;
    assert_eq!(with_time["lines"][0]["description"], "2026-03-02: Wartung");
    assert_eq!(with_time["lines"][0]["quantity"], "0.12");
    assert_eq!(
        with_time["data"]["lines"][0]["unit_price"], "80.00",
        "the customer's rate applies to a project without one"
    );
    let deleted = app
        .as_browser(
            &mut owner,
            "DELETE",
            &format!("/api/documents/{id}"),
            json!({}),
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let unbilled = app.ok(&mut owner, "GET", "/api/unbilled", json!({})).await;
    assert_eq!(unbilled[0]["entries"], 1);

    let plain = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Lizenz", "quantity": "1", "unit": "piece", "unit_price": 5000 }
            ] }),
        )
        .await;
    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", plain["id"]),
            json!({}),
        )
        .await;
    assert_eq!(issued["data"]["service_from"], issued["issue_date"]);
    assert_eq!(issued["data"]["service_to"], issued["issue_date"]);
}
