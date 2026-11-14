use std::{collections::BTreeMap, sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use object_store::{
    ClientOptions, HeaderMap, HeaderValue, ObjectStore, ObjectStoreExt, RetryConfig,
    http::HttpBuilder, path::Path as StorePath,
};
use serde::Serialize;
use sqlx::{SqliteConnection, SqlitePool};
use utoipa::ToSchema;

use crate::{App, auth::now, config::Webdav, documents::Kind, files::ContentHash};

/// A one-way copy of every stored document into a readable folder tree; the file store stays
/// the main storage, and whatever changes in the mirror is overwritten by the next copy.
pub struct Mirror {
    store: Arc<dyn ObjectStore>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Store(#[from] object_store::Error),
    #[error(transparent)]
    Files(#[from] crate::files::Error),
    #[error("invalid mirror credentials: {0}")]
    Credentials(#[from] axum::http::header::InvalidHeaderValue),
}

impl Mirror {
    pub fn webdav(config: &Webdav) -> Result<Self, Error> {
        let credentials = BASE64.encode(format!("{}:{}", config.username, config.password));
        let mut auth = HeaderValue::from_str(&format!("Basic {credentials}"))?;
        auth.set_sensitive(true);
        let headers = HeaderMap::from_iter([(axum::http::header::AUTHORIZATION, auth)]);
        let store = HttpBuilder::new()
            .with_url(config.url.as_str())
            .with_client_options(
                ClientOptions::new()
                    .with_default_headers(headers)
                    .with_allow_http(config.url.scheme() == "http")
                    .with_timeout(Duration::from_secs(120)),
            )
            // pending copies back off on their own, so a failing server isn't hammered
            .with_retry(RetryConfig {
                max_retries: 1,
                retry_timeout: Duration::from_secs(30),
                ..RetryConfig::default()
            })
            .build()?;
        Ok(Self {
            store: Arc::new(store),
        })
    }
}

/// Marks a record's files for copying; call it in the transaction that changes the record.
pub async fn queue(app: &App, tx: &mut SqliteConnection, subject: &str) -> Result<(), sqlx::Error> {
    if app.mirror.is_none() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO mirror_pending (subject, due_at) VALUES (?, ?)
         ON CONFLICT (subject) DO UPDATE SET due_at = excluded.due_at, attempts = 0,
         seq = seq + 1",
    )
    .bind(subject)
    .bind(now())
    .execute(tx)
    .await?;
    Ok(())
}

/// Copies everything again, overwriting what the mirror holds; the first copy of existing data.
pub async fn resync(db: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO mirror_pending (subject, due_at, force)
         SELECT subject, ?1, 1 FROM (
           SELECT 'document:' || id AS subject FROM document WHERE status = 'issued'
           UNION SELECT 'receipt:' || id FROM receipt WHERE file IS NOT NULL
           UNION SELECT 'bank_import:' || id FROM bank_import
           UNION SELECT 'year:' || year FROM closed_year
           UNION SELECT subject FROM mirror_file)
         WHERE true
         ON CONFLICT (subject) DO UPDATE SET due_at = ?1, attempts = 0, force = 1, seq = seq + 1",
    )
    .bind(now())
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Status {
    /// Whether the config sets up a mirror.
    configured: bool,
    /// Records whose files are still to be copied or deleted.
    pending: i64,
    last_copy_at: Option<i64>,
    /// The latest error of a copy still pending.
    error: Option<String>,
    failed_at: Option<i64>,
}

pub async fn status(app: &App) -> Result<Status, sqlx::Error> {
    let (pending, last_copy_at): (i64, Option<i64>) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM mirror_pending), (SELECT max(copied_at) FROM mirror_file)",
    )
    .fetch_one(&app.db)
    .await?;
    let failed: Option<(String, i64)> = sqlx::query_as(
        "SELECT error, failed_at FROM mirror_pending WHERE error IS NOT NULL
         ORDER BY failed_at DESC LIMIT 1",
    )
    .fetch_optional(&app.db)
    .await?;
    let (error, failed_at) = failed.unzip();
    Ok(Status {
        configured: app.mirror.is_some(),
        pending,
        last_copy_at,
        error,
        failed_at,
    })
}

/// Works off pending copies, checking every few seconds.
pub async fn run(app: Arc<App>) {
    loop {
        if let Err(e) = copy_due(&app).await {
            eprintln!("mirror failed: {e}");
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

/// Copies every record that is due and returns how many are done.
pub async fn copy_due(app: &App) -> Result<usize, Error> {
    let Some(mirror) = &app.mirror else {
        return Ok(0);
    };
    let due: Vec<(String, i64, i64, bool)> = sqlx::query_as(
        "SELECT subject, attempts, seq, force FROM mirror_pending WHERE due_at <= ?
         ORDER BY due_at",
    )
    .bind(now())
    .fetch_all(&app.db)
    .await?;
    let mut done = 0;
    for (subject, attempts, seq, force) in due {
        match copy(app, mirror, &subject, force).await {
            Ok(()) => {
                sqlx::query("DELETE FROM mirror_pending WHERE subject = ? AND seq = ?")
                    .bind(&subject)
                    .bind(seq)
                    .execute(&app.db)
                    .await?;
                done += 1;
            }
            Err(e) => {
                let wait = 30_i64 << attempts.min(7);
                sqlx::query(
                    "UPDATE mirror_pending SET attempts = attempts + 1, due_at = ?, error = ?,
                     failed_at = ? WHERE subject = ?",
                )
                .bind(now() + wait.min(3600))
                .bind(e.to_string())
                .bind(now())
                .bind(&subject)
                .execute(&app.db)
                .await?;
            }
        }
    }
    Ok(done)
}

async fn copy(app: &App, mirror: &Mirror, subject: &str, force: bool) -> Result<(), Error> {
    let wanted = wanted(app, subject).await?;
    let held: BTreeMap<String, String> =
        sqlx::query_as("SELECT path, hash FROM mirror_file WHERE subject = ?")
            .bind(subject)
            .fetch_all(&app.db)
            .await?
            .into_iter()
            .collect();
    for (path, hash) in &wanted {
        if !force && held.get(path) == Some(&hash.to_string()) {
            continue;
        }
        let bytes = app.files.get(*hash).await?;
        mirror.store.put(&store_path(path), bytes.into()).await?;
        sqlx::query(
            "INSERT INTO mirror_file (path, subject, hash, copied_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (path) DO UPDATE SET subject = excluded.subject, hash = excluded.hash,
             copied_at = excluded.copied_at",
        )
        .bind(path)
        .bind(subject)
        .bind(hash.to_string())
        .bind(now())
        .execute(&app.db)
        .await?;
    }
    for path in held.keys().filter(|path| !wanted.contains_key(*path)) {
        match mirror.store.delete(&store_path(path)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
            Err(e) => return Err(e.into()),
        }
        sqlx::query("DELETE FROM mirror_file WHERE path = ?")
            .bind(path)
            .execute(&app.db)
            .await?;
    }
    Ok(())
}

fn store_path(path: &str) -> StorePath {
    StorePath::from_iter(path.split('/'))
}

/// The files a record should have in the mirror, by path.
async fn wanted(app: &App, subject: &str) -> Result<BTreeMap<String, ContentHash>, Error> {
    let mut files = BTreeMap::new();
    let Some((kind, id)) = subject.split_once(':') else {
        return Ok(files);
    };
    let Ok(id) = id.parse::<i64>() else {
        return Ok(files);
    };
    let mut db = app.db.acquire().await?;
    let mut add = |path: String, hash: &str| {
        if let Ok(hash) = hash.parse() {
            files.insert(path, hash);
        }
    };
    match kind {
        "document" => {
            let row: Option<(Kind, String, String, String, Option<String>)> = sqlx::query_as(
                "SELECT kind, number, issue_date, pdf, xml FROM document
                 WHERE id = ? AND status = 'issued' AND pdf IS NOT NULL",
            )
            .bind(id)
            .fetch_optional(&mut *db)
            .await?;
            if let Some((kind, number, date, pdf, xml)) = row {
                let base = format!("{}/{}/{}", &date[..4], folder(kind), name(&number));
                add(format!("{base}.pdf"), &pdf);
                if let Some(xml) = xml {
                    add(format!("{base}.xml"), &xml);
                }
            }
        }
        "receipt" => {
            let row: Option<Receipt> = sqlx::query_as(
                "SELECT r.file, r.file_name, r.file_type, r.date, r.number, s.name AS supplier,
                 date(r.created_at, 'unixepoch') AS created
                 FROM receipt r LEFT JOIN supplier s ON s.id = r.supplier_id
                 WHERE r.id = ? AND r.file IS NOT NULL",
            )
            .bind(id)
            .fetch_optional(&mut *db)
            .await?;
            if let Some(r) = row {
                let date = r.date.unwrap_or(r.created);
                let mut parts = vec![date.clone()];
                parts.extend(r.supplier);
                parts.extend(r.number);
                if parts.len() == 1 {
                    parts.push(format!("Beleg {id}"));
                }
                let ext = extension(r.file_name.as_deref(), r.file_type.as_deref());
                add(
                    format!("{}/Belege/{}{ext}", &date[..4], name(&parts.join(" "))),
                    &r.file,
                );
            }
        }
        "bank_import" => {
            let row: Option<(String, Option<String>, String, String, String)> = sqlx::query_as(
                "SELECT i.file, i.file_name, a.name,
                 coalesce(min(t.booking_date), date(i.created_at, 'unixepoch')),
                 coalesce(max(t.booking_date), date(i.created_at, 'unixepoch'))
                 FROM bank_import i JOIN bank_account a ON a.id = i.bank_account_id
                 LEFT JOIN bank_transaction t ON t.import_id = i.id
                 WHERE i.id = ? GROUP BY i.id",
            )
            .bind(id)
            .fetch_optional(&mut *db)
            .await?;
            if let Some((file, file_name, account, from, to)) = row {
                let ext = extension(file_name.as_deref(), None);
                add(
                    format!(
                        "{}/Kontoauszüge/{}{ext}",
                        &to[..4],
                        name(&format!("{from} bis {to} {account}"))
                    ),
                    &file,
                );
            }
        }
        "year" => {
            let archive: Option<String> =
                sqlx::query_scalar("SELECT archive FROM closed_year WHERE year = ?")
                    .bind(id)
                    .fetch_optional(&mut *db)
                    .await?;
            if let Some(archive) = archive {
                add(format!("{id}/Jahresarchiv-{id}.tar.gz"), &archive);
            }
        }
        _ => {}
    }
    Ok(files)
}

#[derive(sqlx::FromRow)]
struct Receipt {
    file: String,
    file_name: Option<String>,
    file_type: Option<String>,
    date: Option<String>,
    number: Option<String>,
    supplier: Option<String>,
    created: String,
}

fn folder(kind: Kind) -> &'static str {
    match kind {
        Kind::Quote => "Angebote",
        Kind::OrderConfirmation => "Auftragsbestätigungen",
        Kind::Invoice | Kind::Cancellation => "Rechnungen",
        Kind::DeliveryNote => "Lieferscheine",
        Kind::AcceptanceReport => "Abnahmeprotokolle",
        Kind::Dunning => "Mahnungen",
    }
}

/// A file or folder name that WebDAV servers and every desktop file system accept.
fn name(value: &str) -> String {
    let name: String = value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.,()&+'".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    name.trim_matches(|c| c == ' ' || c == '.').to_owned()
}

fn extension(file_name: Option<&str>, file_type: Option<&str>) -> String {
    let from_name = file_name
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| {
            !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric())
        });
    let ext = from_name.or_else(|| {
        match file_type? {
            "application/pdf" => Some("pdf"),
            "image/jpeg" => Some("jpg"),
            "image/png" => Some("png"),
            "application/xml" | "text/xml" => Some("xml"),
            _ => None,
        }
        .map(str::to_owned)
    });
    ext.map(|ext| format!(".{ext}")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_keep_letters_and_drop_separators() {
        assert_eq!(
            name("2026-03-04 Müller & Söhne R/12"),
            "2026-03-04 Müller & Söhne R-12"
        );
        assert_eq!(name("..hidden."), "hidden");
        assert_eq!(extension(Some("scan.PDF"), None), ".pdf");
        assert_eq!(extension(Some("noext"), Some("image/png")), ".png");
        assert_eq!(extension(None, None), "");
    }
}
