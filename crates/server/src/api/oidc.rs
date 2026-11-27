use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
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
    auth::{self, Auth, User, digest, now, random_token},
    oidc::{Identity, Pending},
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

#[derive(Serialize, ToSchema)]
pub struct OidcLink {
    /// Whether the current user signs in through the identity provider.
    linked: bool,
}

#[derive(Serialize, Deserialize)]
struct Ceremony {
    pending: Pending,
    invitation: Option<Vec<u8>>,
    /// The signed-in user the identity is to be linked to.
    #[serde(default)]
    link: Option<i64>,
}

#[utoipa::path(post, path = "/oidc/start", tag = "session", request_body = OidcStart,
    responses((status = OK, body = OidcRedirect), (status = "default", body = ApiError)))]
pub async fn oidc_start(
    State(app): State<Arc<App>>,
    _: crate::limit::Limited,
    Json(body): Json<OidcStart>,
) -> Result<Response, ApiError> {
    let invitation = match &body.invitation {
        Some(token) => Some(invitations::check(&app, token).await?),
        None => None,
    };
    redirect_to_provider(&app, invitation, None).await
}

#[utoipa::path(get, path = "/oidc/link", tag = "session",
    responses((status = OK, body = OidcLink), (status = "default", body = ApiError)))]
pub async fn get_oidc_link(
    State(app): State<Arc<App>>,
    auth: Auth,
) -> Result<Json<OidcLink>, ApiError> {
    Ok(Json(OidcLink {
        linked: is_linked(&app.db, auth.user.id).await?,
    }))
}

/// Starts linking the identity provider to the signed-in user; the callback returns to the
/// account page.
#[utoipa::path(post, path = "/oidc/link", tag = "session",
    responses((status = OK, body = OidcRedirect), (status = "default", body = ApiError)))]
pub async fn link_oidc(
    State(app): State<Arc<App>>,
    _: crate::limit::Limited,
    auth: Auth,
) -> Result<Response, ApiError> {
    if is_linked(&app.db, auth.user.id).await? {
        return Err(ApiError::new(ErrorCode::OidcAlreadyLinked));
    }
    redirect_to_provider(&app, None, Some(auth.user.id)).await
}

async fn is_linked(db: impl sqlx::SqliteExecutor<'_>, user_id: i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM oidc_identity WHERE user_id = ?)")
        .bind(user_id)
        .fetch_one(db)
        .await
}

async fn redirect_to_provider(
    app: &App,
    invitation: Option<Vec<u8>>,
    link: Option<i64>,
) -> Result<Response, ApiError> {
    let provider = app
        .oidc
        .as_ref()
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let id = random_token();
    let (url, pending) = provider.start(id.clone()).await.map_err(|e| failed(&e))?;
    let state = Ceremony {
        pending,
        invitation,
        link,
    };
    auth::store_ceremony(app, &id, "oidc", &state).await?;
    let cookie = auth::set_cookie(app, STATE_COOKIE, &id, CALLBACK_PATH, 300);
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(OidcRedirect { url: url.into() }),
    )
        .into_response())
}

/// Binds the provider's answer to the browser that asked, so nobody can sign another person in
/// to their own account by sending them a callback link. Lax, as the provider redirects back.
const STATE_COOKIE: &str = "oidc_state";
const CALLBACK_PATH: &str = "/api/oidc/callback; SameSite=Lax";

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
    headers: HeaderMap,
    Query(callback): Query<Callback>,
) -> Response {
    let clear_state = auth::set_cookie(&app, STATE_COOKIE, "", CALLBACK_PATH, 0);
    let mut response = match finish(&app, &headers, callback).await {
        Ok(Some(cookie)) => (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, "/".parse().expect("valid")),
                (header::SET_COOKIE, cookie),
            ],
        )
            .into_response(),
        Ok(None) => (StatusCode::SEE_OTHER, [(header::LOCATION, ACCOUNT_PAGE)]).into_response(),
        Err((page, e)) => (
            StatusCode::SEE_OTHER,
            [(header::LOCATION, format!("{page}#{}", e.message))],
        )
            .into_response(),
    };
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_state);
    response
}

