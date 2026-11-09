use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use webauthn_rs::prelude::{PasskeyRegistration, Uuid};

use super::{
    error::{ApiError, ErrorCode, Json, Path},
    passkeys::{Challenge, passkey_name, person_name},
    session::{RegistrationFinish, create_user, insert_passkey, signed_in},
};
use crate::{
    App, audit,
    auth::{self, Auth, OwnerOnly, Role, User, digest, now, random_token},
};

const INVITATION_SECS: i64 = 7 * 24 * 3600;

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct InvitationView {
    id: i64,
    name: String,
    role: Role,
    created_at: i64,
    expires_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct NewInvitation {
    invitation: InvitationView,
    /// The one-time link to hand to the invitee; it is not shown again.
    link: String,
}

#[derive(Deserialize, ToSchema)]
pub struct InvitationRequest {
    name: String,
    role: Role,
}

#[utoipa::path(get, path = "/invitations", tag = "users",
    responses((status = OK, body = Vec<InvitationView>), (status = "default", body = ApiError)))]
pub async fn list_invitations(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<Vec<InvitationView>>, ApiError> {
    Ok(Json(
        sqlx::query_as(
            "SELECT id, name, role, created_at, expires_at FROM invitation
             WHERE accepted_by IS NULL AND expires_at > ? ORDER BY id",
        )
        .bind(now())
        .fetch_all(&app.db)
        .await?,
    ))
}

#[utoipa::path(post, path = "/invitations", tag = "users", request_body = InvitationRequest,
    responses((status = OK, body = NewInvitation), (status = "default", body = ApiError)))]
pub async fn invite(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(request): Json<InvitationRequest>,
) -> Result<Json<NewInvitation>, ApiError> {
    let name = person_name(&request.name)?;
    let token = random_token();
    let now = now();
    let mut tx = app.db.begin().await?;
    let invitation: InvitationView = sqlx::query_as(
        "INSERT INTO invitation (token_hash, name, role, created_by, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id, name, role, created_at, expires_at",
    )
    .bind(digest(token.as_bytes()))
    .bind(&name)
    .bind(request.role)
    .bind(auth.user.id)
    .bind(now)
    .bind(now + INVITATION_SECS)
    .fetch_one(&mut *tx)
    .await?;
    let after = json!({ "name": invitation.name, "role": invitation.role });
    let subject = format!("invitation:{}", invitation.id);
    audit::record(
        &mut tx,
        auth.user.id,
        "invitation.create",
        &subject,
        None,
        Some(after),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(NewInvitation {
        invitation,
        link: format!("{}invitation#{token}", app.public_url),
    }))
}

#[utoipa::path(delete, path = "/invitations/{id}", tag = "users",
    params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn revoke_invitation(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = app.db.begin().await?;
    let (name, role): (String, Role) = sqlx::query_as(
        "DELETE FROM invitation WHERE id = ? AND accepted_by IS NULL RETURNING name, role",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let before = json!({ "name": name, "role": role });
    let subject = format!("invitation:{id}");
    audit::record(
        &mut tx,
        auth.user.id,
        "invitation.revoke",
        &subject,
        Some(before),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct InvitationToken {
    token: String,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct InvitationInfo {
    name: String,
    role: Role,
}

async fn pending(
    db: impl sqlx::SqliteExecutor<'_>,
    token_hash: &[u8],
) -> Result<(i64, InvitationInfo), ApiError> {
    let row: Option<(i64, String, Role)> = sqlx::query_as(
        "SELECT id, name, role FROM invitation
         WHERE token_hash = ? AND accepted_by IS NULL AND expires_at > ?",
    )
    .bind(token_hash)
    .bind(now())
    .fetch_optional(db)
    .await?;
    let (id, name, role) = row.ok_or(ApiError::new(ErrorCode::InvitationInvalid))?;
    Ok((id, InvitationInfo { name, role }))
}

/// Creates the invited user and marks the invitation used, inside the caller's transaction.
pub async fn accept(
    tx: &mut sqlx::SqliteConnection,
    token_hash: &[u8],
    handle: Uuid,
) -> Result<User, ApiError> {
    let (id, info) = pending(&mut *tx, token_hash).await?;
    let user = create_user(tx, &info.name, handle, info.role).await?;
    sqlx::query("UPDATE invitation SET accepted_by = ? WHERE id = ?")
        .bind(user.id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let after = json!({ "accepted_by": user.id });
    let subject = format!("invitation:{id}");
    audit::record(
        tx,
        user.id,
        "invitation.accept",
        &subject,
        None,
        Some(after),
    )
    .await?;
    Ok(user)
}

pub async fn check(app: &App, token: &str) -> Result<Vec<u8>, ApiError> {
    let token_hash = digest(token.as_bytes());
    pending(&app.db, &token_hash).await?;
    Ok(token_hash)
}

#[utoipa::path(post, path = "/invitation", tag = "users", request_body = InvitationToken,
    responses((status = OK, body = InvitationInfo), (status = "default", body = ApiError)))]
pub async fn view_invitation(
    State(app): State<Arc<App>>,
    Json(body): Json<InvitationToken>,
) -> Result<Json<InvitationInfo>, ApiError> {
    let (_, info) = pending(&app.db, &digest(body.token.as_bytes())).await?;
    Ok(Json(info))
}

#[derive(Serialize, Deserialize)]
struct AcceptState {
    token_hash: Vec<u8>,
    handle: Uuid,
    registration: PasskeyRegistration,
}

#[utoipa::path(post, path = "/invitation/passkey/start", tag = "users",
    request_body = InvitationToken,
    responses((status = OK, body = Challenge), (status = "default", body = ApiError)))]
pub async fn accept_start(
    State(app): State<Arc<App>>,
    Json(body): Json<InvitationToken>,
) -> Result<Json<Challenge>, ApiError> {
    let token_hash = digest(body.token.as_bytes());
    let (_, info) = pending(&app.db, &token_hash).await?;
    let handle = Uuid::new_v4();
    let (options, registration) = app
        .webauthn
        .start_passkey_registration(handle, &info.name, &info.name, None)
        .map_err(ApiError::internal)?;
    let state = AcceptState {
        token_hash,
        handle,
        registration,
    };
    Ok(Json(Challenge {
        ceremony: auth::start_ceremony(&app, "accept", &state).await?,
        options: serde_json::to_value(options).map_err(ApiError::internal)?,
    }))
}

#[utoipa::path(post, path = "/invitation/passkey/finish", tag = "users",
    request_body = RegistrationFinish,
    responses((status = OK, body = super::session::SessionView), (status = "default", body = ApiError)))]
pub async fn accept_finish(
    State(app): State<Arc<App>>,
    Json(body): Json<RegistrationFinish>,
) -> Result<Response, ApiError> {
    let name = passkey_name(&body.passkey_name)?;
    let state: AcceptState = auth::take_ceremony(&app, "accept", &body.ceremony).await?;
    let passkey = app
        .webauthn
        .finish_passkey_registration(&body.credential, &state.registration)
        .map_err(|_| ApiError::new(ErrorCode::PasskeyRejected))?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let user = accept(&mut tx, &state.token_hash, state.handle).await?;
    insert_passkey(&mut tx, user.id, &name, &passkey).await?;
    let session = auth::start_session(&mut tx, &app, user.id).await?;
    tx.commit().await?;
    Ok(signed_in(&app, session, user))
}
