use std::sync::Arc;

use axum::{extract::State, http::StatusCode};

use super::error::{ApiError, ErrorCode, Json};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly},
    backup, mirror,
};

#[utoipa::path(get, path = "/mirror", tag = "storage",
    responses((status = OK, body = mirror::MirrorStatus), (status = "default", body = ApiError)))]
pub async fn get_mirror(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<mirror::MirrorStatus>, ApiError> {
    Ok(Json(mirror::status(&app).await?))
}

/// Copies every stored document to the mirror again, replacing whatever was changed there.
#[utoipa::path(post, path = "/mirror/resync", tag = "storage",
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn resync_mirror(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
) -> Result<StatusCode, ApiError> {
    if app.mirror.is_none() {
        return Err(ApiError::new(ErrorCode::MirrorNotConfigured));
    }
    let queued = mirror::resync(&app.db).await?;
    let mut db = app.db.acquire().await?;
    audit::record(
        &mut db,
        auth.user.id,
        "mirror.resync",
        "mirror",
        None,
        Some(serde_json::json!({ "queued": queued })),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/backup", tag = "storage",
    responses((status = OK, body = backup::BackupStatus), (status = "default", body = ApiError)))]
pub async fn get_backup(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<backup::BackupStatus>, ApiError> {
    Ok(Json(backup::status(&app).await?))
}

/// Starts a backup now; its outcome shows in the backup status.
#[utoipa::path(post, path = "/backup/run", tag = "storage",
    responses((status = ACCEPTED), (status = "default", body = ApiError)))]
pub async fn run_backup(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<StatusCode, ApiError> {
    if app.backup.is_none() {
        return Err(ApiError::new(ErrorCode::BackupNotConfigured));
    }
    tokio::spawn(async move {
        if let Err(e) = backup::run(&app).await {
            eprintln!("backup failed: {e}");
        }
    });
    Ok(StatusCode::ACCEPTED)
}
