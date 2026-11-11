mod common;

use std::{collections::BTreeMap, io::Read};

use axum::http::StatusCode;
use common::{json as body, start};
use compact_company::{config::Config, files::ContentHash};
use http_body_util::BodyExt;
use serde_json::json;

#[tokio::test]
async fn a_closed_year_refuses_postings_and_its_archive_matches_its_manifest() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    app.ready_to_issue(&mut owner).await;
    let last = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::get("Europe/Berlin").unwrap())
        .year()
        - 1;
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": format!("{}-01-01", last - 1) }),
    )
    .await;
    let accounts = app.ok(&mut owner, "GET", "/api/accounts", json!({})).await;
    let id = |number: &str| {
        accounts
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["number"] == number)
            .unwrap()["id"]
            .clone()
    };
    let post = |date: String| {
        json!({ "date": date, "description": "Bürobedarf", "lines": [
            { "account_id": id("6815"), "amount": 2_500 },
            { "account_id": id("1600"), "amount": -2_500 }
        ] })
    };
    app.ok(
        &mut owner,
        "POST",
        "/api/postings",
        post(format!("{last}-06-01")),
    )
    .await;

    let years = app.ok(&mut owner, "GET", "/api/years", json!({})).await;
    assert_eq!(years["next"], last - 1, "years close in order");
    let early = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/years/{last}/close"),
            json!({}),
        )
        .await;
    assert_eq!(body(early).await["fields"]["year"], "invalid_format");
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/years/{}/close", last - 1),
        json!({}),
    )
    .await;
    let mut accountant = app.with_role(&mut owner, "accountant").await;
    let refused = app
        .as_browser(
            &mut accountant,
            "POST",
            &format!("/api/years/{last}/close"),
            json!({}),
        )
        .await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let closed = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/years/{last}/close"),
            json!({}),
        )
        .await;
    assert_eq!(closed["year"], last);
    let again = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/years/{last}/close"),
            json!({}),
        )
        .await;
    assert_eq!(body(again).await["code"], "year_closed");
    assert_eq!(
        app.ok(&mut owner, "GET", "/api/years", json!({})).await["next"],
        serde_json::Value::Null,
        "the current year is not finished"
    );

    let late = app
        .as_browser(
            &mut owner,
            "POST",
            "/api/postings",
            post(format!("{last}-12-31")),
        )
        .await;
    assert_eq!(late.status(), StatusCode::CONFLICT);
    assert_eq!(body(late).await["code"], "year_closed");
    app.ok(
        &mut owner,
        "POST",
        "/api/postings",
        post(format!("{}-01-02", last + 1)),
    )
    .await;

    let response = app
        .as_browser(
            &mut accountant,
            "GET",
            &format!("/api/years/{last}/archive"),
            json!({}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        ContentHash::of(&bytes).to_string(),
        closed["archive"].as_str().unwrap()
    );
    let mut files = BTreeMap::new();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]));
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().display().to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        files.insert(path, content);
    }
    let sums = String::from_utf8(files.remove("SHA256SUMS").unwrap()).unwrap();
    let listed: BTreeMap<String, String> = sums
        .lines()
        .map(|l| {
            let (hash, path) = l.split_once("  ").unwrap();
            (path.to_owned(), hash.to_owned())
        })
        .collect();
    assert_eq!(
        listed.keys().collect::<Vec<_>>(),
        files.keys().collect::<Vec<_>>()
    );
    for (path, content) in &files {
        assert_eq!(listed[path], ContentHash::of(content).to_string(), "{path}");
    }
    let journal = String::from_utf8(files["journal.csv"].clone()).unwrap();
    assert!(journal.contains(&format!("{last}-06-01;")), "{journal}");
    assert!(journal.contains(";6815;\"Bürobedarf\";25,00;"), "{journal}");
    assert!(files.contains_key(&format!("datev/EXTF_Buchungsstapel_{last}.csv")));
    assert!(files.contains_key("tax/euer.json"));
    let audit: serde_json::Value = serde_json::from_slice(&files["audit.json"]).unwrap();
    assert!(
        audit
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "posting.create" && e["after"]["description"] == "Bürobedarf"),
        "{audit}"
    );
}