const SIGN_IN_PAGE: &str = "/sign-in";
const ACCOUNT_PAGE: &str = "/account";

/// Signs in, returning the session cookie, or links the identity to the user who asked; a failure
/// names the page to report it on.
async fn finish(
    app: &App,
    headers: &HeaderMap,
    callback: Callback,
) -> Result<Option<axum::http::HeaderValue>, (&'static str, ApiError)> {
    let on_sign_in = |e| (SIGN_IN_PAGE, e);
    let provider = app
        .oidc
        .as_ref()
        .ok_or(ApiError::new(ErrorCode::NotFound))
        .map_err(on_sign_in)?;
    let started_here = auth::cookie(headers, STATE_COOKIE)
        .is_some_and(|id| digest(id.as_bytes()) == digest(callback.state.as_bytes()));
    if !started_here {
        return Err(on_sign_in(ApiError::new(ErrorCode::OidcFailed)));
    }
    let state: Ceremony = auth::take_ceremony(app, "oidc", &callback.state)
        .await
        .map_err(on_sign_in)?;
    let page = if state.link.is_some() {
        ACCOUNT_PAGE
    } else {
        SIGN_IN_PAGE
    };
    let code = callback
        .code
        .ok_or(ApiError::new(ErrorCode::OidcFailed))
        .map_err(|e| (page, e))?;
    let identity = provider
        .finish(code, state.pending)
        .await
        .map_err(|e| (page, failed(&e)))?;
    match state.link {
        Some(user_id) => link(app, user_id, &identity).await.map(|()| None),
        None => sign_in(app, state.invitation, &identity).await.map(Some),
    }
    .map_err(|e| (page, e))
}

async fn link(app: &App, user_id: i64, identity: &Identity) -> Result<(), ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let active: bool = sqlx::query_scalar("SELECT active FROM user WHERE id = ?")
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    if !active {
        return Err(ApiError::new(ErrorCode::Unauthenticated));
    }
    if is_linked(&mut *tx, user_id).await? {
        return Err(ApiError::new(ErrorCode::OidcAlreadyLinked));
    }
    insert_identity(&mut tx, identity, user_id).await?;
    tx.commit().await?;
    Ok(())
}

/// Links the identity to the user and records it; refused when another user has it.
async fn insert_identity(
    tx: &mut sqlx::SqliteConnection,
    identity: &Identity,
    user_id: i64,
) -> Result<(), ApiError> {
    let inserted = sqlx::query(
        "INSERT INTO oidc_identity (issuer, subject, user_id, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&identity.issuer)
    .bind(&identity.subject)
    .bind(user_id)
    .bind(now())
    .execute(&mut *tx)
    .await;
    match inserted {
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(ApiError::new(ErrorCode::OidcIdentityTaken));
        }
        result => result?,
    };
    let after = json!({ "issuer": identity.issuer, "subject": identity.subject });
    let subject = format!("user:{user_id}");
    audit::record(tx, user_id, "oidc.link", &subject, None, Some(after)).await?;
    Ok(())
}

async fn sign_in(
    app: &App,
    invitation: Option<Vec<u8>>,
    identity: &Identity,
) -> Result<axum::http::HeaderValue, ApiError> {
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
    let user = match (linked, invitation) {
        (Some(_), Some(_)) => return Err(ApiError::new(ErrorCode::OidcIdentityTaken)),
        (Some(user), None) => user,
        (None, Some(token_hash)) => {
            let user = invitations::accept(&mut tx, &token_hash, Uuid::new_v4()).await?;
            insert_identity(&mut tx, identity, user.id).await?;
            user
        }
        (None, None) => return Err(ApiError::new(ErrorCode::OidcUnknownIdentity)),
    };
    let session = auth::start_session(&mut tx, app, user.id).await?;
    tx.commit().await?;
    Ok(session.cookie)
}

fn failed(error: &crate::oidc::Error) -> ApiError {
    eprintln!("{error}");
    ApiError::new(ErrorCode::OidcFailed)
}
