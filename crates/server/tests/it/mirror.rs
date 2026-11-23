use crate::common;

use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use common::{json as body, mirror_config, start, webdav};
use compact_company::mirror;
use serde_json::json;

#[tokio::test]
async fn stored_documents_reach_the_mirror_also_after_it_was_down() {
    let (url, dav) = webdav().await;
    let app = start(mirror_config(&url)).await;
    let mut owner = app.setup_owner().await;
    let year = jiff::Zoned::now().year();

    let response = app
        .multipart(
            &mut owner,
            "POST",
            "/api/receipts",
            &[("file", Some("scan.pdf"), b"%PDF-1.4 first receipt")],
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let first = body(response).await["id"].as_i64().unwrap();
    assert_eq!(mirror::copy_due(&app.app).await.unwrap(), 1);
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let first_path = format!("{year}/Receipts/{today} Receipt {first}.pdf");
    assert_eq!(
        dav.files.lock().unwrap().get(&first_path).unwrap(),
        b"%PDF-1.4 first receipt"
    );

    dav.down.store(true, Ordering::SeqCst);
    let response = app
        .multipart(
            &mut owner,
            "POST",
            "/api/receipts",
            &[("file", Some("second.pdf"), b"%PDF-1.4 second receipt")],
        )
        .await;
    let second = body(response).await["id"].as_i64().unwrap();
    assert_eq!(mirror::copy_due(&app.app).await.unwrap(), 0);
    let status = app.ok(&mut owner, "GET", "/api/mirror", json!({})).await;
    assert_eq!(status["pending"], 1);
    assert!(
        status["error"].as_str().unwrap().contains("503"),
        "{status}"
    );
    assert_eq!(
        mirror::copy_due(&app.app).await.unwrap(),
        0,
        "a failed copy waits before it is tried again"
    );

    dav.down.store(false, Ordering::SeqCst);
    sqlx::query("UPDATE mirror_pending SET due_at = 0")
        .execute(&app.app.db)
        .await
        .unwrap();
    assert_eq!(mirror::copy_due(&app.app).await.unwrap(), 1);
    let second_path = format!("{year}/Receipts/{today} Receipt {second}.pdf");
    assert!(dav.files.lock().unwrap().contains_key(&second_path));
    let status = app.ok(&mut owner, "GET", "/api/mirror", json!({})).await;
    assert_eq!(status["pending"], 0);
    assert_eq!(status["error"], serde_json::Value::Null);

    let customer = app.ready_to_issue(&mut owner).await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Beratung", "quantity": "1", "unit": "hour", "unit_price": 9000 }
            ] }),
        )
        .await;
    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", draft["id"]),
            json!({}),
        )
        .await;
    mirror::copy_due(&app.app).await.unwrap();
    let invoice = format!("{year}/Invoices/{}.pdf", issued["number"].as_str().unwrap());
    assert!(
        dav.files.lock().unwrap()[&invoice].starts_with(b"%PDF"),
        "{:?}",
        dav.files.lock().unwrap().keys()
    );

    let deleted = app
        .as_browser(
            &mut owner,
            "DELETE",
            &format!("/api/receipts/{second}"),
            json!({}),
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    mirror::copy_due(&app.app).await.unwrap();
    assert!(!dav.files.lock().unwrap().contains_key(&second_path));

    dav.files
        .lock()
        .unwrap()
        .insert(first_path.clone(), b"changed in Nextcloud".to_vec());
    dav.files.lock().unwrap().insert(
        format!("{year}/Notizen.txt"),
        b"added in Nextcloud".to_vec(),
    );
    let tree = |dav: &common::Dav| dav.files.lock().unwrap().clone();
    let resync = app
        .as_browser(&mut owner, "POST", "/api/mirror/resync", json!({}))
        .await;
    assert_eq!(resync.status(), StatusCode::NO_CONTENT);
    mirror::copy_due(&app.app).await.unwrap();
    let once = tree(&dav);
    assert_eq!(once[&first_path], b"%PDF-1.4 first receipt", "the app wins");
    assert!(
        once.contains_key(&format!("{year}/Notizen.txt")),
        "others are left alone"
    );
    app.as_browser(&mut owner, "POST", "/api/mirror/resync", json!({}))
        .await;
    mirror::copy_due(&app.app).await.unwrap();
    assert_eq!(tree(&dav), once);
}

