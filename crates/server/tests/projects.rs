mod common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::{config::Config, money::Money, rates};
use serde_json::{Value, json};

async fn create(
    app: &common::TestApp,
    owner: &mut common::Browser,
    path: &str,
    body: Value,
) -> Value {
    let response = app.as_browser(owner, "POST", path, body).await;
    assert_eq!(response.status(), StatusCode::OK, "POST {path}");
    json(response).await
}

#[tokio::test]
async fn the_rate_comes_from_the_activity_then_the_project_then_the_customer() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = create(
        &app,
        &mut owner,
        "/api/customers",
        json!({ "name": "Krume", "language": "de", "hourly_rate": 8000 }),
    )
    .await;
    let customer_id = customer["id"].as_i64().unwrap();
    let plain = create(
        &app,
        &mut owner,
        "/api/projects",
        json!({ "customer_id": customer_id, "name": "Wartung" }),
    )
    .await;
    let rated = create(
        &app,
        &mut owner,
        "/api/projects",
        json!({ "customer_id": customer_id, "name": "Relaunch", "hourly_rate": 9000 }),
    )
    .await;
    let consulting = create(
        &app,
        &mut owner,
        "/api/activities",
        json!({ "name": "Beratung", "rates": [{ "currency": "EUR", "minor": 12000 }] }),
    )
    .await;
    let francs = create(
        &app,
        &mut owner,
        "/api/activities",
        json!({ "name": "Workshop", "rates": [{ "currency": "CHF", "minor": 15000 }] }),
    )
    .await;

    let eur = |minor| {
        Some(Money {
            minor,
            currency: "EUR".into(),
        })
    };
    let id = |v: &Value| v["id"].as_i64().unwrap();
    assert_eq!(
        rates::resolve(&app.app.db, id(&plain), None).await.unwrap(),
        eur(8000)
    );
    assert_eq!(
        rates::resolve(&app.app.db, id(&rated), None).await.unwrap(),
        eur(9000)
    );
    assert_eq!(
        rates::resolve(&app.app.db, id(&rated), Some(id(&consulting)))
            .await
            .unwrap(),
        eur(12000)
    );
    // the workshop has no euro rate, so the project's applies
    assert_eq!(
        rates::resolve(&app.app.db, id(&rated), Some(id(&francs)))
            .await
            .unwrap(),
        eur(9000)
    );

    let unrated = create(
        &app,
        &mut owner,
        "/api/customers",
        json!({ "name": "Albers", "language": "de" }),
    )
    .await;
    let bare = create(
        &app,
        &mut owner,
        "/api/projects",
        json!({ "customer_id": unrated["id"], "name": "Praxis" }),
    )
    .await;
    assert_eq!(
        rates::resolve(&app.app.db, id(&bare), None).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn projects_stay_with_their_customer_and_close_with_it() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let first = create(
        &app,
        &mut owner,
        "/api/customers",
        json!({ "name": "Krume", "language": "de" }),
    )
    .await;
    let second = create(
        &app,
        &mut owner,
        "/api/customers",
        json!({ "name": "Hofmann", "language": "de" }),
    )
    .await;
    let project = create(
        &app,
        &mut owner,
        "/api/projects",
        json!({ "customer_id": first["id"], "name": "Relaunch", "hourly_rate": 9000 }),
    )
    .await;
    assert_eq!(project["currency"], "EUR");
    assert_eq!(project["customer_number"], "K-001");

    let path = format!("/api/projects/{}", project["id"]);
    let moved = app
        .as_browser(
            &mut owner,
            "PUT",
            &path,
            json!({ "customer_id": second["id"], "name": "Relaunch" }),
        )
        .await;
    assert_eq!(moved.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json(moved).await["fields"]["customer_id"], "locked");

    let customer_path = format!("/api/customers/{}", first["id"]);
    let francs = app
        .as_browser(
            &mut owner,
            "PUT",
            &customer_path,
            json!({ "name": "Krume", "language": "de", "currency": "CHF" }),
        )
        .await;
    assert_eq!(json(francs).await["fields"]["currency"], "locked");

    let archived = app
        .as_browser(
            &mut owner,
            "PUT",
            &customer_path,
            json!({ "name": "Krume", "language": "de", "archived": true }),
        )
        .await;
    assert_eq!(archived.status(), StatusCode::OK);
    let shown = json(app.as_browser(&mut owner, "GET", &path, json!({})).await).await;
    assert_eq!(shown["closed"], true);
    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/projects",
            json!({ "customer_id": first["id"], "name": "Neu" }),
        )
        .await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    assert_eq!(json(refused).await["code"], "archived");
}

#[tokio::test]
async fn activities_take_one_rate_per_known_currency() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let twice = app
        .as_browser(&mut owner, "POST", "/api/activities", json!({ "name": "Beratung", "rates": [{ "currency": "EUR", "minor": 1 }, { "currency": "EUR", "minor": 2 }] }))
        .await;
    assert_eq!(json(twice).await["fields"]["rates"], "invalid_format");
    let unknown = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/activities",
            json!({ "name": "Beratung", "rates": [{ "currency": "XYZ", "minor": 1 }] }),
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let activity = json(
        app.as_browser(
            &mut owner,
            "POST",
            "/api/activities",
            json!({ "name": "Beratung", "rates": [{ "currency": "EUR", "minor": 12000 }] }),
        )
        .await,
    )
    .await;
    let path = format!("/api/activities/{}", activity["id"]);
    let changed = json(app.as_browser(&mut owner, "PUT", &path, json!({ "name": "Beratung", "rates": [{ "currency": "CHF", "minor": 13000 }, { "currency": "EUR", "minor": 12500 }] })).await).await;
    assert_eq!(
        changed["rates"],
        json!([{ "currency": "CHF", "minor": 13000 }, { "currency": "EUR", "minor": 12500 }])
    );
}
