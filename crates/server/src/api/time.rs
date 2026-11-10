use std::sync::Arc;

use axum::{extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOrStaff, Role, User, now},
    money::Money,
    rates,
};

/// The longest single entry; anything longer is almost always a forgotten timer or a typo.
const MAX_SECS: i64 = 24 * 3600;

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct TimeEntry {
    id: i64,
    user_id: i64,
    user_name: String,
    #[sqlx(flatten)]
    #[serde(flatten)]
    fields: EntryFields,
    project_name: String,
    customer_id: i64,
    customer_name: String,
    todo_title: Option<String>,
    activity_name: Option<String>,
    /// The hourly rate this work is worth, if any applies.
    #[sqlx(skip)]
    rate: Option<Money>,
    created_at: i64,
    updated_at: i64,
}

/// What an edit may change; also the snapshot the audit log compares.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct EntryFields {
    project_id: i64,
    todo_id: Option<i64>,
    activity_id: Option<i64>,
    description: Option<String>,
    /// Unix seconds.
    started_at: i64,
    /// Unix seconds; a running timer has none.
    ended_at: Option<i64>,
    billable: bool,
    /// Seconds charged when that is not the whole tracked duration.
    billable_secs: Option<i64>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TimeFilter {
    /// Unix seconds; entries that end after it.
    from: i64,
    /// Unix seconds; entries that start before it.
    to: i64,
    /// Whose time; your own when left out. Only the owner sees other people's.
    user_id: Option<i64>,
}

#[utoipa::path(get, path = "/time", tag = "time", params(TimeFilter),
    responses((status = OK, body = Vec<TimeEntry>), (status = "default", body = ApiError)))]
pub async fn list_time(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Query(filter): Query<TimeFilter>,
) -> Result<Json<Vec<TimeEntry>>, ApiError> {
    let user_id = filter.user_id.unwrap_or(auth.user.id);
    if user_id != auth.user.id && auth.user.role != Role::Owner {
        return Err(ApiError::new(ErrorCode::Forbidden));
    }
    let mut db = app.db.acquire().await?;
    let entries = sqlx::query_as(
        "SELECT * FROM time_entry_view WHERE user_id = ? AND started_at < ?
         AND coalesce(ended_at, ?) > ? ORDER BY started_at",
    )
    .bind(user_id)
    .bind(filter.to)
    .bind(now())
    .bind(filter.from)
    .fetch_all(&mut *db)
    .await?;
    Ok(Json(with_rates(&mut db, entries).await?))
}

/// Your running timers, newest first.
#[utoipa::path(get, path = "/timers", tag = "time",
    responses((status = OK, body = Vec<TimeEntry>), (status = "default", body = ApiError)))]
pub async fn list_timers(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
) -> Result<Json<Vec<TimeEntry>>, ApiError> {
    let mut db = app.db.acquire().await?;
    let entries = sqlx::query_as(
        "SELECT * FROM time_entry_view WHERE user_id = ? AND ended_at IS NULL
         ORDER BY started_at DESC",
    )
    .bind(auth.user.id)
    .fetch_all(&mut *db)
    .await?;
    Ok(Json(with_rates(&mut db, entries).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TimerInput {
    project_id: i64,
    todo_id: Option<i64>,
    activity_id: Option<i64>,
    description: Option<String>,
}

/// Starts a timer now; other running timers keep running.
#[utoipa::path(post, path = "/timers", tag = "time", request_body = TimerInput,
    responses((status = OK, body = TimeEntry), (status = "default", body = ApiError)))]
pub async fn start_timer(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Json(input): Json<TimerInput>,
) -> Result<Json<TimeEntry>, ApiError> {
    let input = EntryInput {
        project_id: input.project_id,
        todo_id: input.todo_id,
        activity_id: input.activity_id,
        description: input.description,
        started_at: now(),
        ended_at: None,
        billable: None,
        billable_secs: None,
        user_id: None,
    };
    create(&app, &auth.user, input, true).await.map(Json)
}

#[utoipa::path(post, path = "/time/{id}/stop", tag = "time", params(("id" = i64, Path)),
    responses((status = OK, body = TimeEntry), (status = "default", body = ApiError)))]
