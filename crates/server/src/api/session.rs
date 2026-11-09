use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use webauthn_rs::prelude::{
    DiscoverableAuthentication, Passkey, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Uuid,
};

use super::{
    error::{ApiError, ErrorCode, Json},
    passkeys::{Challenge, passkey_name, person_name},
};
use crate::{
    App, audit,
    auth::{self, Auth, NewSession, Role, User, digest, now},
};

#[derive(Serialize, ToSchema)]
pub struct SessionView {
    user: Option<User>,
    /// Send as `x-csrf-token` with every state-changing request.
    csrf_token: Option<String>,
    setup_required: bool,
    /// Label of the identity provider's sign-in button, if one is configured.
    oidc: Option<String>,
}

#[utoipa::path(get, path = "/session", tag = "session",
    responses((status = OK, body = SessionView), (status = "default", body = ApiError)))]
pub async fn get_session(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> Result<Json<SessionView>, ApiError> {
    let current = auth::current(&app, &headers).await?;
    Ok(Json(SessionView {
        setup_required: app.setup_required().await?,
        oidc: app.oidc.as_ref().map(|p| p.name.clone()),
        csrf_token: current.as_ref().map(|c| c.csrf_token.clone()),
        user: current.map(|c| c.user),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct SetupStart {
    /// The token the server printed when it started without users.
    token: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct SetupState {
    name: String,
    handle: Uuid,
    registration: PasskeyRegistration,
}

#[utoipa::path(post, path = "/setup/start", tag = "session", request_body = SetupStart,
    responses((status = OK, body = Challenge), (status = "default", body = ApiError)))]
pub async fn setup_start(
    State(app): State<Arc<App>>,
    Json(body): Json<SetupStart>,
) -> Result<Json<Challenge>, ApiError> {
    if !app.setup_required().await? {
        return Err(ApiError::new(ErrorCode::SetupDone));
    }
    if digest(body.token.as_bytes()) != digest(app.setup_token.as_bytes()) {
        return Err(ApiError::new(ErrorCode::InvalidSetupToken));
    }
    let name = person_name(&body.name)?;
    let handle = Uuid::new_v4();
    let (options, registration) = app
        .webauthn
        .start_passkey_registration(handle, &name, &name, None)
        .map_err(ApiError::internal)?;
    let state = SetupState {
        name,
        handle,
        registration,
    };
    Ok(Json(Challenge {
        ceremony: auth::start_ceremony(&app, "setup", &state).await?,
        options: serde_json::to_value(options).map_err(ApiError::internal)?,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct RegistrationFinish {
    pub ceremony: String,
    /// Shown in the user's list of passkeys.
    pub passkey_name: String,
    #[schema(value_type = HashMap<String, serde_json::Value>)]
    pub credential: RegisterPublicKeyCredential,
}

#[utoipa::path(post, path = "/setup/finish", tag = "session", request_body = RegistrationFinish,
    responses((status = OK, body = SessionView), (status = "default", body = ApiError)))]
pub async fn setup_finish(
    State(app): State<Arc<App>>,
    Json(body): Json<RegistrationFinish>,
) -> Result<Response, ApiError> {
    let passkey_name = passkey_name(&body.passkey_name)?;
    let state: SetupState = auth::take_ceremony(&app, "setup", &body.ceremony).await?;
    let passkey = app
        .webauthn
        .finish_passkey_registration(&body.credential, &state.registration)
        .map_err(|_| ApiError::new(ErrorCode::PasskeyRejected))?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM user")
        .fetch_one(&mut *tx)
        .await?;
    if users > 0 {
        return Err(ApiError::new(ErrorCode::SetupDone));
    }
    let user = create_user(&mut tx, &state.name, state.handle, Role::Owner).await?;
    insert_passkey(&mut tx, user.id, &passkey_name, &passkey).await?;
    let session = auth::start_session(&mut *tx, &app, user.id).await?;
    tx.commit().await?;
    Ok(signed_in(&app, session, user))
}

pub async fn create_user(
    tx: &mut sqlx::SqliteConnection,
    name: &str,
    handle: Uuid,
    role: Role,
) -> Result<User, ApiError> {
    let user: User = sqlx::query_as(
        "INSERT INTO user (handle, name, role, created_at) VALUES (?, ?, ?, ?)
         RETURNING id, name, role",
    )
    .bind(handle.as_bytes().as_slice())
    .bind(name)
    .bind(role)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    let after = json!({ "name": user.name, "role": user.role, "active": true });
    audit::record(
        tx,
        user.id,
        "user.create",
        &format!("user:{}", user.id),
        None,
        Some(after),
    )
    .await?;
    Ok(user)
}

pub async fn insert_passkey(
    tx: &mut sqlx::SqliteConnection,
    user_id: i64,
    name: &str,
    passkey: &Passkey,
) -> Result<(), ApiError> {
    let result = sqlx::query_scalar(
        "INSERT INTO passkey (user_id, credential_id, name, passkey, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(passkey.cred_id().as_slice())
    .bind(name)
    .bind(serde_json::to_string(passkey).map_err(ApiError::internal)?)
    .bind(now())
    .fetch_one(&mut *tx)
    .await;
    let id: i64 = match result {
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(ApiError::new(ErrorCode::PasskeyRejected));
        }
        result => result?,
    };
    let after = json!({ "user_id": user_id, "name": name });
    audit::record(
        tx,
        user_id,
        "passkey.add",
        &format!("passkey:{id}"),
        None,
        Some(after),
    )
    .await?;
    Ok(())
}

pub fn signed_in(app: &App, session: NewSession, user: User) -> Response {
    let view = SessionView {
        user: Some(user),
        csrf_token: Some(session.csrf_token),
        setup_required: false,
        oidc: app.oidc.as_ref().map(|p| p.name.clone()),
    };
    ([(header::SET_COOKIE, session.cookie)], Json(view)).into_response()
}

#[utoipa::path(post, path = "/sign-in/start", tag = "session",
    responses((status = OK, body = Challenge), (status = "default", body = ApiError)))]
pub async fn sign_in_start(State(app): State<Arc<App>>) -> Result<Json<Challenge>, ApiError> {
    let (options, state) = app
        .webauthn
        .start_discoverable_authentication()
        .map_err(ApiError::internal)?;
    Ok(Json(Challenge {
        ceremony: auth::start_ceremony(&app, "sign_in", &state).await?,
        options: serde_json::to_value(options).map_err(ApiError::internal)?,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct SignInFinish {
    ceremony: String,
    #[schema(value_type = HashMap<String, serde_json::Value>)]
    credential: PublicKeyCredential,
}

#[utoipa::path(post, path = "/sign-in/finish", tag = "session", request_body = SignInFinish,
    responses((status = OK, body = SessionView), (status = "default", body = ApiError)))]
pub async fn sign_in_finish(
    State(app): State<Arc<App>>,
    Json(body): Json<SignInFinish>,
) -> Result<Response, ApiError> {
    let state: DiscoverableAuthentication =
        auth::take_ceremony(&app, "sign_in", &body.ceremony).await?;
    let rejected = || ApiError::new(ErrorCode::PasskeyRejected);
    let row: Option<(i64, String, i64, String, Role)> = sqlx::query_as(
        "SELECT passkey.id, passkey.passkey, user.id, user.name, user.role
         FROM passkey JOIN user ON user.id = passkey.user_id
         WHERE passkey.credential_id = ? AND user.active = 1",
    )
    .bind(body.credential.get_credential_id())
    .fetch_optional(&app.db)
    .await?;
    let (passkey_id, passkey, user_id, name, role) = row.ok_or_else(rejected)?;
    let mut passkey: Passkey = serde_json::from_str(&passkey).map_err(ApiError::internal)?;
    let result = app
        .webauthn
        .finish_discoverable_authentication_v2(&body.credential, state, &passkey)
        .map_err(|_| rejected())?;
    passkey.update_credential(&result);
    let mut tx = app.db.begin().await?;
    sqlx::query("UPDATE passkey SET passkey = ?, last_used_at = ? WHERE id = ?")
        .bind(serde_json::to_string(&passkey).map_err(ApiError::internal)?)
        .bind(now())
        .bind(passkey_id)
        .execute(&mut *tx)
        .await?;
    let session = auth::start_session(&mut *tx, &app, user_id).await?;
    tx.commit().await?;
    let user = User {
        id: user_id,
        name,
        role,
    };
    Ok(signed_in(&app, session, user))
}

#[utoipa::path(post, path = "/sign-out", tag = "session",
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn sign_out(State(app): State<Arc<App>>, auth: Auth) -> Result<Response, ApiError> {
    sqlx::query("DELETE FROM session WHERE token_hash = ?")
        .bind(&auth.token_hash)
        .execute(&app.db)
        .await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, auth::clear_session_cookie(&app))],
    )
        .into_response())
}
