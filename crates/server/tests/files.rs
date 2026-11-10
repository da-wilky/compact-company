use std::fs;

use axum::{body::Bytes, error_handling::HandleError, http::StatusCode};
use compact_company::{
    config::Config,
    files::{ContentHash, Error, Files},
};
use s3s::{auth::SimpleAuth, service::S3ServiceBuilder};

async fn round_trip(files: &Files) {
    let bytes = Bytes::from_static(b"%PDF-1.7 invoice");
    let hash = files.put(bytes.clone()).await.unwrap();
    assert_eq!(hash, ContentHash::of(&bytes));
    assert_eq!(files.put(bytes.clone()).await.unwrap(), hash);
    assert_eq!(files.get(hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn local_disk_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    round_trip(&Files::local(dir.path()).unwrap()).await;
}

#[tokio::test]
async fn local_disk_detects_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let files = Files::local(dir.path()).unwrap();
    let hash = files.put(Bytes::from_static(b"original")).await.unwrap();
    let hex = hash.to_string();
    fs::write(
        dir.path()
            .join(format!("sha256/{}/{}", &hex[..2], &hex[2..])),
        "tampered",
    )
    .unwrap();
    assert!(matches!(files.get(hash).await, Err(Error::Corrupt(_))));
}

#[tokio::test]
async fn s3_round_trip() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("files")).unwrap();
    let mut s3 = S3ServiceBuilder::new(s3s_fs::FileSystem::new(root.path()).unwrap());
    s3.set_auth(SimpleAuth::from_single("test-key", "test-secret"));
    let s3 = HandleError::new(s3.build(), |_| async { StatusCode::INTERNAL_SERVER_ERROR });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().fallback_service(s3))
            .await
            .unwrap()
    });

    fs::write(root.path().join("secret"), "test-secret").unwrap();
    let config = root.path().join("config.toml");
    fs::write(
        &config,
        format!(
            "[storage.s3]\nbucket = \"files\"\nregion = \"us-east-1\"\nendpoint = \"{endpoint}\"\n\
             access_key_id = \"test-key\"\nsecret_access_key_file = {:?}",
            root.path().join("secret")
        ),
    )
    .unwrap();
    let config = Config::load(&config).unwrap();
    round_trip(&Files::s3(config.storage.s3.as_ref().unwrap()).unwrap()).await;
}