pub async fn stop_timer(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Path(id): Path<i64>,
) -> Result<Json<TimeEntry>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, &auth.user, id).await?;
    if before.fields.ended_at.is_none() {
        let ended = now().max(before.fields.started_at);
        sqlx::query("UPDATE time_entry SET ended_at = ?, updated_at = ? WHERE id = ?")
            .bind(ended)
            .bind(now())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        audit::record(
            &mut tx,
            auth.user.id,
            "time.stop",
            &format!("time:{id}"),
            Some(serde_json::json!({ "ended_at": null })),
            Some(serde_json::json!({ "ended_at": ended })),
        )
        .await?;
    }
    let entry = find(&mut tx, &auth.user, id).await?;
    tx.commit().await?;
    Ok(Json(entry))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryInput {
    project_id: i64,
    todo_id: Option<i64>,
    activity_id: Option<i64>,
    description: Option<String>,
    /// Unix seconds.
    started_at: i64,
    /// Unix seconds; left out keeps a timer running.
    ended_at: Option<i64>,
    /// The project's setting when left out.
    billable: Option<bool>,
    /// Seconds charged when that is not the whole tracked duration.
    billable_secs: Option<i64>,
    /// Whose time; your own when left out. Only the owner enters time for others.
    user_id: Option<i64>,
}

#[utoipa::path(post, path = "/time", tag = "time", request_body = EntryInput,
    responses((status = OK, body = TimeEntry), (status = "default", body = ApiError)))]
pub async fn create_entry(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Json(input): Json<EntryInput>,
) -> Result<Json<TimeEntry>, ApiError> {
    create(&app, &auth.user, input, false).await.map(Json)
}

#[utoipa::path(put, path = "/time/{id}", tag = "time", request_body = EntryInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = TimeEntry), (status = "default", body = ApiError)))]
pub async fn put_entry(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Path(id): Path<i64>,
    Json(input): Json<EntryInput>,
) -> Result<Json<TimeEntry>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, &auth.user, id).await?;
    if input.user_id.is_some_and(|user| user != before.user_id) {
        return Err(ApiError::new(ErrorCode::MalformedRequest));
    }
    if input.ended_at.is_none() && before.fields.ended_at.is_some() {
        let mut check = Check::default();
        check.fail("ended_at", FieldError::Required);
        check.done()?;
    }
    let fields = validate(&mut tx, input, Some(&before.fields)).await?;
    sqlx::query(
        "UPDATE time_entry SET project_id = ?, todo_id = ?, activity_id = ?, description = ?,
         started_at = ?, ended_at = ?, billable = ?, billable_secs = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(fields.project_id)
    .bind(fields.todo_id)
    .bind(fields.activity_id)
    .bind(&fields.description)
    .bind(fields.started_at)
    .bind(fields.ended_at)
    .bind(fields.billable)
    .bind(fields.billable_secs)
    .bind(now())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if let Some((old, new)) = audit::changes(&before.fields, &fields) {
        audit::record(&mut tx, auth.user.id, "time.update", &format!("time:{id}"), Some(old), Some(new))
            .await?;
    }
    let entry = find(&mut tx, &auth.user, id).await?;
    tx.commit().await?;
    Ok(Json(entry))
}

#[utoipa::path(delete, path = "/time/{id}", tag = "time", params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn delete_entry(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let entry = find(&mut tx, &auth.user, id).await?;
    sqlx::query("DELETE FROM time_entry WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "time.delete",
        &format!("time:{id}"),
        Some(serde_json::to_value(&entry.fields).map_err(ApiError::internal)?),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create(app: &App, user: &User, input: EntryInput, running: bool) -> Result<TimeEntry, ApiError> {
    let owner = match input.user_id {
        Some(other) if other != user.id => {
            if user.role != Role::Owner {
                return Err(ApiError::new(ErrorCode::Forbidden));
            }
            other
        }
        _ => user.id,
    };
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    if owner != user.id {
        let trackable: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM user WHERE id = ? AND role IN ('owner', 'staff'))",
        )
        .bind(owner)
        .fetch_one(&mut *tx)
        .await?;
        if !trackable {
            let mut check = Check::default();
            check.fail("user_id", FieldError::InvalidFormat);
            check.done()?;
        }
    }
    if !running && input.ended_at.is_none() {
        let mut check = Check::default();
        check.fail("ended_at", FieldError::Required);
        check.done()?;
    }
    let fields = validate(&mut tx, input, None).await?;
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO time_entry (user_id, project_id, todo_id, activity_id, description,
         started_at, ended_at, billable, billable_secs, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(owner)
    .bind(fields.project_id)
    .bind(fields.todo_id)
    .bind(fields.activity_id)
    .bind(&fields.description)
    .bind(fields.started_at)
    .bind(fields.ended_at)
    .bind(fields.billable)
    .bind(fields.billable_secs)
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        user.id,
        if running { "time.start" } else { "time.create" },
        &format!("time:{id}"),
        None,
        Some(serde_json::to_value(&fields).map_err(ApiError::internal)?),
    )
    .await?;
    let entry = find(&mut tx, user, id).await?;
    tx.commit().await?;
    Ok(entry)
}

