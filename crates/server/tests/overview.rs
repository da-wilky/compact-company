mod common;

use common::start;
use compact_company::config::Config;
use serde_json::json;

#[tokio::test]
async fn the_overview_shows_timers_unbilled_time_and_open_invoices_by_role() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut staff = app.with_role(&mut owner, "staff").await;
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
        "/api/timers",
        json!({ "project_id": project, "description": "Owner arbeitet" }),
    )
    .await;
    app.ok(
        &mut staff,
        "POST",
        "/api/timers",
        json!({ "project_id": project, "description": "Staff arbeitet" }),
    )
    .await;
    app.ok(
        &mut owner,
        "POST",
        "/api/time",
        json!({ "project_id": project, "started_at": 1_772_438_400, "ended_at": 1_772_444_700 }),
    )
    .await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Lizenz", "quantity": "1", "unit": "piece", "unit_price": 10000 }
            ] }),
        )
        .await;
    let id = draft["id"].as_i64().unwrap();
    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/issue"),
            json!({}),
        )
        .await;
    assert_eq!(issued["amount_due"]["minor"], 10000);
    let paid = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/payments"),
            json!({ "date": "2026-03-10", "amount": 4000 }),
        )
        .await;
    assert_eq!(paid["amount_due"]["minor"], 6000);
    assert_eq!(paid["payments"][0]["user_name"], "Samuel");

    let overview = app.ok(&mut owner, "GET", "/api/overview", json!({})).await;
    assert_eq!(overview["running"].as_array().unwrap().len(), 2);
    assert_eq!(overview["unbilled"][0]["minutes"], 105);
    assert_eq!(overview["unbilled"][0]["project_name"], "Website");
    assert_eq!(overview["open_invoices"][0]["number"], issued["number"]);
    assert_eq!(overview["open_invoices"][0]["due"]["minor"], 6000);

    let mine = app.ok(&mut staff, "GET", "/api/overview", json!({})).await;
    assert_eq!(mine["running"].as_array().unwrap().len(), 1);
    assert_eq!(mine["running"][0]["description"], "Staff arbeitet");
    assert_eq!(mine["unbilled"], json!([]));
    assert_eq!(mine["open_invoices"], json!([]));

    app.ok(
        &mut owner,
        "POST",
        &format!("/api/documents/{id}/payments"),
        json!({ "date": "2026-03-20", "amount": 6000 }),
    )
    .await;
    let overview = app.ok(&mut owner, "GET", "/api/overview", json!({})).await;
    assert_eq!(
        overview["open_invoices"],
        json!([]),
        "a paid invoice is no longer open"
    );
}
