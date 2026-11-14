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
    let first_path = format!("{year}/Belege/{today} Beleg {first}.pdf");
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
    let second_path = format!("{year}/Belege/{today} Beleg {second}.pdf");
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
    let invoice = format!(
        "{year}/Rechnungen/{}.pdf",
        issued["number"].as_str().unwrap()
    );
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
