use crate::common;

use axum::http::{StatusCode, header};
use common::{smtp, smtp_config, start, text};
use serde_json::json;

#[tokio::test]
async fn a_calendar_subscribes_to_the_deadlines_and_a_reminder_goes_out_a_week_before() {
    let (port, received) = smtp(false).await;
    let app = start(smtp_config(port)).await;
    let mut owner = app.setup_owner().await;
    app.ok(
        &mut owner,
        "PUT",
        "/api/business",
        json!({
            "name": "swilk", "email": "fristen@swilk.example", "taxation": "accrual",
            "vat_period": "quarterly", "vat_extension": false
        }),
    )
    .await;

    let mut accountant = app.with_role(&mut owner, "accountant").await;
    let upcoming = app
        .ok(&mut accountant, "GET", "/api/deadlines", json!({}))
        .await;
    let upcoming = upcoming.as_array().unwrap();
    assert!(
        upcoming.iter().any(|d| d["kind"] == "vat_advance"),
        "{upcoming:?}"
    );
    let refused = app
        .as_browser(&mut accountant, "GET", "/api/calendar-feed", json!({}))
        .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let feed = app
        .ok(&mut owner, "POST", "/api/calendar-feed", json!({}))
        .await;
    let link = feed["link"].as_str().unwrap();
    let path = &link[link.find("/api/").unwrap()..];
    let response = app.get(path).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/calendar; charset=utf-8"
    );
    let calendar = text(response).await;
    assert!(calendar.starts_with("BEGIN:VCALENDAR\r\n"));
    assert!(calendar.contains("SUMMARY:USt-Voranmeldung 3. Quartal"));
    assert!(calendar.contains("DTSTART;VALUE=DATE:"));
    assert!(calendar.ends_with("END:VCALENDAR\r\n"));
    assert_eq!(
        app.get("/api/calendar/guessed.ics").await.status(),
        StatusCode::NOT_FOUND
    );
    app.ok(&mut owner, "POST", "/api/calendar-feed", json!({}))
        .await;
    assert_eq!(
        app.get(path).await.status(),
        StatusCode::NOT_FOUND,
        "a renewed link replaces the old one"
    );
    let gone = app
        .ok(&mut owner, "DELETE", "/api/calendar-feed", json!({}))
        .await;
    assert_eq!(gone["created_at"], serde_json::Value::Null);

    // Q3 2026 is due on Monday 12 October, the 10th being a Saturday
    let remind = |day: &str| compact_company::deadlines::remind(&app.app, day.parse().unwrap());
    assert_eq!(remind("2026-10-04").await.unwrap(), 0, "eight days before");
    assert_eq!(remind("2026-10-05").await.unwrap(), 1);
    assert_eq!(remind("2026-10-06").await.unwrap(), 0, "sent once");
    assert_eq!(remind("2026-10-11").await.unwrap(), 1, "and the day before");
    let mails = received.lock().unwrap().clone();
    assert_eq!(mails.len(), 2);
    assert!(mails[0].contains("To: fristen@swilk.example"));
    assert!(
        mails[0].contains("Frist: USt-Voranmeldung 3. Quartal 2026 am 12.10.2026"),
        "{}",
        mails[0]
    );
}
