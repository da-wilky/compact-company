use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use axum::body::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, path::Path as StorePath, prefix::PrefixStore};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{
    App,
    auth::now,
    config::{self, Config},
    files::{self, ContentHash, Files},
};

/// Encrypted snapshots in an S3 bucket: `snapshots/<name>/database.age` and its list of files,
/// `snapshots/<name>/files`, written last; every stored file once under `files/<hash>`.
pub struct Backup {
    store: Arc<dyn ObjectStore>,
    recipient: age::x25519::Recipient,
    time: jiff::civil::Time,
    keep_daily: usize,
    keep_monthly: usize,
    running: tokio::sync::Mutex<()>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Store(#[from] object_store::Error),
    #[error(transparent)]
    Files(#[from] files::Error),
    #[error("invalid age recipient: {0}")]
    Recipient(&'static str),
    #[error("cannot read the age identity {0}: {1}")]
    Identity(PathBuf, String),
    #[error("cannot encrypt: {0}")]
    Encrypt(#[from] age::EncryptError),
    #[error("cannot decrypt, is it the identity of the configured recipient? {0}")]
    Decrypt(#[from] age::DecryptError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("a backup is running already")]
    Running,
    #[error("{0}")]
    Restore(String),
}

fn store(config: &config::Backup) -> Result<Arc<dyn ObjectStore>, Error> {
    let bucket = files::s3(&config.s3)?;
    Ok(if config.prefix.is_empty() {
        bucket
    } else {
        Arc::new(PrefixStore::new(bucket, config.prefix.as_str()))
    })
}

impl Backup {
    pub fn new(config: &config::Backup) -> Result<Self, Error> {
        Ok(Self {
            store: store(config)?,
            recipient: age::x25519::Recipient::from_str(&config.recipient)
                .map_err(Error::Recipient)?,
            time: config.time,
            keep_daily: config.keep_daily,
            keep_monthly: config.keep_monthly,
            running: tokio::sync::Mutex::new(()),
        })
    }
}

fn database(snapshot: &str) -> StorePath {
    StorePath::from(format!("snapshots/{snapshot}/database.age"))
}

fn manifest(snapshot: &str) -> StorePath {
    StorePath::from(format!("snapshots/{snapshot}/files"))
}

fn file(hash: &str) -> StorePath {
    StorePath::from(format!("files/{hash}"))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct BackupRun {
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub snapshot: Option<String>,
    pub uploaded: Option<i64>,
    pub error: Option<String>,
}

/// Takes a snapshot now, then drops what rotation no longer keeps.
pub async fn run(app: &App) -> Result<BackupRun, Error> {
    let Some(backup) = &app.backup else {
        return Err(Error::Restore("no backup configured".into()));
    };
    let _running = backup.running.try_lock().map_err(|_| Error::Running)?;
    let started_at = now();
    let id: i64 = sqlx::query_scalar("INSERT INTO backup_run (started_at) VALUES (?) RETURNING id")
        .bind(started_at)
        .fetch_one(&app.db)
        .await?;
    let result = snapshot(app, backup).await;
    let (snapshot, uploaded, error) = match &result {
        Ok((snapshot, uploaded)) => (Some(snapshot.clone()), Some(*uploaded), None),
        Err(e) => (None, None, Some(e.to_string())),
    };
    let run: BackupRun = sqlx::query_as(
        "UPDATE backup_run SET finished_at = ?, snapshot = ?, uploaded = ?, error = ? WHERE id = ?
         RETURNING started_at, finished_at, snapshot, uploaded, error",
    )
    .bind(now())
    .bind(snapshot)
    .bind(uploaded)
    .bind(&error)
    .bind(id)
    .fetch_one(&app.db)
    .await?;
    if let Some(error) = error {
        notify_failure(app, &error).await;
    }
    result.map(|_| run)
}

async fn notify_failure(app: &App, error: &str) {
    let Ok(business) = app.settings.business().await else {
        return;
    };
    let (Some(mailer), Some(to)) = (&app.mailer, &business.email) else {
        return;
    };
    let body = format!(
        "Guten Tag,\n\ndie Sicherung ist fehlgeschlagen:\n\n{error}\n\n{}backup\n\nCompact Company\n",
        app.public_url
    );
    if let Err(e) = mailer.notify(to, "Sicherung fehlgeschlagen", &body).await {
        eprintln!("backup failure not mailed: {e}");
    }
}

async fn snapshot(app: &App, backup: &Backup) -> Result<(String, i64), Error> {
    let name = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%S%.3fZ")
        .to_string();
    // VACUUM INTO gives a consistent copy while the app keeps writing
    let copy = app.data_dir.join(format!("backup-{name}.db"));
    sqlx::query("VACUUM INTO ?")
        .bind(copy.to_string_lossy().as_ref())
        .execute(&app.db)
        .await?;
    let bytes = std::fs::read(&copy);
    std::fs::remove_file(&copy)?;
    let encrypted = age::encrypt(&backup.recipient, &bytes?)?;
    backup
        .store
        .put(&database(&name), Bytes::from(encrypted).into())
        .await?;

    let held: BTreeSet<String> = list(&backup.store, "files").await?.into_iter().collect();
    let hashes = app.files.hashes().await?;
    let mut uploaded = 0;
    for hash in &hashes {
        let hex = hash.to_string();
        if held.contains(&hex) {
            continue;
        }
        let bytes = app.files.get(*hash).await?;
        let encrypted = age::encrypt(&backup.recipient, &bytes)?;
        backup
            .store
            .put(&file(&hex), Bytes::from(encrypted).into())
            .await?;
        uploaded += 1;
    }
    let list: String = hashes.iter().map(|h| format!("{h}\n")).collect();
    backup
        .store
        .put(&manifest(&name), Bytes::from(list).into())
        .await?;
    rotate(backup).await?;
    Ok((name, uploaded))
}

/// The last path segment of every object directly under `prefix`.
async fn list(store: &Arc<dyn ObjectStore>, prefix: &str) -> Result<Vec<String>, Error> {
    Ok(store
        .list_with_delimiter(Some(&StorePath::from(prefix)))
        .await?
        .objects
        .into_iter()
        .filter_map(|o| o.location.filename().map(str::to_owned))
        .collect())
}

/// Snapshot names, oldest first, and whether each is complete.
async fn snapshots(store: &Arc<dyn ObjectStore>) -> Result<Vec<(String, bool)>, Error> {
    let dirs = store
        .list_with_delimiter(Some(&StorePath::from("snapshots")))
        .await?
        .common_prefixes;
    let mut snapshots = Vec::new();
    for dir in dirs {
        let Some(name) = dir.filename().map(str::to_owned) else {
            continue;
        };
        let complete = list(store, &format!("snapshots/{name}"))
            .await?
            .iter()
            .any(|f| f == "files");
        snapshots.push((name, complete));
    }
    snapshots.sort();
    Ok(snapshots)
}

async fn hashes_of(store: &Arc<dyn ObjectStore>, snapshot: &str) -> Result<Vec<String>, Error> {
    let bytes = store.get(&manifest(snapshot)).await?.bytes().await?;
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .map(str::to_owned)
        .collect())
}

/// Keeps the newest snapshot of each of the last `daily` days and `monthly` months; deletes the
/// others, unfinished ones, and every file no kept snapshot lists.
async fn rotate(backup: &Backup) -> Result<(), Error> {
    let all = snapshots(&backup.store).await?;
    let complete: Vec<String> = all.iter().filter(|s| s.1).map(|s| s.0.clone()).collect();
    let kept = keep(&complete, backup.keep_daily, backup.keep_monthly);
    for (name, _) in all.iter().filter(|s| !kept.contains(&s.0)) {
        for path in [manifest(name), database(name)] {
            match backup.store.delete(&path).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    let mut needed = BTreeSet::new();
    for name in &kept {
        needed.extend(hashes_of(&backup.store, name).await?);
    }
    for hash in list(&backup.store, "files").await? {
        if !needed.contains(&hash) {
            backup.store.delete(&file(&hash)).await?;
        }
    }
    Ok(())
}

/// Snapshot names are UTC timestamps such as `20261114T030000.000Z`, oldest first.
fn keep(snapshots: &[String], daily: usize, monthly: usize) -> BTreeSet<String> {
    let mut kept = BTreeSet::new();
    let (mut days, mut months) = (BTreeSet::new(), BTreeSet::new());
    for name in snapshots.iter().rev() {
        if days.len() < daily && days.insert(&name[..8]) {
            kept.insert(name.clone());
        }
        if months.len() < monthly && months.insert(&name[..6]) {
            kept.insert(name.clone());
        }
    }
    kept.extend(snapshots.last().cloned());
    kept
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BackupStatus {
    /// Whether the config sets up backups.
    configured: bool,
    /// Daily, as HH:MM in the business's timezone.
    time: Option<String>,
    keep_daily: Option<usize>,
    keep_monthly: Option<usize>,
    running: bool,
    last_success: Option<BackupRun>,
    /// The latest run, when it failed after the last success.
    failure: Option<BackupRun>,
}

pub async fn status(app: &App) -> Result<BackupStatus, sqlx::Error> {
    let last_success: Option<BackupRun> = sqlx::query_as(
        "SELECT started_at, finished_at, snapshot, uploaded, error FROM backup_run
         WHERE error IS NULL AND finished_at IS NOT NULL ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&app.db)
    .await?;
    Ok(BackupStatus {
        configured: app.backup.is_some(),
        time: app
            .backup
            .as_ref()
            .map(|b| b.time.strftime("%H:%M").to_string()),
        keep_daily: app.backup.as_ref().map(|b| b.keep_daily),
        keep_monthly: app.backup.as_ref().map(|b| b.keep_monthly),
        running: app
            .backup
            .as_ref()
            .is_some_and(|b| b.running.try_lock().is_err()),
        failure: failure(app).await?,
        last_success,
    })
}

/// The latest run, when it failed after the last success; shown on the overview.
pub async fn failure(app: &App) -> Result<Option<BackupRun>, sqlx::Error> {
    sqlx::query_as(
        "SELECT started_at, finished_at, snapshot, uploaded, error FROM backup_run
         WHERE error IS NOT NULL AND id > coalesce((SELECT max(id) FROM backup_run
           WHERE error IS NULL AND finished_at IS NOT NULL), 0)
         ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&app.db)
    .await
}

/// Backs up once a day at the configured time; a failed run is tried again after an hour.
pub async fn schedule(app: Arc<App>) {
    let Some(backup) = &app.backup else {
        return;
    };
    loop {
        match due(&app, backup).await {
            Ok(true) => {
                if let Err(e) = run(&app).await {
                    eprintln!("backup failed: {e}");
                }
            }
            Ok(false) => {}
            Err(e) => eprintln!("backup schedule failed: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

async fn due(app: &App, backup: &Backup) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let business = app.settings.business().await?;
    let tz = jiff::tz::TimeZone::get(business.timezone.as_deref().unwrap_or("Europe/Berlin"))?;
    let now = jiff::Timestamp::now().to_zoned(tz.clone());
    let at = now.date().to_datetime(backup.time).to_zoned(tz)?;
    if now < at {
        return Ok(false);
    }
    let (done, tried): (bool, bool) = sqlx::query_as(
        "SELECT
           count(*) FILTER (WHERE error IS NULL AND finished_at IS NOT NULL) > 0,
           count(*) FILTER (WHERE started_at > ?) > 0
         FROM backup_run WHERE started_at >= ?",
    )
    .bind(now.timestamp().as_second() - 3600)
    .bind(at.timestamp().as_second())
    .fetch_one(&app.db)
    .await?;
    Ok(!done && !tried)
}

fn identity(path: &Path) -> Result<age::x25519::Identity, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Identity(path.to_owned(), e.to_string()))?;
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| Error::Identity(path.to_owned(), "no key in it".into()))?
        .parse()
        .map_err(|e: &str| Error::Identity(path.to_owned(), e.to_owned()))
}

fn decrypt(identity: &age::x25519::Identity, bytes: &[u8]) -> Result<Vec<u8>, Error> {
    Ok(age::decrypt(identity, bytes)?)
}

/// Snapshot names in the configured bucket, oldest first.
pub async fn list_snapshots(config: &Config) -> Result<Vec<String>, Error> {
    let config = config
        .backup
        .as_ref()
        .ok_or_else(|| Error::Restore("the config has no [backup] section".into()))?;
    Ok(snapshots(&store(config)?)
        .await?
        .into_iter()
        .filter(|s| s.1)
        .map(|s| s.0)
        .collect())
}

/// Restores a snapshot into the configured data directory and file store, which must be empty,
/// then checks every file and year archive against its checksum.
pub async fn restore(
    config: &Config,
    identity_file: &Path,
    snapshot: &str,
) -> Result<String, Error> {
    let backup = config
        .backup
        .as_ref()
        .ok_or_else(|| Error::Restore("the config has no [backup] section".into()))?;
    let identity = identity(identity_file)?;
    let bucket = store(backup)?;
    let db_path = config.data_dir.join(crate::DATABASE);
    if db_path.exists() {
        return Err(Error::Restore(format!(
            "{} exists; restore only into an empty data directory",
            db_path.display()
        )));
    }
    let files = match &config.storage.s3 {
        Some(s3) => Files::s3(s3)?,
        None => Files::local(&config.data_dir.join("files"))?,
    };
    if !files.hashes().await?.is_empty() {
        return Err(Error::Restore("the file store is not empty".into()));
    }
    let hashes = hashes_of(&bucket, snapshot).await?;
    let database = bucket.get(&database(snapshot)).await?.bytes().await?;
    let database = decrypt(&identity, &database)?;
    for hex in &hashes {
        let hash: ContentHash = hex
            .parse()
            .map_err(|_| Error::Restore(format!("invalid hash {hex} in the snapshot")))?;
        let bytes = bucket.get(&file(hex)).await?.bytes().await?;
        let bytes = decrypt(&identity, &bytes)?;
        if ContentHash::of(&bytes) != hash {
            return Err(Error::Restore(format!(
                "file {hex} does not match its hash"
            )));
        }
        files.put(bytes.into()).await?;
    }
    std::fs::create_dir_all(&config.data_dir)?;
    std::fs::write(&db_path, database)?;

    let app = App::open(config)
        .await
        .map_err(|e| Error::Restore(e.to_string()))?;
    for hash in files.hashes().await? {
        files.get(hash).await?;
    }
    let archives: Vec<(i64, String)> = sqlx::query_as(
        "SELECT year, archive FROM year_archive WHERE archive IS NOT NULL ORDER BY year",
    )
    .fetch_all(&app.db)
    .await?;
    for (year, archive) in &archives {
        let hash: ContentHash = archive
            .parse()
            .map_err(|_| Error::Restore(format!("invalid archive hash of {year}")))?;
        let bytes = files.get(hash).await?;
        crate::api::years::verify_archive(&bytes)
            .map_err(|e| Error::Restore(format!("year archive {year}: {e}")))?;
    }
    Ok(format!(
        "restored {snapshot}: {} files and {} year archives match their checksums",
        hashes.len(),
        archives.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_keeps_the_newest_of_each_day_and_month() {
        let names: Vec<String> = [
            "20260115T030000Z",
            "20260131T030000Z",
            "20260201T030000Z",
            "20260227T030000Z",
            "20260228T030000Z",
            "20260228T120000Z",
            "20260301T030000Z",
        ]
        .map(str::to_owned)
        .to_vec();
        let kept: Vec<_> = keep(&names, 2, 2).into_iter().collect();
        assert_eq!(
            kept,
            ["20260228T120000Z", "20260301T030000Z"],
            "the newest of the last two days are also the newest of the last two months"
        );
        let kept: Vec<_> = keep(&names, 1, 3).into_iter().collect();
        assert_eq!(
            kept,
            ["20260131T030000Z", "20260228T120000Z", "20260301T030000Z"]
        );
        assert_eq!(keep(&names, 0, 0).len(), 1, "the newest always stays");
    }
}
