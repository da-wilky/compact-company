use crate::common;

use std::collections::BTreeMap;

use axum::http::StatusCode;
use common::{account, backdate, json as body, mirror_config, receipt, start, webdav};
use compact_company::{api::years, files::ContentHash, mirror};
use http_body_util::BodyExt;
use serde_json::{Value, json};

#[tokio::test]
async fn files_past_their_period_are_deleted_and_pruned_from_archive_and_mirror() {
    let (url, dav) = webdav().await;
    let app = start(mirror_config(&url)).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let this_year = jiff::Zoned::now().year();
    let year = this_year - 10;
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": format!("{year}-01-01") }),
    )
    .await;
    let supplier = app
        .ok(
            &mut owner,
            "POST",
            "/api/suppliers",
            json!({ "name": "Bürohaus Meyer" }),
        )
        .await;
    let bank = account(&app, &mut owner, "1800").await;
    let office = account(&app, &mut owner, "6815").await;
    let low_value = account(&app, &mut owner, "0670").await;
    let paper = receipt(
        &app,
        &mut owner,
        b"%PDF-1.4 paper",
        json!({ "supplier_id": supplier["id"], "number": "R-17", "date": format!("{year}-03-04"),
                "lines": [{ "description": "Papier", "account_id": office, "net": 2_000, "vat_rate": "19" }] }),
        &bank,
    )
    .await;
    let chair = receipt(
        &app,
        &mut owner,
        b"%PDF-1.4 chair",
        json!({ "supplier_id": supplier["id"], "number": "R-18", "date": format!("{year}-04-15"),
                "lines": [{ "description": "Stuhl", "account_id": low_value, "net": 50_000, "vat_rate": "19" }] }),
        &bank,
    )
    .await;
    let line = app.ok(&mut owner, "GET", "/api/assets", json!({})).await["candidates"][0]
        ["receipt_line_id"]
        .clone();
    app.ok(
        &mut owner,
        "POST",
        "/api/assets",
        json!({ "name": "Bürostuhl", "kind": "office", "method": "low_value", "receipt_line_id": line }),
    )
    .await;
    app.ok(
        &mut owner,
        "POST",
        "/api/assets/depreciation",
        json!({ "year": year }),
    )
    .await;
    let quote = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "quote", "customer_id": customer, "lines": [
                { "description": "Beratung", "quantity": "1", "unit": "hour", "unit_price": 9000 }
            ] }),
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
    let paper_path = format!("{year}/Belege/{year}-03-04 Bürohaus Meyer R-17.pdf");
    assert!(dav.files.lock().unwrap().contains_key(&paper_path));

    let retention = app.ok(&mut owner, "GET", "/api/retention", json!({})).await;
    let oldest = retention.as_array().unwrap().last().unwrap().clone();
    assert_eq!(oldest["year"], year);
    let item = |list: &Value, subject: String| {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["subject"] == subject)
            .unwrap()
            .clone()
    };
    let chair_item = item(&oldest, format!("receipt:{chair}"));
    assert_eq!(
        chair_item["held"],
        json!({ "reason": "asset", "by": "Bürostuhl" })
    );
    let quote_item = item(&oldest, format!("document:{quote}"));
    assert_eq!(quote_item["class"], "business_letter");
    assert_eq!(quote_item["deletable_from"], format!("{}-01-01", year + 7));
    assert_eq!(
        item(&oldest, format!("receipt:{paper}"))["deletable_from"],
        format!("{}-01-01", year + 9)
    );
    assert_eq!(oldest["deletable"], 2);
    let newest = &retention[0];
    assert_eq!(newest["deletable"], 0, "last year's files are kept");

    let path = format!("/api/retention/{year}/delete");
    let unconfirmed = app
        .as_browser(
            &mut owner,
            "POST",
            &path,
            json!({ "no_open_assessment": false }),
        )
        .await;
    assert_eq!(unconfirmed.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let before = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/years/{year}/archive"),
            json!({}),
        )
        .await
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let after = app
        .ok(
            &mut owner,
            "POST",
            &path,
            json!({ "no_open_assessment": true }),
        )
        .await;
    assert_eq!(after["deletable"], 0);
    assert!(after["pruned"].as_bool().unwrap());
    assert!(item(&after, format!("receipt:{paper}"))["deleted_at"].is_i64());
    assert!(item(&after, format!("receipt:{chair}"))["deleted_at"].is_null());
    let again = app
        .as_browser(
            &mut owner,
            "POST",
            &path,
            json!({ "no_open_assessment": true }),
        )
        .await;
    assert_eq!(body(again).await["code"], "nothing_to_delete");

    let gone = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/receipts/{paper}/file"),
            json!({}),
        )
        .await;
    assert_eq!(gone.status(), StatusCode::GONE);
    assert!(
        app.app
            .files
            .get(ContentHash::of(b"%PDF-1.4 paper"))
            .await
            .is_err(),
        "the file left the store"
    );
    assert!(
        app.app.files.get(ContentHash::of(&before)).await.is_err(),
        "so did the archive that held it"
    );
    let receipt = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/receipts/{paper}"),
            json!({}),
        )
        .await;
    assert_eq!(receipt["lines"][0]["net"], 2_000, "the record stays");

    let pruned = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/years/{year}/archive"),
            json!({}),
        )
        .await
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    years::verify_archive(&pruned).unwrap();
    let original: BTreeMap<String, Vec<u8>> = years::unpack(&before).unwrap().into_iter().collect();
    let files: BTreeMap<String, Vec<u8>> = years::unpack(&pruned).unwrap().into_iter().collect();
    assert_eq!(
        files["SHA256SUMS"], original["SHA256SUMS"],
        "the original sums stay"
    );
    let sums = String::from_utf8(original["SHA256SUMS"].clone()).unwrap();
    let mut missing = Vec::new();
    for line in sums.lines() {
        let (hash, path) = line.split_once("  ").unwrap();
        match files.get(path) {
            Some(bytes) => assert_eq!(ContentHash::of(bytes).to_string(), hash, "{path}"),
            None => missing.push(path.to_owned()),
        }
    }
    assert_eq!(missing.len(), 2, "{missing:?}");
    let deleted = String::from_utf8(files["DELETED"].clone()).unwrap();
    for path in &missing {
        assert!(deleted.contains(path.as_str()), "{deleted}");
    }

    mirror::copy_due(&app.app).await.unwrap();
    let mirrored = dav.files.lock().unwrap().clone();
    assert!(!mirrored.contains_key(&paper_path));
    assert!(mirrored.contains_key(&format!(
        "{year}/Belege/{year}-04-15 Bürohaus Meyer R-18.pdf"
    )));
    assert_eq!(
        mirrored[&format!("{year}/Jahresarchiv-{year}.tar.gz")],
        pruned.to_vec()
    );

    let audit = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/audit?subject=year:{year}"),
            json!({}),
        )
        .await;
    assert!(
        audit
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "retention.delete"),
        "{audit}"
    );
}
