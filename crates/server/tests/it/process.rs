use crate::common;

use axum::http::StatusCode;
use common::start;
use compact_company::{App, api::years, config::Config, process};
use http_body_util::BodyExt;
use serde_json::json;

fn config(time: &str) -> Config {
    let mut config: Config = toml::from_str(&format!(
        "[backup]\nrecipient = \"age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p\"\n\
         time = \"{time}\"\n[backup.s3]\nbucket = \"backups\"\nregion = \"eu-central-1\"\n\
         access_key_id = \"key\"\nsecret_access_key_file = \"/dev/null\"\n"
    ))
    .unwrap();
    config.backup.as_mut().unwrap().s3.secret_access_key = "secret".into();
    config
}

#[tokio::test]
async fn answers_and_config_changes_make_versions_and_closing_archives_them() {
    let app = start(config("03:00")).await;
    let mut owner = app.setup_owner().await;
    app.ready_to_issue(&mut owner).await;
    assert_eq!(process::refresh(&app.app).await.unwrap(), Some(1));
    assert_eq!(
        process::refresh(&app.app).await.unwrap(),
        None,
        "nothing changed"
    );

    let view = app
        .ok(
            &mut owner,
            "PUT",
            "/api/process-documentation/answers",
            json!({ "answers": { "activity": "Softwareentwicklung für Geschäftskunden" } }),
        )
        .await;
    assert_eq!(view["versions"][0]["id"], 2);
    assert_eq!(
        view["versions"][0]["changes"],
        json!(["Was tut das Unternehmen?"])
    );
    assert!(view["versions"][1]["valid_until"].is_string());
    let question = view["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["key"] == "capture")
        .unwrap();
    assert!(question["help"].as_str().unwrap().contains("Rz. 136"));
    assert!(!question["suggestion"].as_str().unwrap().is_empty());

    let mut accountant = app.with_role(&mut owner, "accountant").await;
    for language in ["de", "en"] {
        let pdf = app
            .as_browser(
                &mut accountant,
                "GET",
                &format!("/api/process-documentation/versions/2/pdf?language={language}"),
                json!({}),
            )
            .await;
        assert_eq!(pdf.status(), StatusCode::OK);
        let bytes = pdf.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.starts_with(b"%PDF"));
    }

    let last = jiff::Zoned::now().year() - 1;
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": format!("{last}-01-01") }),
    )
    .await;
    let listing = app.ok(&mut owner, "GET", "/api/years", json!({})).await;
    let review = listing["review_answers"].as_array().unwrap();
    assert!(review.contains(&json!("capture")));
    assert!(!review.contains(&json!("activity")), "answered just now");
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/years/{last}/close"),
        json!({}),
    )
    .await;
    let archive = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/years/{last}/archive"),
            json!({}),
        )
        .await
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let paths: Vec<String> = years::unpack(&archive)
        .unwrap()
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    // starting the books changed the chart of accounts, so closing made version 3
    for path in [
        "process-documentation/v3.json",
        "process-documentation/v3-de.pdf",
        "process-documentation/v3-en.pdf",
    ] {
        assert!(paths.contains(&path.to_owned()), "{paths:?}");
    }

    let restarted = App::open(&Config {
        data_dir: app.dir.path().join("data"),
        ..config("04:30")
    })
    .await
    .unwrap();
    assert_eq!(process::refresh(&restarted).await.unwrap(), Some(4));
    let changes: String = sqlx::query_scalar("SELECT changes FROM process_version WHERE id = 4")
        .fetch_one(&restarted.db)
        .await
        .unwrap();
    assert_eq!(changes, r#"["backup.time"]"#);
    assert_eq!(
        process::label("backup.time", compact_company::api::customers::Language::De),
        "Sicherung: Uhrzeit"
    );
}
