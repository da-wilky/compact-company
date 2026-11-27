use crate::common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::config::Config;
use serde_json::{Value, json};

fn customer(name: &str) -> Value {
    json!({ "name": name, "language": "de", "hourly_rate": 8500 })
}

#[tokio::test]
async fn customers_get_consecutive_permanent_numbers_and_are_archived_not_deleted() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut staff = app.with_role(&mut owner, "staff").await;

    let first = json(
        app.as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            customer("Bäckerei Krume"),
        )
        .await,
    )
    .await;
    let second = json(
        app.as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            customer("Hofmann Logistik"),
        )
        .await,
    )
    .await;
    assert_eq!(first["number"], "K-001");
    assert_eq!(second["number"], "K-002");
    assert_eq!(first["currency"], "EUR");

    let id = first["id"].as_i64().unwrap();
    let archived = app
        .as_browser(
            &mut owner,
            "PUT",
            &format!("/api/customers/{id}"),
            json!({ "name": "Bäckerei Krume", "language": "de", "archived": true }),
        )
        .await;
    assert_eq!(archived.status(), StatusCode::OK);
    assert_eq!(json(archived).await["number"], "K-001");

    let active = json(
        app.as_browser(&mut staff, "GET", "/api/customers", json!({}))
            .await,
    )
    .await;
    assert_eq!(active.as_array().unwrap().len(), 1);
    assert_eq!(active[0]["name"], "Hofmann Logistik");
    let old = json(
        app.as_browser(&mut staff, "GET", "/api/customers?archived=true", json!({}))
            .await,
    )
    .await;
    assert_eq!(old[0]["number"], "K-001");

    let third = json(
        app.as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            customer("Praxis Albers"),
        )
        .await,
    )
    .await;
    assert_eq!(third["number"], "K-003");

    let staff_edit = app
        .as_browser(
            &mut staff,
            "PUT",
            &format!("/api/customers/{id}"),
            customer("Anders"),
        )
        .await;
    assert_eq!(staff_edit.status(), StatusCode::FORBIDDEN);

    let log = json(
        app.as_browser(
            &mut owner,
            "GET",
            &format!("/api/audit?subject=customer:{id}"),
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(log[0]["action"], "customer.update");
    assert_eq!(
        log[0]["before"],
        json!({ "archived": false, "hourly_rate": 8500 })
    );
    assert_eq!(
        log[0]["after"],
        json!({ "archived": true, "hourly_rate": null })
    );
    assert_eq!(log[1]["action"], "customer.create");
}

#[tokio::test]
async fn invalid_customers_are_refused_per_field() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let response = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            json!({ "name": "  ", "language": "de", "email": "nope", "currency": "XYZ", "hourly_rate": -1 }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json(response).await["fields"],
        json!({ "name": "required", "email": "invalid_format", "currency": "invalid_format", "hourly_rate": "invalid_format" })
    );
    let huge = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            json!({ "name": "A", "language": "de", "hourly_rate": 100_000_001 }),
        )
        .await;
    assert_eq!(
        json(huge).await["fields"],
        json!({ "hourly_rate": "invalid_format" })
    );
    let missing = app
        .as_browser(&mut owner, "GET", "/api/customers/99", json!({}))
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn new_customers_use_the_business_currency() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let saved = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "currency": "CHF" }),
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let created = json(
        app.as_browser(&mut owner, "POST", "/api/customers", customer("Zürich AG"))
            .await,
    )
    .await;
    assert_eq!(created["currency"], "CHF");
}

#[tokio::test]
async fn a_draft_is_addressed_to_the_billing_address_it_picks_and_keeps_it_when_issued() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let path = format!("/api/customers/{customer}");
    let main = app.ok(&mut owner, "GET", &path, json!({})).await["addresses"][0].clone();
    assert_eq!(main["street"], "Brotgasse 2");
    assert_eq!(main["is_default"], true);

    let both = app
        .ok(
            &mut owner,
            "PUT",
            &path,
            json!({ "name": "Bäckerei Krume", "language": "de", "addresses": [
                { "id": main["id"], "street": "Brotgasse 2", "postcode": "04109", "city": "Leipzig" },
                { "label": "Steuerbüro", "street": "c/o Steuerbüro Lind\nAm Ring 4",
                  "postcode": "04103", "city": "Leipzig" }
            ] }),
        )
        .await;
    assert_eq!(both["addresses"][0]["id"], main["id"]);
    let other = both["addresses"][1]["id"].as_i64().unwrap();

    let draft = |address: Value| {
        json!({ "kind": "invoice", "customer_id": customer, "address_id": address,
                "lines": [{ "description": "Brot", "quantity": "1", "unit": "piece",
                            "unit_price": 500 }] })
    };
    let plain = app
        .ok(&mut owner, "POST", "/api/documents", draft(Value::Null))
        .await;
    assert_eq!(plain["data"]["customer"]["street"], "Brotgasse 2");
    let picked = app
        .ok(&mut owner, "POST", "/api/documents", draft(json!(other)))
        .await;
    assert_eq!(
        picked["data"]["customer"]["street"],
        "c/o Steuerbüro Lind\nAm Ring 4"
    );
    assert_eq!(picked["data"]["customer"]["name"], "Bäckerei Krume");
    let id = picked["id"].as_i64().unwrap();
    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/issue"),
            json!({}),
        )
        .await;

    // the issued invoice's address is archived, the draft's falls back to the default
    let only_main = app
        .ok(
            &mut owner,
            "PUT",
            &path,
            json!({ "name": "Bäckerei Krume", "language": "de", "addresses": [
                { "id": main["id"], "street": "Brotgasse 3", "postcode": "04109", "city": "Leipzig" }
            ] }),
        )
        .await;
    assert_eq!(only_main["addresses"].as_array().unwrap().len(), 1);
    let again = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/documents/{id}"),
            json!({}),
        )
        .await;
    assert_eq!(again["data"], issued["data"]);
    let dunning = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/follow-up"),
            json!({ "kind": "dunning" }),
        )
        .await;
    assert_eq!(dunning["address_id"], Value::Null);
    assert_eq!(dunning["data"]["customer"]["street"], "Brotgasse 3");

    let refused = app
        .as_browser(&mut owner, "POST", "/api/documents", draft(json!(other)))
        .await;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json(refused).await["fields"]["address_id"],
        "invalid_format"
    );
    let foreign = app
        .as_browser(
            &mut owner,
            "PUT",
            &path,
            json!({ "name": "Bäckerei Krume", "language": "de",
                    "addresses": [{ "id": other, "city": "Leipzig" }] }),
        )
        .await;
    assert_eq!(
        json(foreign).await["fields"]["addresses.0.id"],
        "invalid_format"
    );
    let four_lines = app
        .as_browser(
            &mut owner,
            "PUT",
            &path,
            json!({ "name": "Bäckerei Krume", "language": "de",
                    "addresses": [{ "street": "a\nb\nc\nd" }] }),
        )
        .await;
    assert_eq!(
        json(four_lines).await["fields"]["addresses.0.street"],
        "invalid_format"
    );
    let taken = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/customers",
            json!({ "name": "Andere", "language": "de", "addresses": [{ "id": main["id"] }] }),
        )
        .await;
    assert_eq!(
        json(taken).await["fields"]["addresses.0.id"],
        "invalid_format"
    );
}
