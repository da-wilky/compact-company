mod common;

use std::collections::BTreeMap;

use axum::http::StatusCode;
use common::{Browser, TestApp, json as body, start};
use compact_company::config::Config;
use serde_json::{Value, json};

fn this_year() -> i16 {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::get("Europe/Berlin").unwrap())
        .year()
}

async fn account(app: &TestApp, owner: &mut Browser, number: &str) -> Value {
    app.ok(owner, "GET", "/api/accounts", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == number)
        .unwrap()
        .clone()
}

/// An owner of a business on accrual with SKR 04 books from the first day of last year.
async fn books(app: &TestApp) -> Browser {
    let mut owner = app.setup_owner().await;
    app.ready_to_issue(&mut owner).await;
    app.ok(
        &mut owner,
        "PUT",
        "/api/business",
        json!({
            "name": "swilk UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
            "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
            "taxation": "accrual", "iban": "DE89370400440532013000"
        }),
    )
    .await;
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": format!("{}-01-01", this_year() - 1) }),
    )
    .await;
    owner
}

async fn status(
    app: &TestApp,
    owner: &mut Browser,
    method: &str,
    path: &str,
    input: Value,
) -> (StatusCode, Value) {
    let response = app.as_browser(owner, method, path, input).await;
    (response.status(), body(response).await)
}

/// Amounts by Zeile.
async fn euer(app: &TestApp, owner: &mut Browser, year: i16) -> BTreeMap<String, i64> {
    app.ok(
        owner,
        "GET",
        &format!("/api/tax/euer?year={year}"),
        json!({}),
    )
    .await["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["key"].as_str().unwrap().to_owned(),
                f["amount"].as_i64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_laptop_depreciates_pro_rata_and_a_monitor_is_written_off_at_once() {
    let app = start(Config::default()).await;
    let mut owner = books(&app).await;
    let last = this_year() - 1;
    let supplier = app
        .ok(
            &mut owner,
            "POST",
            "/api/suppliers",
            json!({ "name": "Computerhaus Dresden" }),
        )
        .await;
    let photo = app
        .multipart(
            &mut owner,
            "POST",
            "/api/receipts",
            &[("file", Some("bon.jpg"), &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16])],
        )
        .await;
    let receipt = body(photo).await;
    let path = format!("/api/receipts/{}", receipt["id"]);
    let office = account(&app, &mut owner, "0650").await["id"].clone();
    let low_value = account(&app, &mut owner, "0670").await["id"].clone();
    app.ok(
        &mut owner,
        "PUT",
        &path,
        json!({ "supplier_id": supplier["id"], "date": format!("{last}-04-15"), "lines": [
            { "description": "Laptop", "account_id": office, "net": 180_000, "vat_rate": "19" },
            { "description": "Monitor", "account_id": low_value, "net": 30_000, "vat_rate": "19" }
        ] }),
    )
    .await;
    let bank = account(&app, &mut owner, "1800").await["id"].clone();
    app.ok(
        &mut owner,
        "POST",
        &format!("{path}/book"),
        json!({ "paid_from": bank }),
    )
    .await;

    let listing = app.ok(&mut owner, "GET", "/api/assets", json!({})).await;
    let candidates = listing["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    let line = |name: &str| {
        candidates
            .iter()
            .find(|c| c["description"] == name)
            .unwrap()["receipt_line_id"]
            .clone()
    };
    let (status_code, error) = status(
        &app,
        &mut owner,
        "POST",
        "/api/assets",
        json!({ "name": "Laptop", "kind": "office", "method": "low_value",
                "receipt_line_id": line("Laptop") }),
    )
    .await;
    assert_eq!(status_code, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
    let laptop = app
        .ok(
            &mut owner,
            "POST",
            "/api/assets",
            json!({ "name": "Laptop", "kind": "office", "method": "straight_line",
                    "life_years": 3, "receipt_line_id": line("Laptop") }),
        )
        .await;
    assert_eq!(laptop["cost"], 180_000);
    assert_eq!(laptop["acquired"], format!("{last}-04-15"));
    assert_eq!(laptop["schedule"][0]["depreciation"], 45_000);
    app.ok(
        &mut owner,
        "POST",
        "/api/assets",
        json!({ "name": "Monitor", "kind": "office", "method": "low_value",
                "receipt_line_id": line("Monitor") }),
    )
    .await;

    let listing = app.ok(&mut owner, "GET", "/api/assets", json!({})).await;
    assert_eq!(listing["due"], json!([last]));
    assert_eq!(listing["candidates"], json!([]));
    let (status_code, error) = status(
        &app,
        &mut owner,
        "POST",
        &format!("/api/years/{last}/close"),
        json!({}),
    )
    .await;
    assert_eq!(status_code, StatusCode::CONFLICT);
    assert_eq!(error["code"], "depreciation_due");

    let booked = app
        .ok(
            &mut owner,
            "POST",
            "/api/assets/depreciation",
            json!({ "year": last }),
        )
        .await;
    assert_eq!(booked["due"], json!([]));
    let figures = euer(&app, &mut owner, last).await;
    assert_eq!(figures.get("33"), Some(&45_000));
    assert_eq!(figures.get("36"), Some(&30_000));

    let register = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/assets/register?year={last}"),
            json!({}),
        )
        .await;
    assert_eq!(
        register["groups"]["office"],
        json!({ "cost": 180_000, "start": 0, "additions": 180_000, "depreciation": 45_000,
                "disposals": 0, "end": 135_000 })
    );
    assert_eq!(register["low_value_total"], 30_000);
    assert_eq!(account(&app, &mut owner, "0650").await["balance"], 135_000);
    assert_eq!(account(&app, &mut owner, "0670").await["balance"], 0);

    let (status_code, error) = status(
        &app,
        &mut owner,
        "POST",
        &format!("{path}/reverse"),
        json!({}),
    )
    .await;
    assert_eq!(status_code, StatusCode::CONFLICT);
    assert_eq!(error["code"], "has_asset");
    let journal = app.ok(&mut owner, "GET", "/api/postings", json!({})).await;
    let afa = journal
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["asset_id"] == laptop["id"])
        .unwrap();
    assert_eq!(afa["date"], format!("{last}-12-31"));
    let (status_code, error) = status(
        &app,
        &mut owner,
        "POST",
        &format!("/api/postings/{}/reverse", afa["id"]),
        json!({}),
    )
    .await;
    assert_eq!(error["code"], "automatic_posting", "{status_code}");
    let (_, error) = status(
        &app,
        &mut owner,
        "PUT",
        &format!("/api/assets/{}", laptop["id"]),
        json!({ "name": "Laptop", "kind": "office", "method": "straight_line",
                "life_years": 4, "receipt_line_id": line("Laptop") }),
    )
    .await;
    assert_eq!(error["code"], "asset_booked");

    app.ok(
        &mut owner,
        "POST",
        &format!("/api/years/{last}/close"),
        json!({}),
    )
    .await;
}

