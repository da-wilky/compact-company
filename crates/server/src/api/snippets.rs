use std::sync::Arc;

use axum::{extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use super::{
    customers::Language,
    error::{ApiError, ErrorCode, Json, Path},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, now},
};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SnippetInput {
    name: Option<String>,
    language: Language,
    body: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Snippet {
    id: i64,
    #[sqlx(flatten)]
    #[serde(flatten)]
    fields: SnippetFields,
    updated_at: i64,
}

/// What an edit may change; also the snapshot the audit log compares.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct SnippetFields {
    name: String,
    language: Language,
    body: String,
}

#[utoipa::path(get, path = "/snippets", tag = "documents",
    responses((status = OK, body = Vec<Snippet>), (status = "default", body = ApiError)))]
pub async fn list_snippets(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<Vec<Snippet>>, ApiError> {
    Ok(Json(
        sqlx::query_as("SELECT * FROM snippet ORDER BY name COLLATE NOCASE, language")
            .fetch_all(&app.db)
            .await?,
    ))
}

#[utoipa::path(post, path = "/snippets", tag = "documents", request_body = SnippetInput,
    responses((status = OK, body = Snippet), (status = "default", body = ApiError)))]
pub async fn create_snippet(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<SnippetInput>,
) -> Result<Json<Snippet>, ApiError> {
    let fields = validate(input)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO snippet (name, language, body, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&fields.name)
    .bind(fields.language)
    .bind(&fields.body)
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "snippet.create",
        &format!("snippet:{id}"),
        None,
        Some(serde_json::to_value(&fields).map_err(ApiError::internal)?),
    )
    .await?;
    let snippet = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(snippet))
}

#[utoipa::path(put, path = "/snippets/{id}", tag = "documents", request_body = SnippetInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Snippet), (status = "default", body = ApiError)))]
pub async fn put_snippet(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<SnippetInput>,
) -> Result<Json<Snippet>, ApiError> {
    let fields = validate(input)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    sqlx::query("UPDATE snippet SET name = ?, language = ?, body = ?, updated_at = ? WHERE id = ?")
        .bind(&fields.name)
        .bind(fields.language)
        .bind(&fields.body)
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if let Some((old, new)) = audit::changes(&before.fields, &fields) {
        audit::record(
            &mut tx,
            auth.user.id,
            "snippet.update",
            &format!("snippet:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    let snippet = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(snippet))
}

/// Documents keep the text copied from it.
#[utoipa::path(delete, path = "/snippets/{id}", tag = "documents", params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn delete_snippet(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    sqlx::query("DELETE FROM snippet WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "snippet.delete",
        &format!("snippet:{id}"),
        Some(serde_json::to_value(&before.fields).map_err(ApiError::internal)?),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn find(db: &mut SqliteConnection, id: i64) -> Result<Snippet, ApiError> {
    sqlx::query_as("SELECT * FROM snippet WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}

fn validate(input: SnippetInput) -> Result<SnippetFields, ApiError> {
    let mut check = Check::default();
    let fields = SnippetFields {
        name: check.required("name", input.name, 100),
        language: input.language,
        body: check.required("body", input.body, 10_000),
    };
    check.done()?;
    Ok(fields)
}
