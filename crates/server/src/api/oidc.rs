use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use webauthn_rs::prelude::Uuid;

use super::{
    error::{ApiError, ErrorCode, Json},
    invitations,
};
use crate::{
    App, audit,
    auth::{self, User, now, random_token},
    oidc::Pending,
};

#[derive(Deserialize, ToSchema)]
pub struct OidcStart {
    /// Set when an invitee signs up through the identity provider.
    invitation: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct OidcRedirect {
    /// Send the browser here.
    url: String,
}

#[derive(Serialize, Deserialize)]
struct Ceremony {
    pending: Pending,
    invitation: Option<Vec<u8>>,
}

#[utoipa::path(post, path = "/oidc/start", tag = "session", request_body = OidcStart,
    responses((status = OK, body = OidcRedirect), (status = "default", body = ApiError)))]
pub async fn oidc_start(
    State(app): State<Arc<App>>,
    Json(body): Json<OidcStart>,
) -> Result<Json<OidcRedirect>, ApiError> {
    let provider = app
        .oidc
        .as_ref()
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let invitation = match &body.invitation {
        Some(token) => Some(invitations::check(&app, token).await?),
        None => None,
    };
    let id = random_token();
    let (url, pending) = provider.start(id.clone()).await.map_err(|e| failed(&e))?;
    let state = Ceremony {
        pending,
        invitation,
    };
    auth::store_ceremony(&app, &id, "oidc", &state).await?;
    Ok(Json(OidcRedirect { url: url.into() }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Callback {
    state: String,
    code: Option<String>,
}

/// Where the identity provider sends the browser back; it ends on the client with a session or
/// on the sign-in page with an error.
#[utoipa::path(get, path = "/oidc/callback", tag = "session", params(Callback),
    responses((status = SEE_OTHER)))]
pub async fn oidc_callback(
    State(app): State<Arc<App>>,
    Query(callback): Query<Callback>,
) -> Response {
    match sign_in(&app, callback).await {
        Ok(cookie) => (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, "/".parse().expect("valid")),
                (header::SET_COOKIE, cookie),
            ],
        )
            .into_response(),
        Err(e) => (
            StatusCode::SEE_OTHER,
            [(header::LOCATION, format!("/sign-in#{}", e.message))],
        )
            .into_response(),
    }
}

async fn sign_in(app: &App, callback: Callback) -> Result<axum::http::HeaderValue, ApiError> {
    let provider = app
        .oidc
        .as_ref()
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let state: Ceremony = auth::take_ceremony(app, "oidc", &callback.state).await?;
    let code = callback.code.ok_or(ApiError::new(ErrorCode::OidcFailed))?;
    let identity = provider
        .finish(code, state.pending)
        .await
        .map_err(|e| failed(&e))?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let linked: Option<User> = sqlx::query_as(
        "SELECT user.id, user.name, user.role FROM oidc_identity
         JOIN user ON user.id = oidc_identity.user_id
         WHERE oidc_identity.issuer = ? AND oidc_identity.subject = ? AND user.active = 1",
    )
    .bind(&identity.issuer)
    .bind(&identity.subject)
    .fetch_optional(&mut *tx)
    .await?;
    let user = match (linked, state.invitation) {
        (Some(_), Some(_)) => return Err(ApiError::new(ErrorCode::OidcIdentityTaken)),
        (Some(user), None) => user,
        (None, Some(token_hash)) => {
            let user = invitations::accept(&mut tx, &token_hash, Uuid::new_v4()).await?;
            let linked = sqlx::query(
                "INSERT INTO oidc_identity (issuer, subject, user_id, created_at)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(&identity.issuer)
            .bind(&identity.subject)
            .bind(user.id)
            .bind(now())
            .execute(&mut *tx)
            .await;
            match linked {
                Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                    return Err(ApiError::new(ErrorCode::OidcIdentityTaken));
                }
                result => result?,
            };
            let after = json!({ "issuer": identity.issuer, "subject": identity.subject });
            let subject = format!("user:{}", user.id);
            audit::record(&mut tx, user.id, "oidc.link", &subject, None, Some(after)).await?;
            user
        }
        (None, None) => return Err(ApiError::new(ErrorCode::OidcUnknownIdentity)),
    };
    let session = auth::start_session(&mut *tx, app, user.id).await?;
    tx.commit().await?;
    Ok(session.cookie)
}

fn failed(error: &crate::oidc::Error) -> ApiError {
    eprintln!("{error}");
    ApiError::new(ErrorCode::OidcFailed)
}
