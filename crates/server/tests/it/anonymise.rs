use crate::common;

use axum::http::StatusCode;
use common::{account, backdate, json as body, mirror_config, receipt, start, webdav};
use compact_company::mirror;
use serde_json::{Value, json};

async fn created_in(app: &common::TestApp, table: &str, id: i64, date: &str) {
    let sql = match table {
        "customer" => "UPDATE customer SET created_at = unixepoch(?) WHERE id = ?",
        "supplier" => "UPDATE supplier SET created_at = unixepoch(?) WHERE id = ?",
        _ => unreachable!(),
    };
    sqlx::query(sql)
        .bind(date)
        .bind(id)
        .execute(&app.app.db)
        .await
        .unwrap();
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[tokio::test]
async fn an_anonymised_year_keeps_later_figures_and_no_names() {
    let (url, dav) = webdav().await;
    let app = start(mirror_config(&url)).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let this_year = jiff::Zoned::now().year();
    let year = this_year - 11;
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": format!("{year}-01-01") }),
    )
    .await;
    let bank = account(&app, &mut owner, "1800").await;
    let office = account(&app, &mut owner, "6815").await;
    let meyer = app
        .ok(
            &mut owner,
            "POST",
            "/api/suppliers",
            json!({ "name": "Bürohaus Meyer" }),
        )
        .await["id"]
        .as_i64()
        .unwrap();
    let laden = app
        .ok(
            &mut owner,
            "POST",
            "/api/suppliers",
            json!({ "name": "Papierladen Schulz" }),
        )
        .await["id"]
        .as_i64()
        .unwrap();
    created_in(&app, "customer", customer, &format!("{year}-01-02")).await;
    created_in(&app, "supplier", meyer, &format!("{year}-01-02")).await;
    let lines = |description: &str| json!([{ "description": description, "account_id": office, "net": 2_000, "vat_rate": "19" }]);
    receipt(
        &app,
        &mut owner,
        b"%PDF-1.4 old paper",
        json!({ "supplier_id": meyer, "number": "R-17", "date": format!("{year}-03-04"),
                "note": "von Herrn Meyer persönlich", "lines": lines("Papier für Frau Krume") }),
        &bank,
    )
    .await;
    receipt(
        &app,
        &mut owner,
        b"%PDF-1.4 later paper",
        json!({ "supplier_id": laden, "number": "S-1", "date": format!("{}-02-01", year + 1),
                "lines": lines("Papier") }),
        &bank,
    )
    .await;
    let project = app
        .ok(
            &mut owner,
            "POST",
            "/api/projects",
            json!({ "customer_id": customer, "name": "Website Krume" }),
        )
        .await;
    let start_at = jiff::civil::date(year, 6, 1)
        .at(9, 0, 0, 0)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .unwrap()
        .timestamp()
        .as_second();
    app.ok(
        &mut owner,
        "POST",
        "/api/time",
        json!({ "project_id": project["id"], "started_at": start_at, "ended_at": start_at + 3600,
                "description": "Telefonat mit Frau Krume" }),
    )
    .await;
    let quote = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "quote", "customer_id": customer, "intro": "Liebe Frau Krume,",
                    "lines": [{ "description": "Beratung Krume", "quantity": "1", "unit": "hour",
                                "unit_price": 9000 }] }),
        )
        .await["id"]
        .as_i64()
        .unwrap();
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/documents/{quote}/issue"),
        json!({}),
    )
    .await;
    backdate(&app, quote, &format!("{year}-05-02")).await;
    for year in year..this_year {
        app.ok(
            &mut owner,
            "POST",
            &format!("/api/years/{year}/close"),
            json!({}),
        )
        .await;
    }
    mirror::resync(&app.app.db).await.unwrap();
    mirror::copy_due(&app.app).await.unwrap();
    assert!(
        dav.files
            .lock()
            .unwrap()
            .keys()
            .any(|p| p.contains("Bürohaus Meyer"))
    );

    let before = figures(&app, &mut owner, year + 1).await;

    let retention = app.ok(&mut owner, "GET", "/api/retention", json!({})).await;
    let plan = &retention.as_array().unwrap().last().unwrap()["anonymisation"];
    assert_eq!(plan["blockers"], json!([]), "{plan}");
    assert_eq!(plan["customers"][0]["id"], customer);
    let suppliers: Vec<&Value> = plan["suppliers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| &s["id"])
        .collect();
    assert_eq!(
        suppliers,
        [&json!(meyer)],
        "Schulz has a receipt of a later year"
    );
    let next = &retention.as_array().unwrap()[retention.as_array().unwrap().len() - 2];
    assert!(
        next["anonymisation"]["from"].as_str().unwrap() > &format!("{this_year}-12-31")[..],
        "the next year is not due yet"
    );

    let path = format!("/api/retention/{year}/anonymise");
    let early = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/retention/{}/anonymise", year + 1),
            json!({ "no_open_assessment": true }),
        )
        .await;
    assert_eq!(body(early).await["code"], "anonymisation_not_due");
    let done = app
        .ok(
            &mut owner,
            "POST",
            &path,
            json!({ "no_open_assessment": true }),
        )
        .await;
    assert!(done["anonymisation"]["anonymised_at"].is_i64());
    let again = app
        .as_browser(
            &mut owner,
            "POST",
            &path,
            json!({ "no_open_assessment": true }),
        )
        .await;
    assert_eq!(again.status(), StatusCode::CONFLICT);
    assert_eq!(figures(&app, &mut owner, year + 1).await, before);
    let archive = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/years/{year}/archive"),
            json!({}),
        )
        .await;
    assert_eq!(archive.status(), StatusCode::NOT_FOUND);

    mirror::copy_due(&app.app).await.unwrap();
    let mirrored: Vec<String> = dav.files.lock().unwrap().keys().cloned().collect();
    assert!(
        !mirrored.iter().any(|p| p.starts_with(&format!("{year}/"))),
        "{mirrored:?}"
    );
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&app.app.db)
        .await
        .unwrap();
    let data = app.dir.path().join("data");
    let mut bytes = std::fs::read(data.join("compact-company.db")).unwrap();
    bytes.extend(std::fs::read(data.join("compact-company.db-wal")).unwrap_or_default());
    for name in ["Krume", "Meyer"] {
        assert!(!contains(&bytes, name), "{name} is still in the database");
    }
    assert!(
        contains(&bytes, "Papierladen Schulz"),
        "later suppliers stay"
    );
    let name: String = sqlx::query_scalar("SELECT name FROM supplier WHERE id = ?")
        .bind(meyer)
        .fetch_one(&app.app.db)
        .await
        .unwrap();
    assert_eq!(
        name,
        format!("Supplier {meyer} (anonymised)"),
        "English without a business language"
    );
}

/// The EÜR of a year and every account's balance.
async fn figures(app: &common::TestApp, owner: &mut common::Browser, year: i16) -> Value {
    let euer = app
        .ok(
            owner,
            "GET",
            &format!("/api/tax/euer?year={year}"),
            json!({}),
        )
        .await;
    let accounts: Vec<Value> = app
        .ok(owner, "GET", "/api/accounts", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|a| json!([a["number"], a["balance"]]))
        .collect();
    json!({ "euer": euer, "accounts": accounts })
}
