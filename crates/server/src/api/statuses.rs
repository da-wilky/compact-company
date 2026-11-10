use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use super::{
    error::{ApiError, ErrorCode, Json},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrStaff},
};

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Status {
    id: i64,
    name: String,
    /// Todos in this status count as done.
    done: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusInput {
    /// Left out for a new status.
    id: Option<i64>,
    name: Option<String>,
    #[serde(default)]
    done: bool,
}

#[utoipa::path(get, path = "/statuses", tag = "todos",
    responses((status = OK, body = Vec<Status>), (status = "default", body = ApiError)))]
pub async fn list_statuses(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrStaff>,
) -> Result<Json<Vec<Status>>, ApiError> {
    Ok(Json(list(&mut *app.db.acquire().await?).await?))
}

/// Replaces the whole list in the given order; a status left out is removed, which only works
/// while no todo is in it.
#[utoipa::path(put, path = "/statuses", tag = "todos", request_body = Vec<StatusInput>,
    responses((status = OK, body = Vec<Status>), (status = "default", body = ApiError)))]
pub async fn put_statuses(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<Vec<StatusInput>>,
) -> Result<Json<Vec<Status>>, ApiError> {
    let mut check = Check::default();
    let statuses: Vec<(Option<i64>, String, bool)> = input
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            (
                s.id,
                check.required(&format!("{i}.name"), s.name, 100),
                s.done,
            )
        })
        .collect();
    check.done()?;
    if statuses.is_empty() {
        return Err(ApiError::new(ErrorCode::MalformedRequest));
    }
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = list(&mut tx).await?;
    let kept: Vec<i64> = statuses.iter().filter_map(|(id, _, _)| *id).collect();
    if kept.iter().collect::<std::collections::HashSet<_>>().len() < kept.len() {
        return Err(ApiError::new(ErrorCode::MalformedRequest));
    }
    if kept.iter().any(|id| !before.iter().any(|s| s.id == *id)) {
        return Err(ApiError::new(ErrorCode::NotFound));
    }
    for gone in before.iter().filter(|s| !kept.contains(&s.id)) {
        let used: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM todo WHERE status_id = ?)")
                .bind(gone.id)
                .fetch_one(&mut *tx)
                .await?;
        if used {
            return Err(ApiError::new(ErrorCode::StatusInUse));
        }
        sqlx::query("DELETE FROM todo_status WHERE id = ?")
            .bind(gone.id)
            .execute(&mut *tx)
            .await?;
    }
    for (position, (id, name, done)) in statuses.iter().enumerate() {
        let position = position as i64 + 1;
        match id {
            Some(id) => {
                sqlx::query("UPDATE todo_status SET name = ?, done = ?, position = ? WHERE id = ?")
                    .bind(name)
                    .bind(done)
                    .bind(position)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?
            }
            None => {
                sqlx::query("INSERT INTO todo_status (name, done, position) VALUES (?, ?, ?)")
                    .bind(name)
                    .bind(done)
                    .bind(position)
                    .execute(&mut *tx)
                    .await?
            }
        };
    }
    let after = list(&mut tx).await?;
    if serde_json::to_value(&before).ok() != serde_json::to_value(&after).ok() {
        audit::record(
            &mut tx,
            auth.user.id,
            "statuses.update",
            "statuses",
            Some(json!(before)),
            Some(json!(after)),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(after))
}

pub async fn list(db: &mut SqliteConnection) -> Result<Vec<Status>, ApiError> {
    Ok(
        sqlx::query_as("SELECT id, name, done FROM todo_status ORDER BY position")
            .fetch_all(db)
            .await?,
    )
}
