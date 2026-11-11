mod common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::config::Config;
use serde_json::{Value, json};

#[tokio::test]
async fn a_partial_cancellation_reverses_the_chosen_lines_as_issued() {
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
    app.ok(
        &mut owner,
        "POST",
        "/api/time",
        json!({ "project_id": project, "started_at": 1_772_438_400, "ended_at": 1_772_445_600 }),
    )
    .await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "project_id": project, "lines": [
                { "description": "Lizenz", "quantity": "1", "unit": "piece", "unit_price": 50000 },
                { "description": "Hosting", "quantity": "3", "unit": "month", "unit_price": 1250 }
            ] }),
        )
        .await;
    let id = draft["id"].as_i64().unwrap();
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/documents/{id}/time"),
        json!({ "group": "project", "rounding": 0 }),
    )
    .await;
    let invoice = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/issue"),
            json!({}),
        )
        .await;
    assert_eq!(invoice["data"]["totals"]["gross"], "717.50");
    app.ok(
        &mut owner,
        "PUT",
        &format!("/api/projects/{project}"),
        json!({ "customer_id": customer, "name": "Website", "hourly_rate": 20000 }),
    )
    .await;

    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "cancellation", "customer_id": customer }),
        )
        .await;
    assert_eq!(json(refused).await["code"], "follow_up_not_allowed");

    let cancellation = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/follow-up"),
            json!({ "kind": "cancellation" }),
        )
        .await;
    let lines = cancellation["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 3);
    let keep: Vec<Value> = lines[1..]
        .iter()
        .map(|line| {
            json!({ "description": "anything", "quantity": "-99", "unit": "piece",
                    "unit_price": 1, "cancels_line_id": line["cancels_line_id"] })
        })
        .collect();
    let path = format!("/api/documents/{}", cancellation["id"]);
    let partial = app
        .ok(
            &mut owner,
            "PUT",
            &path,
            json!({ "kind": "cancellation", "customer_id": customer, "lines": keep }),
        )
        .await;
    assert_eq!(partial["lines"][0]["description"], "Hosting");
    assert_eq!(partial["lines"][0]["quantity"], "-3");
    assert_eq!(
        partial["lines"][1]["unit_price"], 9000,
        "the rate as invoiced, not today's"
    );
    let issued = app
        .ok(&mut owner, "POST", &format!("{path}/issue"), json!({}))
        .await;
    assert_eq!(issued["data"]["totals"]["gross"], "-217.50");
    assert_eq!(
        issued["data"]["reference"],
        format!("Storniert Rechnung {}", invoice["number"].as_str().unwrap()).as_str()
    );
    assert!(issued["number"].as_str().unwrap().starts_with("RE-"));

    let unbilled = app.ok(&mut owner, "GET", "/api/unbilled", json!({})).await;
    assert_eq!(
        unbilled[0]["minutes"], 120,
        "the cancelled time can be billed again"
    );

    let rest = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/follow-up"),
            json!({ "kind": "cancellation" }),
        )
        .await;
    assert_eq!(rest["lines"].as_array().unwrap().len(), 1);
    let again = app
        .as_browser(
            &mut owner,
            "PUT",
            &format!("/api/documents/{}", rest["id"]),
            json!({ "kind": "cancellation", "customer_id": customer, "lines": [
                { "description": "x", "quantity": "1", "unit": "piece",
                  "cancels_line_id": lines[1]["cancels_line_id"] }
            ] }),
        )
        .await;
    assert_eq!(again.status(), StatusCode::CONFLICT);
    assert_eq!(json(again).await["code"], "already_cancelled");
}

#[tokio::test]
async fn delivery_notes_and_acceptance_reports_carry_no_prices() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let project = app
        .ok(
            &mut owner,
            "POST",
            "/api/projects",
            json!({ "customer_id": customer, "name": "Kassensystem" }),
        )
        .await["id"]
        .clone();
    let note = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "delivery_note", "customer_id": customer, "project_id": project,
                    "lines": [{ "description": "Kassenterminal", "quantity": "2", "unit": "piece" }] }),
        )
        .await;
    assert_eq!(
        note["missing"],
        json!({}),
        "a delivery note needs no prices"
    );
    let note = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", note["id"]),
            json!({}),
        )
        .await;
    assert!(note["number"].as_str().unwrap().starts_with("LS-"));
    assert_eq!(note["data"]["totals"], Value::Null);

    let report = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "acceptance_report", "customer_id": customer, "project_id": project,
                    "lines": [{ "description": "Einrichtung der Kassen", "quantity": "1",
                                "unit": "lump_sum" }] }),
        )
        .await;
    let report = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", report["id"]),
            json!({}),
        )
        .await;
    assert!(report["number"].as_str().unwrap().starts_with("AP-"));
    assert_eq!(report["project_id"], project);
}
