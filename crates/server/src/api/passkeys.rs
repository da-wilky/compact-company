use std::{collections::BTreeMap, sync::Arc};

use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use webauthn_rs::prelude::{CredentialID, PasskeyRegistration, Uuid};

use super::{
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    session::{RegistrationFinish, insert_passkey},
};
use crate::{
    App, audit,
    auth::{self, Auth},
};

/// A started WebAuthn ceremony: pass `options` to the browser and send `ceremony` back with its
/// result.
#[derive(Serialize, ToSchema)]
pub struct Challenge {
    pub ceremony: String,
    #[schema(value_type = HashMap<String, serde_json::Value>)]
    pub options: serde_json::Value,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PasskeyView {
    id: i64,
    name: String,
    created_at: i64,
    last_used_at: Option<i64>,
}

pub fn person_name(name: &str) -> Result<String, ApiError> {
    required("name", name, 100)
}

pub fn passkey_name(name: &str) -> Result<String, ApiError> {
    required("passkey_name", name, 100)
}

fn required(field: &str, value: &str, max: usize) -> Result<String, ApiError> {
    let value = value.trim();
    let error = if value.is_empty() {
        FieldError::Required
    } else if value.chars().count() > max {
        FieldError::TooLong
    } else {
        return Ok(value.to_owned());
    };
    Err(ApiError::fields(BTreeMap::from([(
        field.to_owned(),
        error,
    )])))
}

#[utoipa::path(get, path = "/passkeys", tag = "passkeys",
    responses((status = OK, body = Vec<PasskeyView>), (status = "default", body = ApiError)))]
pub async fn list_passkeys(
    State(app): State<Arc<App>>,
    auth: Auth,
) -> Result<Json<Vec<PasskeyView>>, ApiError> {
    list(&app, auth.user.id).await.map(Json)
}

async fn list(app: &App, user_id: i64) -> Result<Vec<PasskeyView>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT id, name, created_at, last_used_at FROM passkey WHERE user_id = ? ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&app.db)
    .await?)
}

#[derive(Serialize, Deserialize)]
struct AddState {
    user_id: i64,
    registration: PasskeyRegistration,
}

#[utoipa::path(post, path = "/passkeys/start", tag = "passkeys",
    responses((status = OK, body = Challenge), (status = "default", body = ApiError)))]
pub async fn add_passkey_start(
    State(app): State<Arc<App>>,
    auth: Auth,
) -> Result<Json<Challenge>, ApiError> {
    let handle: Vec<u8> = sqlx::query_scalar("SELECT handle FROM user WHERE id = ?")
        .bind(auth.user.id)
        .fetch_one(&app.db)
        .await?;
    let handle = Uuid::from_slice(&handle).map_err(ApiError::internal)?;
    let exclude: Vec<CredentialID> =
        sqlx::query_scalar("SELECT credential_id FROM passkey WHERE user_id = ?")
            .bind(auth.user.id)
            .fetch_all(&app.db)
            .await?;
    let (options, registration) = app
        .webauthn
        .start_passkey_registration(handle, &auth.user.name, &auth.user.name, Some(exclude))
        .map_err(ApiError::internal)?;
    let state = AddState {
        user_id: auth.user.id,
        registration,
    };
    Ok(Json(Challenge {
        ceremony: auth::start_ceremony(&app, "add_passkey", &state).await?,
        options: serde_json::to_value(options).map_err(ApiError::internal)?,
    }))
}

#[utoipa::path(post, path = "/passkeys/finish", tag = "passkeys", request_body = RegistrationFinish,
    responses((status = OK, body = Vec<PasskeyView>), (status = "default", body = ApiError)))]
pub async fn add_passkey_finish(
    State(app): State<Arc<App>>,
    auth: Auth,
    Json(body): Json<RegistrationFinish>,
) -> Result<Json<Vec<PasskeyView>>, ApiError> {
    let name = passkey_name(&body.passkey_name)?;
    let state: AddState = auth::take_ceremony(&app, "add_passkey", &body.ceremony).await?;
    if state.user_id != auth.user.id {
        return Err(ApiError::new(ErrorCode::CeremonyExpired));
    }
    let passkey = app
        .webauthn
        .finish_passkey_registration(&body.credential, &state.registration)
        .map_err(|_| ApiError::new(ErrorCode::PasskeyRejected))?;
    let mut tx = app.db.begin().await?;
    insert_passkey(&mut tx, auth.user.id, &name, &passkey).await?;
    tx.commit().await?;
    list(&app, auth.user.id).await.map(Json)
}

#[utoipa::path(delete, path = "/passkeys/{id}", tag = "passkeys",
    params(("id" = i64, Path)),
    responses((status = OK, body = Vec<PasskeyView>), (status = "default", body = ApiError)))]
pub async fn remove_passkey(
    State(app): State<Arc<App>>,
    auth: Auth,
    Path(id): Path<i64>,
) -> Result<Json<Vec<PasskeyView>>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM passkey WHERE user_id = ?")
        .bind(auth.user.id)
        .fetch_one(&mut *tx)
        .await?;
    let name: String =
        sqlx::query_scalar("DELETE FROM passkey WHERE id = ? AND user_id = ? RETURNING name")
            .bind(id)
            .bind(auth.user.id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(ApiError::new(ErrorCode::NotFound))?;
    if count <= 1 {
        return Err(ApiError::new(ErrorCode::LastPasskey));
    }
    let before = json!({ "user_id": auth.user.id, "name": name });
    audit::record(
        &mut tx,
        auth.user.id,
        "passkey.remove",
        &format!("passkey:{id}"),
        Some(before),
        None,
    )
    .await?;
    tx.commit().await?;
    list(&app, auth.user.id).await.map(Json)
}
