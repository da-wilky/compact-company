mod common;

use common::{json, start};
use compact_company::{api::recurring::draft_due, config::Config};
use serde_json::{Value, json};

fn periods(recurring: &Value) -> Vec<String> {
    recurring["invoices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["recurring_date"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_monthly_invoice_started_in_the_past_drafts_each_missed_month_once() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let input = json!({
        "customer_id": customer, "title": "Wartung", "interval": "monthly",
        "period": "arrears", "next_date": "2026-01-31", "ends_on": "2026-04-30",
        "lines": [{ "description": "Wartungspauschale", "quantity": "1", "unit": "month",
                    "unit_price": 12000 }]
    });
    let recurring = app
        .ok(&mut owner, "POST", "/api/recurring-invoices", input.clone())
        .await;
    let id = recurring["id"].as_i64().unwrap();
    assert_eq!(
        periods(&recurring),
        ["2026-04-30", "2026-03-31", "2026-02-28", "2026-01-31"],
        "the 31st stays the 31st where a month has it"
    );
    assert_eq!(recurring["next_date"], Value::Null, "past its end");

    let draft = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/documents/{}", recurring["invoices"][2]["id"]),
            json!({}),
        )
        .await;
    assert_eq!(draft["status"], "draft");
    assert_eq!(draft["recurring_id"], id);
    assert_eq!(draft["title"], "Wartung");
    assert_eq!(draft["data"]["service_from"], "2026-01-31");
    assert_eq!(draft["data"]["service_to"], "2026-02-27");
    assert_eq!(draft["lines"][0]["unit_price"], 12000);
    assert!(draft["missing"].as_object().unwrap().is_empty());

    let overview = app.ok(&mut owner, "GET", "/api/overview", json!({})).await;
    assert_eq!(overview["recurring_drafts"].as_array().unwrap().len(), 4);

    let today = "2026-11-13".parse().unwrap();
    assert_eq!(
        draft_due(&app.app, today).await.unwrap(),
        0,
        "a restart drafts nothing twice"
    );

    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", recurring["invoices"][3]["id"]),
            json!({}),
        )
        .await;
    assert_eq!(issued["data"]["service_from"], "2025-12-31");
    assert_eq!(issued["data"]["service_to"], "2026-01-30");
    let overview = app.ok(&mut owner, "GET", "/api/overview", json!({})).await;
    assert_eq!(overview["recurring_drafts"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_recurring_invoice_drafts_on_its_date_and_edits_affect_only_later_drafts() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let mut input = json!({
        "customer_id": customer, "interval": "quarterly", "period": "advance",
        "next_date": "2030-01-01",
        "lines": [{ "description": "Hosting", "quantity": "3", "unit": "month",
                    "unit_price": 1500 }]
    });
    let recurring = app
        .ok(&mut owner, "POST", "/api/recurring-invoices", input.clone())
        .await;
    let id = recurring["id"].as_i64().unwrap();
    let path = format!("/api/recurring-invoices/{id}");
    assert!(periods(&recurring).is_empty());

    let day = |d: &str| d.parse().unwrap();
    assert_eq!(draft_due(&app.app, day("2029-12-31")).await.unwrap(), 0);
    assert_eq!(draft_due(&app.app, day("2030-01-01")).await.unwrap(), 1);
    assert_eq!(draft_due(&app.app, day("2030-01-01")).await.unwrap(), 0);

    input["lines"][0]["unit_price"] = json!(1800);
    input["next_date"] = json!("2030-04-01");
    let edited = app.ok(&mut owner, "PUT", &path, input.clone()).await;
    assert_eq!(edited["next_date"], "2030-04-01");
    let first = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/documents/{}", edited["invoices"][0]["id"]),
            json!({}),
        )
        .await;
    assert_eq!(
        first["lines"][0]["unit_price"], 1500,
        "drafted before the edit"
    );
    assert_eq!(first["data"]["service_to"], "2030-03-31");

    input["paused"] = json!(true);
    app.ok(&mut owner, "PUT", &path, input.clone()).await;
    assert_eq!(draft_due(&app.app, day("2030-07-01")).await.unwrap(), 0);
    input["paused"] = json!(false);
    app.ok(&mut owner, "PUT", &path, input.clone()).await;
    assert_eq!(
        draft_due(&app.app, day("2030-07-01")).await.unwrap(),
        2,
        "a paused invoice catches up when resumed"
    );
    let recurring = app.ok(&mut owner, "GET", &path, json!({})).await;
    assert_eq!(
        periods(&recurring),
        ["2030-07-01", "2030-04-01", "2030-01-01"]
    );
    let second = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/documents/{}", recurring["invoices"][1]["id"]),
            json!({}),
        )
        .await;
    assert_eq!(second["lines"][0]["unit_price"], 1800);

    let refused = app.as_browser(&mut owner, "DELETE", &path, json!({})).await;
    assert_eq!(json(refused).await["code"], "has_invoices");

    app.ok(
        &mut owner,
        "PUT",
        &format!("/api/customers/{customer}"),
        json!({ "name": "Bäckerei Krume", "language": "de", "archived": true,
                "street": "Brotgasse 2", "postcode": "04109", "city": "Leipzig" }),
    )
    .await;
    assert_eq!(draft_due(&app.app, day("2030-10-01")).await.unwrap(), 0);
    let recurring = app.ok(&mut owner, "GET", &path, json!({})).await;
    assert_eq!(recurring["paused"], true, "an archived customer pauses it");
    assert_eq!(recurring["next_date"], "2030-10-01");
}

#[tokio::test]
async fn recurring_lines_take_no_time_entries() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let response = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/recurring-invoices",
            json!({
                "customer_id": customer, "interval": "monthly", "period": "advance",
                "next_date": "2030-01-01",
                "lines": [{ "description": "x", "quantity": "1", "unit": "piece",
                            "unit_price": 1, "time_entry_ids": [1] }]
            }),
        )
        .await;
    assert_eq!(
        json(response).await["fields"]["lines.0.time_entry_ids"],
        "invalid_format"
    );
}
