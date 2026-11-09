use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::ToSchema;

use super::error::{ApiError, ErrorCode, Json, Path};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, Role},
};

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct UserView {
    id: i64,
    name: String,
    role: Role,
    active: bool,
    created_at: i64,
    passkeys: i64,
    /// Whether the user signs in through the identity provider.
    oidc: bool,
}

#[utoipa::path(get, path = "/users", tag = "users",
    responses((status = OK, body = Vec<UserView>), (status = "default", body = ApiError)))]
pub async fn list_users(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<Vec<UserView>>, ApiError> {
    Ok(Json(
        sqlx::query_as("SELECT * FROM user_view ORDER BY id")
            .fetch_all(&app.db)
            .await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct UserChange {
    role: Option<Role>,
    active: Option<bool>,
}

#[utoipa::path(patch, path = "/users/{id}", tag = "users", request_body = UserChange,
    params(("id" = i64, Path)),
    responses((status = OK, body = UserView), (status = "default", body = ApiError)))]
pub async fn change_user(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(change): Json<UserChange>,
) -> Result<Json<UserView>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let (role, active): (Role, bool) = sqlx::query_as("SELECT role, active FROM user WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let (mut before, mut after) = (Map::new(), Map::new());
    if let Some(new) = change.role.filter(|new| *new != role) {
        before.insert("role".into(), json!(role));
        after.insert("role".into(), json!(new));
    }
    if let Some(new) = change.active.filter(|new| *new != active) {
        before.insert("active".into(), json!(active));
        after.insert("active".into(), json!(new));
    }
    sqlx::query(
        "UPDATE user SET role = coalesce(?, role), active = coalesce(?, active) WHERE id = ?",
    )
    .bind(change.role)
    .bind(change.active)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if !after.is_empty() {
        audit::record(
            &mut tx,
            auth.user.id,
            "user.update",
            &format!("user:{id}"),
            Some(Value::Object(before)),
            Some(Value::Object(after)),
        )
        .await?;
    }
    let owners: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user WHERE role = 'owner' AND active = 1")
            .fetch_one(&mut *tx)
            .await?;
    if owners == 0 {
        return Err(ApiError::new(ErrorCode::LastOwner));
    }
    if change.active == Some(false) {
        sqlx::query("DELETE FROM session WHERE user_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let user = sqlx::query_as("SELECT * FROM user_view WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(user))
}
