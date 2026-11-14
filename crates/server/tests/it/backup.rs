use crate::common;

use std::path::Path;

use axum::http::StatusCode;
use common::{close_years, s3, start};
use compact_company::{App, backup, config::Config, files::ContentHash};
use serde_json::json;

fn config(endpoint: &str, recipient: &str, data_dir: &Path, keep: usize) -> Config {
    let mut config: Config = toml::from_str(&format!(
        "data_dir = {data_dir:?}\n[backup]\nprefix = \"cc\"\nrecipient = \"{recipient}\"\n\
         keep_daily = {keep}\nkeep_monthly = {keep}\n\
         [backup.s3]\nbucket = \"backups\"\nregion = \"us-east-1\"\nendpoint = \"{endpoint}\"\n\
         access_key_id = \"test-key\"\nsecret_access_key_file = \"/dev/null\"\n"
    ))
    .unwrap();
    config.backup.as_mut().unwrap().s3.secret_access_key = "test-secret".into();
    config
}

fn bucket_file(root: &Path, hash: ContentHash) -> std::path::PathBuf {
    root.join(format!("backups/cc/files/{hash}"))
}

#[tokio::test]
async fn a_backup_restores_onto_a_fresh_instance_with_matching_checksums() {
    let (endpoint, bucket) = s3("backups").await;
    let identity = age::x25519::Identity::generate();
    let keys = tempfile::tempdir().unwrap();
    let identity_file = keys.path().join("identity.txt");
    std::fs::write(
        &identity_file,
        format!(
            "# created: today\n{}\n",
            age::secrecy::ExposeSecret::expose_secret(&identity.to_string())
        ),
    )
    .unwrap();
    let recipient = identity.to_public().to_string();
    let app = start(config(&endpoint, &recipient, Path::new("unused"), 1)).await;
    let mut owner = app.setup_owner().await;
    let response = app
        .multipart(
            &mut owner,
            "POST",
            "/api/receipts",
            &[("file", Some("scan.pdf"), b"%PDF-1.4 a receipt to back up")],
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let last = jiff::Zoned::now().year() - 1;
    close_years(&app, &mut owner, last, last).await;

    let first = backup::run(&app.app).await.unwrap();
    let files = app.app.files.hashes().await.unwrap().len();
    assert_eq!(first.uploaded, Some(i64::try_from(files).unwrap()));
    let receipt = ContentHash::of(b"%PDF-1.4 a receipt to back up");
    let stored = std::fs::read(bucket_file(bucket.path(), receipt)).unwrap();
    assert!(
        !stored.windows(7).any(|w| w == b"receipt"),
        "files are encrypted"
    );
    let second = backup::run(&app.app).await.unwrap();
    assert_eq!(second.uploaded, Some(0), "each file is uploaded once");
    let status = app.ok(&mut owner, "GET", "/api/backup", json!({})).await;
    assert_eq!(
        status["last_success"]["snapshot"],
        second.snapshot.as_deref().unwrap()
    );
    assert_eq!(status["failure"], serde_json::Value::Null);

    let fresh = tempfile::tempdir().unwrap();
    let target = config(&endpoint, &recipient, &fresh.path().join("data"), 1);
    let snapshots = backup::list_snapshots(&target).await.unwrap();
    assert_eq!(
        snapshots,
        [second.snapshot.clone().unwrap()],
        "rotation kept the newest"
    );
    let wrong = keys.path().join("wrong.txt");
    std::fs::write(
        &wrong,
        age::secrecy::ExposeSecret::expose_secret(&age::x25519::Identity::generate().to_string()),
    )
    .unwrap();
    assert!(
        backup::restore(&target, &wrong, &snapshots[0])
            .await
            .is_err(),
        "only the recipient's identity decrypts"
    );
    let report = backup::restore(&target, &identity_file, &snapshots[0])
        .await
        .unwrap();
    assert!(report.contains("1 year archives match"), "{report}");
    let restored = App::open(&target).await.unwrap();
    let archive = |db| async move {
        sqlx::query_scalar::<_, String>("SELECT archive FROM closed_year")
            .fetch_one(db)
            .await
            .unwrap()
    };
    assert_eq!(archive(&restored.db).await, archive(&app.app.db).await);
    assert_eq!(
        restored.files.get(receipt).await.unwrap(),
        &b"%PDF-1.4 a receipt to back up"[..]
    );
    assert!(
        backup::restore(&target, &identity_file, &snapshots[0])
            .await
            .unwrap_err()
            .to_string()
            .contains("empty data directory"),
        "a restore never overwrites"
    );

    app.app.files.delete(receipt).await.unwrap();
    backup::run(&app.app).await.unwrap();
    assert!(
        !bucket_file(bucket.path(), receipt).exists(),
        "a file no kept snapshot lists leaves the bucket"
    );
}
