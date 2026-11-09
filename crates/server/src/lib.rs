use std::time::Duration;

use axum::{
    Json, Router,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::Embed;
use serde_json::json;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

use crate::{config::Config, files::Files, settings::Settings};

pub mod config;
pub mod files;
pub mod settings;

pub struct App {
    pub db: SqlitePool,
    pub files: Files,
    pub settings: Settings,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot create {0}: {1}")]
    DataDir(String, std::io::Error),
    #[error("cannot open the database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("cannot migrate the database: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("cannot open the file store: {0}")]
    Files(#[from] files::Error),
}

impl App {
    pub async fn open(config: &Config) -> Result<Self, Error> {
        std::fs::create_dir_all(&config.data_dir)
            .map_err(|e| Error::DataDir(config.data_dir.display().to_string(), e))?;
        let db = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(config.data_dir.join("compact-company.db"))
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await?;
        sqlx::migrate!().run(&db).await?;
        let files = match &config.storage.s3 {
            Some(s3) => Files::s3(s3)?,
            None => Files::local(&config.data_dir.join("files"))?,
        };
        let settings = Settings::new(db.clone(), &config.business);
        Ok(Self {
            db,
            files,
            settings,
        })
    }
}

#[derive(Embed)]
#[folder = "../../web/build"]
struct Client;

const CLIENT_FALLBACK: &str = "200.html";

pub fn router() -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .fallback(api_not_found);
    Router::new().nest("/api", api).fallback(client)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn api_not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response()
}

async fn client(uri: Uri) -> Response {
    let file = Client::get(uri.path().trim_start_matches('/'))
        .or_else(|| Client::get(CLIENT_FALLBACK))
        .expect("the client build contains its fallback page");
    (
        [(header::CONTENT_TYPE, file.metadata.mimetype())],
        file.data,
    )
        .into_response()
}