/// The entry, if the user may see it: their own, or anyone's for the owner.
async fn find(db: &mut SqliteConnection, user: &User, id: i64) -> Result<TimeEntry, ApiError> {
    let entry: TimeEntry = sqlx::query_as("SELECT * FROM time_entry_view WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *db)
        .await?
        .filter(|e: &TimeEntry| e.user_id == user.id || user.role == Role::Owner)
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    Ok(with_rates(db, vec![entry]).await?.remove(0))
}

async fn with_rates(
    db: &mut SqliteConnection,
    mut entries: Vec<TimeEntry>,
) -> Result<Vec<TimeEntry>, ApiError> {
    for entry in &mut entries {
        entry.rate =
            rates::resolve(&mut *db, entry.fields.project_id, entry.fields.activity_id).await?;
    }
    Ok(entries)
}

async fn validate(
    db: &mut SqliteConnection,
    input: EntryInput,
    current: Option<&EntryFields>,
) -> Result<EntryFields, ApiError> {
    let mut check = Check::default();
    let project: Option<(bool, bool)> =
        sqlx::query_as("SELECT closed, billable FROM project_view WHERE id = ?")
            .bind(input.project_id)
            .fetch_optional(&mut *db)
            .await?;
    let billable_default = match project {
        None => {
            check.fail("project_id", FieldError::InvalidFormat);
            true
        }
        // time already on a project may be corrected after it closes; new time may not go there
        Some((true, _)) if current.is_none_or(|c| c.project_id != input.project_id) => {
            return Err(ApiError::new(ErrorCode::Archived));
        }
        Some((_, billable)) => billable,
    };
    if let Some(todo) = input.todo_id {
        let on_project: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM todo WHERE id = ? AND project_id = ?)")
                .bind(todo)
                .bind(input.project_id)
                .fetch_one(&mut *db)
                .await?;
        if !on_project {
            check.fail("todo_id", FieldError::InvalidFormat);
        }
    }
    if let Some(activity) = input.activity_id {
        let usable: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM activity WHERE id = ? AND (archived = 0 OR id = ?))",
        )
        .bind(activity)
        .bind(current.and_then(|c| c.activity_id))
        .fetch_one(&mut *db)
        .await?;
        if !usable {
            check.fail("activity_id", FieldError::InvalidFormat);
        }
    }
    if let Some(ended) = input.ended_at {
        // saturating, so absurd timestamps cannot wrap around into a valid-looking duration
        if ended < input.started_at || ended.saturating_sub(input.started_at) > MAX_SECS {
            check.fail("ended_at", FieldError::InvalidFormat);
        }
    }
    if input.started_at <= 0 || input.started_at > now() + 60 {
        check.fail("started_at", FieldError::InvalidFormat);
    }
    if let Some(secs) = input.billable_secs {
        let tracked = input.ended_at.map(|ended| ended.saturating_sub(input.started_at));
        if secs < 0 || tracked.is_none_or(|tracked| secs > tracked) {
            check.fail("billable_secs", FieldError::InvalidFormat);
        }
    }
    let fields = EntryFields {
        project_id: input.project_id,
        todo_id: input.todo_id,
        activity_id: input.activity_id,
        description: check.text("description", input.description, 2000),
        started_at: input.started_at,
        ended_at: input.ended_at,
        billable: input
            .billable
            .or(current.map(|c| c.billable))
            .unwrap_or(billable_default),
        billable_secs: input.billable_secs,
    };
    check.done()?;
    Ok(fields)
}