#[tokio::test]
async fn names_follow_the_business_language_unless_the_mirror_sets_one() {
    let (url, dav) = webdav().await;
    let app = start(mirror_config(&url)).await;
    let mut owner = app.setup_owner().await;
    let year = jiff::Zoned::now().year();
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let response = app
        .multipart(
            &mut owner,
            "POST",
            "/api/receipts",
            &[("file", Some("scan.pdf"), b"%PDF-1.4 receipt")],
        )
        .await;
    let id = body(response).await["id"].as_i64().unwrap();
    let paths = |dav: &common::Dav| {
        dav.files
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };

    mirror::copy_due(&app.app).await.unwrap();
    assert_eq!(
        paths(&dav),
        [format!("{year}/Receipts/{today} Receipt {id}.pdf")]
    );

    app.ok(
        &mut owner,
        "PUT",
        "/api/business",
        json!({ "language": "de" }),
    )
    .await;
    mirror::copy_due(&app.app).await.unwrap();
    assert_eq!(
        paths(&dav),
        [format!("{year}/Belege/{today} Beleg {id}.pdf")],
        "a new language moves the files"
    );
    let folders = |dav: &common::Dav| dav.folders.lock().unwrap().clone();
    assert!(folders(&dav).contains(&format!("{year}/Belege")));
    assert!(
        !folders(&dav).contains(&format!("{year}/Receipts")),
        "the emptied folder is gone"
    );

    let note = format!("{year}/Belege/Notiz.txt");
    dav.files
        .lock()
        .unwrap()
        .insert(note.clone(), b"added in Nextcloud".to_vec());
    let deleted = app
        .as_browser(
            &mut owner,
            "DELETE",
            &format!("/api/receipts/{id}"),
            json!({}),
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    mirror::copy_due(&app.app).await.unwrap();
    assert_eq!(paths(&dav), [note], "a folder with other files stays");
    assert!(folders(&dav).contains(&format!("{year}/Belege")));

    let mut config = mirror_config(&url);
    config.mirror.language = Some(compact_company::api::customers::Language::En);
    let app = start(config).await;
    let mut owner = app.setup_owner().await;
    app.ok(
        &mut owner,
        "PUT",
        "/api/business",
        json!({ "language": "de" }),
    )
    .await;
    let response = app
        .multipart(
            &mut owner,
            "POST",
            "/api/receipts",
            &[("file", Some("other.pdf"), b"%PDF-1.4 other")],
        )
        .await;
    let other = body(response).await["id"].as_i64().unwrap();
    mirror::copy_due(&app.app).await.unwrap();
    assert!(
        paths(&dav).contains(&format!("{year}/Receipts/{today} Receipt {other}.pdf")),
        "{:?}",
        paths(&dav)
    );
}

#[tokio::test]
async fn a_renamed_supplier_renames_its_receipts() {
    let (url, dav) = webdav().await;
    let app = start(mirror_config(&url)).await;
    let mut owner = app.setup_owner().await;
    let year = jiff::Zoned::now().year();
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let supplier = app
        .ok(
            &mut owner,
            "POST",
            "/api/suppliers",
            json!({ "name": "Meyer" }),
        )
        .await;
    let response = app
        .multipart(
            &mut owner,
            "POST",
            "/api/receipts",
            &[("file", Some("scan.pdf"), b"%PDF-1.4 receipt")],
        )
        .await;
    let id = body(response).await["id"].as_i64().unwrap();
    app.ok(
        &mut owner,
        "PUT",
        &format!("/api/receipts/{id}"),
        json!({ "supplier_id": supplier["id"], "number": "R-1" }),
    )
    .await;
    let paths = |dav: &common::Dav| {
        dav.files
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    mirror::copy_due(&app.app).await.unwrap();
    assert_eq!(
        paths(&dav),
        [format!("{year}/Receipts/{today} Meyer R-1.pdf")]
    );

    app.ok(
        &mut owner,
        "PUT",
        &format!("/api/suppliers/{}", supplier["id"]),
        json!({ "name": "Meyer GmbH" }),
    )
    .await;
    mirror::copy_due(&app.app).await.unwrap();
    assert_eq!(
        paths(&dav),
        [format!("{year}/Receipts/{today} Meyer GmbH R-1.pdf")]
    );
}

#[tokio::test]
async fn a_missing_mirror_folder_is_created() {
    let (url, dav) = webdav().await;
    dav.folders.lock().unwrap().clear();
    let app = start(mirror_config(&url)).await;
    let mut owner = app.setup_owner().await;

    app.multipart(
        &mut owner,
        "POST",
        "/api/receipts",
        &[("file", Some("scan.pdf"), b"%PDF-1.4 receipt")],
    )
    .await;
    assert_eq!(mirror::copy_due(&app.app).await.unwrap(), 1);
    assert!(dav.folders.lock().unwrap().contains(""));
    assert_eq!(dav.files.lock().unwrap().len(), 1);
}
