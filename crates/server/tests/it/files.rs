use std::fs;

use axum::body::Bytes;
use compact_company::{
    config::Config,
    files::{ContentHash, Error, Files},
};

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
    let (endpoint, root) = crate::common::s3("files").await;

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

#[tokio::test]
async fn lists_and_deletes_stored_files() {
    let dir = tempfile::tempdir().unwrap();
    let files = Files::local(dir.path()).unwrap();
    let a = files.put(Bytes::from_static(b"a")).await.unwrap();
    let b = files.put(Bytes::from_static(b"b")).await.unwrap();
    let mut listed = files.hashes().await.unwrap();
    listed.sort_by_key(ToString::to_string);
    let mut expected = vec![a, b];
    expected.sort_by_key(ToString::to_string);
    assert_eq!(listed, expected);
    files.delete(a).await.unwrap();
    files.delete(a).await.unwrap();
    assert_eq!(files.hashes().await.unwrap(), [b]);
}
