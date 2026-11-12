use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use super::{
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, digest, now, random_token},
    tokens::Scope,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct Token {
    id: i64,
    name: String,
    scopes: Vec<Scope>,
    created_at: i64,
    last_used_at: Option<i64>,
    /// Only when just created: the token itself, shown once.
    secret: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    name: String,
    scopes: String,
    created_at: i64,
    last_used_at: Option<i64>,
}

impl From<Row> for Token {
    fn from(row: Row) -> Self {
        Self {
            id: row.id,
            name: row.name,
            scopes: row.scopes.split(' ').filter_map(Scope::parse).collect(),
            created_at: row.created_at,
            last_used_at: row.last_used_at,
            secret: None,
        }
    }
}

async fn own(db: &mut SqliteConnection, user_id: i64) -> Result<Vec<Token>, sqlx::Error> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, name, scopes, created_at, last_used_at FROM api_token
         WHERE user_id = ? AND revoked_at IS NULL ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Token::from).collect())
}

/// Your API tokens that are not revoked.
#[utoipa::path(get, path = "/tokens", tag = "tokens",
    responses((status = OK, body = Vec<Token>), (status = "default", body = ApiError)))]
pub async fn list_tokens(
    State(app): State<Arc<App>>,
    auth: Auth,
) -> Result<Json<Vec<Token>>, ApiError> {
    let mut db = app.db.acquire().await?;
    Ok(Json(own(&mut db, auth.user.id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TokenInput {
    name: Option<String>,
    scopes: Vec<Scope>,
}

/// Creates an API token that acts as you within its scopes; send it as `Authorization: Bearer`.
#[utoipa::path(post, path = "/tokens", tag = "tokens", request_body = TokenInput,
    responses((status = OK, body = Token), (status = "default", body = ApiError)))]
pub async fn create_token(
    State(app): State<Arc<App>>,
    auth: Auth,
    Json(input): Json<TokenInput>,
) -> Result<Json<Token>, ApiError> {
    let mut check = Check::default();
    let name = check.required("name", input.name, 100);
    let mut scopes = input.scopes;
    scopes.sort();
    scopes.dedup();
    if scopes.is_empty() {
        check.fail("scopes", FieldError::Required);
    }
    check.done()?;
    let secret = format!("cc_{}", random_token());
    let scopes_text = scopes
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let created_at = now();
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO api_token (user_id, name, token_hash, scopes, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(auth.user.id)
    .bind(&name)
    .bind(digest(secret.as_bytes()))
    .bind(&scopes_text)
    .bind(created_at)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "token.create",
        &format!("token:{id}"),
        None,
        Some(serde_json::json!({ "name": name, "scopes": scopes_text })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(Token {
        id,
        name,
        scopes,
        created_at,
        last_used_at: None,
        secret: Some(secret),
    }))
}

/// Revokes one of your API tokens for good.
#[utoipa::path(delete, path = "/tokens/{id}", tag = "tokens", params(("id" = i64, Path)),
    responses((status = OK, body = Vec<Token>), (status = "default", body = ApiError)))]
pub async fn revoke_token(
    State(app): State<Arc<App>>,
    auth: Auth,
    Path(id): Path<i64>,
) -> Result<Json<Vec<Token>>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let revoked = sqlx::query(
        "UPDATE api_token SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL",
    )
    .bind(now())
    .bind(id)
    .bind(auth.user.id)
    .execute(&mut *tx)
    .await?;
    if revoked.rows_affected() == 0 {
        return Err(ApiError::new(ErrorCode::NotFound));
    }
    audit::record(
        &mut tx,
        auth.user.id,
        "token.revoke",
        &format!("token:{id}"),
        None,
        None,
    )
    .await?;
    let tokens = own(&mut tx, auth.user.id).await?;
    tx.commit().await?;
    Ok(Json(tokens))
}
