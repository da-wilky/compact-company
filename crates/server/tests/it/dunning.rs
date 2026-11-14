use crate::common;

use common::{json as body, start};
use compact_company::config::Config;
use serde_json::{Value, json};

fn in_days(days: i64) -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::get("Europe/Berlin").unwrap())
        .date()
        .saturating_add(jiff::Span::new().days(days))
        .to_string()
}

#[tokio::test]
async fn an_open_invoice_gets_dunning_letters_until_it_is_paid() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Wartung", "quantity": "1", "unit": "piece", "unit_price": 10000 }
            ] }),
        )
        .await;
    let id = draft["id"].as_i64().unwrap();
    let invoice = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/issue"),
            json!({}),
        )
        .await;

    let open = app
        .ok(&mut owner, "GET", "/api/open-items", json!({}))
        .await;
    assert_eq!(open["invoices"][0]["id"], id);
    assert_eq!(open["invoices"][0]["due"]["minor"], 10_000);
    assert_eq!(open["invoices"][0]["dunning_level"], Value::Null);

    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "dunning", "customer_id": customer }),
        )
        .await;
    assert_eq!(body(refused).await["code"], "follow_up_not_allowed");

    let follow_up = format!("/api/documents/{id}/follow-up");
    let first = app
        .ok(&mut owner, "POST", &follow_up, json!({ "kind": "dunning" }))
        .await;
    assert_eq!(first["kind"], "dunning");
    assert_eq!(first["lines"], json!([]));
    assert_eq!(first["missing"], json!({}));
    assert_eq!(first["data"]["dunning"]["level"], 1);
    assert_eq!(
        first["data"]["dunning"]["invoice_number"],
        invoice["number"]
    );
    assert_eq!(first["data"]["dunning"]["amount_due"], "100.00");
    assert!(
        first["intro"]
            .as_str()
            .unwrap()
            .contains("Zahlung eingegangen")
    );
    let duplicate = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/duplicate", first["id"]),
            json!({}),
        )
        .await;
    assert_eq!(body(duplicate).await["code"], "follow_up_not_allowed");
    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", first["id"]),
            json!({}),
        )
        .await;
    assert!(issued["number"].as_str().unwrap().starts_with("MA-"));
    assert_eq!(issued["data"]["dunning"]["pay_until"], in_days(7));
    assert_eq!(
        issued["data"]["reference"],
        format!("Zu Rechnung {}", invoice["number"].as_str().unwrap())
    );

    let second = app
        .ok(&mut owner, "POST", &follow_up, json!({ "kind": "dunning" }))
        .await;
    assert_eq!(second["data"]["dunning"]["level"], 2);
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/documents/{id}/payments"),
        json!({ "date": in_days(0), "amount": 4_000 }),
    )
    .await;
    let second = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", second["id"]),
            json!({}),
        )
        .await;
    assert_eq!(
        second["data"]["dunning"]["amount_due"], "60.00",
        "the amount due when it is issued"
    );
    let open = app
        .ok(&mut owner, "GET", "/api/open-items", json!({}))
        .await;
    assert_eq!(open["invoices"][0]["dunning_level"], 2);
    assert_eq!(open["invoices"][0]["dunned_on"], in_days(0));

    let third = app
        .ok(&mut owner, "POST", &follow_up, json!({ "kind": "dunning" }))
        .await;
    let open = app
        .ok(&mut owner, "GET", "/api/open-items", json!({}))
        .await;
    assert_eq!(open["invoices"][0]["dunning_draft_id"], third["id"]);
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/documents/{id}/payments"),
        json!({ "date": in_days(0), "amount": 6_000 }),
    )
    .await;
    let open = app
        .ok(&mut owner, "GET", "/api/open-items", json!({}))
        .await;
    assert_eq!(
        open["invoices"],
        json!([]),
        "a paid invoice is no longer open"
    );
    let late = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", third["id"]),
            json!({}),
        )
        .await;
    assert_eq!(body(late).await["code"], "follow_up_not_allowed");
    let more = app
        .as_browser(&mut owner, "POST", &follow_up, json!({ "kind": "dunning" }))
        .await;
    assert_eq!(body(more).await["code"], "follow_up_not_allowed");
}