#[tokio::test]
async fn a_disposal_books_depreciation_through_its_month_and_writes_off_the_rest() {
    let app = start(Config::default()).await;
    let mut owner = books(&app).await;
    let (last, this) = (this_year() - 1, this_year());
    let bank = account(&app, &mut owner, "1800").await["id"].clone();
    let mut by_hand = async |number: &str, name: &str, kind: &str, cost: i64, life: i16| {
        let id = account(&app, &mut owner, number).await["id"].clone();
        app.ok(
            &mut owner,
            "POST",
            "/api/postings",
            json!({ "date": format!("{last}-01-10"), "description": name, "lines": [
                { "account_id": id, "amount": cost }, { "account_id": bank, "amount": -cost }
            ] }),
        )
        .await;
        app.ok(
            &mut owner,
            "POST",
            "/api/assets",
            json!({ "name": name, "kind": kind, "method": "straight_line", "life_years": life,
                    "account_id": id, "acquired": format!("{last}-01-10"), "cost": cost }),
        )
        .await
    };
    let software = by_hand("0135", "Software", "intangible", 120_000, 3).await;
    let chair = by_hand("0650", "Stuhl", "office", 60_000, 5).await;
    app.ok(
        &mut owner,
        "POST",
        "/api/assets/depreciation",
        json!({ "year": last }),
    )
    .await;
    let figures = euer(&app, &mut owner, last).await;
    assert_eq!(figures.get("32"), Some(&40_000));
    assert_eq!(figures.get("33"), Some(&12_000));

    // the chair left last year after its year was booked: that year is booked again
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/assets/{}/dispose", chair["id"]),
        json!({ "date": format!("{last}-06-30") }),
    )
    .await;
    let figures = euer(&app, &mut owner, last).await;
    assert_eq!(figures.get("33"), Some(&6_000));
    assert_eq!(figures.get("38"), Some(&54_000));
    assert_eq!(account(&app, &mut owner, "0650").await["balance"], 0);

    let software = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/assets/{}/dispose", software["id"]),
            json!({ "date": format!("{this}-01-01") }),
        )
        .await;
    assert_eq!(
        software["schedule"][1],
        json!({ "year": this, "depreciation": 3_333, "disposal": 76_667, "book_value": 0,
                "booked": true })
    );
    let figures = euer(&app, &mut owner, this).await;
    assert_eq!(figures.get("32"), Some(&3_333));
    assert_eq!(figures.get("38"), Some(&76_667));
    let register = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/assets/register?year={this}"),
            json!({}),
        )
        .await;
    assert_eq!(
        register["groups"]["intangible"],
        json!({ "cost": 120_000, "start": 80_000, "additions": 0, "depreciation": 3_333,
                "disposals": 76_667, "end": 0 })
    );
    assert_eq!(account(&app, &mut owner, "0135").await["balance"], 0);
    let (status_code, _) = status(
        &app,
        &mut owner,
        "POST",
        &format!("/api/assets/{}/dispose", software["id"]),
        json!({ "date": format!("{this}-01-01") }),
    )
    .await;
    assert_eq!(status_code, StatusCode::UNPROCESSABLE_ENTITY);
}
