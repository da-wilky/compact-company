use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

use url::Url;
use webauthn_rs::prelude::{Webauthn, WebauthnBuilder, WebauthnError};

use crate::{config::Config, files::Files, settings::Settings};

pub mod api;
pub mod audit;
pub mod auth;
pub mod books;
pub mod config;
pub mod documents;
pub mod einvoice;
pub mod files;
pub mod layouts;
pub mod limit;
pub mod mail;
pub mod money;
pub mod oidc;
pub mod rates;
pub mod render;
pub mod settings;

pub struct App {
    pub db: SqlitePool,
    pub files: Files,
    pub settings: Settings,
    pub webauthn: Webauthn,
    pub public_url: Url,
    /// Required to create the first owner, so that only whoever can read the server log can.
    pub setup_token: String,
    pub oidc: Option<oidc::Provider>,
    pub limiter: limit::Limiter,
    pub typst_packages: Url,
    pub mailer: Option<mail::Mailer>,
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
    #[error(transparent)]
    Oidc(#[from] oidc::Error),
    #[error(transparent)]
    Mail(#[from] mail::Error),
    #[error("invalid public_url {0}: {1}")]
    PublicUrl(Url, WebauthnError),
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
        let url = &config.public_url;
        let rp_id = url.host_str().unwrap_or_default();
        let webauthn = WebauthnBuilder::new(rp_id, url)
            .map(|b| {
                b.rp_name("compact-company")
                    .allow_any_port(rp_id == "localhost")
            })
            .and_then(WebauthnBuilder::build)
            .map_err(|e| Error::PublicUrl(url.clone(), e))?;
        Ok(Self {
            db,
            files,
            settings,
            webauthn,
            public_url: url.clone(),
            setup_token: auth::random_token(),
            oidc: config
                .oidc
                .as_ref()
                .map(|oidc| oidc::Provider::new(oidc, url))
                .transpose()?,
            limiter: limit::Limiter::new(config.trusted_proxies.clone()),
            typst_packages: config.typst_packages.clone(),
            mailer: config.smtp.as_ref().map(mail::Mailer::new).transpose()?,
        })
    }

    pub async fn setup_required(&self) -> Result<bool, sqlx::Error> {
        let users: i64 = sqlx::query_scalar("SELECT count(*) FROM user")
            .fetch_one(&self.db)
            .await?;
        Ok(users == 0)
    }
}

#[derive(Embed)]
#[folder = "../../web/build"]
struct Client;

const CLIENT_FALLBACK: &str = "200.html";

pub fn router(app: Arc<App>) -> Router {
    let (api, _) = api::routes().split_for_parts();
    api.with_state(app)
        .fallback(client)
        .layer(middleware::from_fn(security_headers))
}

/// The client's own CSP sits in a meta tag, which cannot forbid framing.
async fn security_headers(request: Request, next: Next) -> Response {
    let cache = match request.uri().path() {
        path if path.starts_with("/api/") => "no-store",
        path if path.starts_with("/_app/immutable/") => "public, max-age=31536000, immutable",
        _ => "no-cache",
    };
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    response
}

pub fn openapi() -> utoipa::openapi::OpenApi {
    api::routes().into_openapi()
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
