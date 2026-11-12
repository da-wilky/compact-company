use std::sync::Arc;

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

use super::{
    documents::{business, today},
    error::{ApiError, ErrorCode, Json, Path},
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, digest, now, random_token},
    deadlines::{self, Deadline},
};

/// The deadlines due from a month ago to a year ahead.
#[utoipa::path(get, path = "/deadlines", tag = "tax",
    responses((status = OK, body = Vec<Deadline>), (status = "default", body = ApiError)))]
pub async fn list_deadlines(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
) -> Result<Json<Vec<Deadline>>, ApiError> {
    let business = business(&app).await?;
    let today = today(&business)?;
    let since = today
        .checked_sub(jiff::Span::new().months(1))
        .map_err(ApiError::internal)?
        .to_string();
    let until = today
        .checked_add(jiff::Span::new().years(1))
        .map_err(ApiError::internal)?
        .to_string();
    Ok(Json(
        deadlines::deadlines(&business, today.year() - 2, today.year() + 1)
            .into_iter()
            .filter(|d| d.due >= since && d.due <= until)
            .collect(),
    ))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Feed {
    /// When the current link was made; none while there is no link.
    created_at: Option<i64>,
    /// Only right after renewing: the secret link, shown once.
    link: Option<String>,
}

#[utoipa::path(get, path = "/calendar-feed", tag = "tax",
    responses((status = OK, body = Feed), (status = "default", body = ApiError)))]
pub async fn get_feed(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<Feed>, ApiError> {
    let created_at = sqlx::query_scalar("SELECT created_at FROM calendar_feed")
        .fetch_optional(&app.db)
        .await?;
    Ok(Json(Feed {
        created_at,
        link: None,
    }))
}

/// Makes a new secret link for calendar apps; the old one stops working.
#[utoipa::path(post, path = "/calendar-feed", tag = "tax",
    responses((status = OK, body = Feed), (status = "default", body = ApiError)))]
pub async fn renew_feed(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
) -> Result<Json<Feed>, ApiError> {
    let token = random_token();
    let created_at = now();
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        "INSERT INTO calendar_feed (id, token_hash, created_at, created_by) VALUES (1, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET token_hash = excluded.token_hash,
           created_at = excluded.created_at, created_by = excluded.created_by",
    )
    .bind(digest(token.as_bytes()))
    .bind(created_at)
    .bind(auth.user.id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "calendar_feed.renew",
        "calendar_feed",
        None,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(Feed {
        created_at: Some(created_at),
        link: Some(format!("{}api/calendar/{token}.ics", app.public_url)),
    }))
}

#[utoipa::path(delete, path = "/calendar-feed", tag = "tax",
    responses((status = OK, body = Feed), (status = "default", body = ApiError)))]
pub async fn delete_feed(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
) -> Result<Json<Feed>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("DELETE FROM calendar_feed")
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "calendar_feed.delete",
        "calendar_feed",
        None,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(Feed {
        created_at: None,
        link: None,
    }))
}

/// The deadlines as an iCalendar feed, for the secret link only.
#[utoipa::path(get, path = "/calendar/{file}", tag = "tax", params(("file" = String, Path)),
    responses((status = OK, content_type = "text/calendar", body = String),
        (status = "default", body = ApiError)))]
pub async fn calendar(
    State(app): State<Arc<App>>,
    Path(file): Path<String>,
) -> Result<Response, ApiError> {
    let token = file
        .strip_suffix(".ics")
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let known: bool =
        sqlx::query_scalar("SELECT count(*) > 0 FROM calendar_feed WHERE token_hash = ?")
            .bind(digest(token.as_bytes()))
            .fetch_one(&app.db)
            .await?;
    if !known {
        return Err(ApiError::new(ErrorCode::NotFound));
    }
    let business = business(&app).await?;
    let year = today(&business)?.year();
    let stamp = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%SZ")
        .to_string();
    let body = deadlines::ics(
        &deadlines::deadlines(&business, year - 1, year + 1),
        app.public_url.host_str().unwrap_or("localhost"),
        &stamp,
    );
    Ok((
        [(header::CONTENT_TYPE, "text/calendar; charset=utf-8")],
        body,
    )
        .into_response())
}
