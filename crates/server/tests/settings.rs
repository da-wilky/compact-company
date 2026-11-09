use compact_company::{
    App,
    config::Config,
    settings::{Business, Error},
};
use serde_json::json;

async fn open(business: Business) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        data_dir: dir.path().join("data"),
        business,
        ..Config::default()
    };
    let app = App::open(&config).await.unwrap();
    (dir, app)
}

#[tokio::test]
async fn starts_on_an_empty_data_dir() {
    let (dir, app) = open(Business::default()).await;
    assert!(dir.path().join("data/compact-company.db").exists());
    assert_eq!(app.settings.business().await.unwrap(), Business::default());
}

#[tokio::test]
async fn stored_settings_are_read_back() {
    let (_dir, app) = open(Business::default()).await;
    app.settings.set("name", json!("Wilk IT")).await.unwrap();
    app.settings
        .set("small_business", json!(true))
        .await
        .unwrap();
    let business = app.settings.business().await.unwrap();
    assert_eq!(business.name.as_deref(), Some("Wilk IT"));
    assert_eq!(business.small_business, Some(true));
}

#[tokio::test]
async fn pinned_keys_win_and_cannot_be_set() {
    let pinned = Business {
        name: Some("Pinned".into()),
        ..Business::default()
    };
    let (_dir, app) = open(pinned).await;
    assert!(app.settings.is_pinned("name"));
    assert!(!app.settings.is_pinned("vat_id"));
    assert!(matches!(
        app.settings.set("name", json!("Other")).await,
        Err(Error::Pinned(_))
    ));
    app.settings
        .set("vat_id", json!("DE123456789"))
        .await
        .unwrap();
    let business = app.settings.business().await.unwrap();
    assert_eq!(business.name.as_deref(), Some("Pinned"));
    assert_eq!(business.vat_id.as_deref(), Some("DE123456789"));
}

#[tokio::test]
async fn unknown_keys_and_wrong_types_are_rejected() {
    let (_dir, app) = open(Business::default()).await;
    assert!(matches!(
        app.settings.set("colour", json!("red")).await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        app.settings.set("small_business", json!("yes")).await,
        Err(Error::Invalid(_))
    ));
}
