use std::{
    marker::PhantomData,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, HeaderValue, header, request::Parts},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::SqliteExecutor;
use utoipa::ToSchema;

use crate::{
    App,
    api::error::{ApiError, ErrorCode},
};

const SESSION_COOKIE: &str = "session";
pub const CSRF_HEADER: &str = "x-csrf-token";
const SESSION_IDLE_SECS: i64 = 7 * 24 * 3600;
const SESSION_MAX_SECS: i64 = 30 * 24 * 3600;
const CEREMONY_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Staff,
    Accountant,
}

#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub role: Role,
}

pub trait Allowed {
    const ROLES: &[Role];
}

pub struct AnyRole;
pub struct OwnerOnly;
pub struct OwnerOrAccountant;

impl Allowed for AnyRole {
    const ROLES: &[Role] = &[Role::Owner, Role::Staff, Role::Accountant];
}

impl Allowed for OwnerOnly {
    const ROLES: &[Role] = &[Role::Owner];
}

impl Allowed for OwnerOrAccountant {
    const ROLES: &[Role] = &[Role::Owner, Role::Accountant];
}

/// A signed-in user whose role is one of `R::ROLES`; state-changing requests must also carry the
/// session's CSRF token.
pub struct Auth<R: Allowed = AnyRole> {
    pub user: User,
    pub token_hash: Vec<u8>,
    roles: PhantomData<R>,
}

pub struct Current {
    pub user: User,
    pub token_hash: Vec<u8>,
    pub csrf_token: String,
}

impl<R: Allowed> FromRequestParts<Arc<App>> for Auth<R> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, ApiError> {
        let current = current(app, &parts.headers)
            .await?
            .ok_or(ApiError::new(ErrorCode::Unauthenticated))?;
        if !parts.method.is_safe() {
            let sent = parts.headers.get(CSRF_HEADER).map(HeaderValue::as_bytes);
            if sent.map(digest) != Some(digest(current.csrf_token.as_bytes())) {
                return Err(ApiError::new(ErrorCode::CsrfToken));
            }
        }
        if !R::ROLES.contains(&current.user.role) {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }
        Ok(Self {
            user: current.user,
            token_hash: current.token_hash,
            roles: PhantomData,
        })
    }
}

/// The session the request's cookie belongs to, if it is still valid.
pub async fn current(app: &App, headers: &HeaderMap) -> Result<Option<Current>, ApiError> {
    let Some(token) = cookie(headers, SESSION_COOKIE) else {
        return Ok(None);
    };
    let token_hash = digest(token.as_bytes());
    let now = now();
    let row: Option<(i64, String, Role, String, i64)> = sqlx::query_as(
        "SELECT user.id, user.name, user.role, session.csrf_token, session.last_seen_at
         FROM session JOIN user ON user.id = session.user_id
         WHERE session.token_hash = ? AND user.active = 1
           AND session.last_seen_at > ? AND session.created_at > ?",
    )
    .bind(&token_hash)
    .bind(now - SESSION_IDLE_SECS)
    .bind(now - SESSION_MAX_SECS)
    .fetch_optional(&app.db)
    .await?;
    let Some((id, name, role, csrf_token, last_seen_at)) = row else {
        return Ok(None);
    };
    if now - last_seen_at > 60 {
        sqlx::query("UPDATE session SET last_seen_at = ? WHERE token_hash = ?")
            .bind(now)
            .bind(&token_hash)
            .execute(&app.db)
            .await?;
    }
    Ok(Some(Current {
        user: User { id, name, role },
        token_hash,
        csrf_token,
    }))
}

pub struct NewSession {
    /// The `Set-Cookie` value that carries the session.
    pub cookie: HeaderValue,
    pub csrf_token: String,
}

pub async fn start_session(
    db: impl SqliteExecutor<'_>,
    app: &App,
    user_id: i64,
) -> Result<NewSession, sqlx::Error> {
    let token = random_token();
    let csrf_token = random_token();
    let now = now();
    sqlx::query(
        "INSERT INTO session (token_hash, user_id, csrf_token, created_at, last_seen_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(digest(token.as_bytes()))
    .bind(user_id)
    .bind(&csrf_token)
    .bind(now)
    .bind(now)
    .execute(db)
    .await?;
    Ok(NewSession {
        cookie: session_cookie(app, &token, SESSION_MAX_SECS),
        csrf_token,
    })
}

pub fn clear_session_cookie(app: &App) -> HeaderValue {
    session_cookie(app, "", 0)
}

fn session_cookie(app: &App, token: &str, max_age: i64) -> HeaderValue {
    let secure = if app.public_url.scheme() == "https" {
        "; Secure"
    } else {
        ""
    };
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
    )
    .parse()
    .expect("cookie is ASCII")
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// Stores the state of a started exchange and returns the id its finishing request must present.
pub async fn start_ceremony(
    app: &App,
    kind: &str,
    state: &impl Serialize,
) -> Result<String, ApiError> {
    let id = random_token();
    store_ceremony(app, &id, kind, state).await?;
    Ok(id)
}

/// Like [`start_ceremony`] for an exchange whose id must be known before its state.
pub async fn store_ceremony(
    app: &App,
    id: &str,
    kind: &str,
    state: &impl Serialize,
) -> Result<(), ApiError> {
    let now = now();
    sqlx::query("DELETE FROM ceremony WHERE expires_at <= ?")
        .bind(now)
        .execute(&app.db)
        .await?;
    sqlx::query("INSERT INTO ceremony (id_hash, kind, state, expires_at) VALUES (?, ?, ?, ?)")
        .bind(digest(id.as_bytes()))
        .bind(kind)
        .bind(serde_json::to_string(state).map_err(ApiError::internal)?)
        .bind(now + CEREMONY_SECS)
        .execute(&app.db)
        .await?;
    Ok(())
}

/// Consumes a started exchange; each id can be used once.
pub async fn take_ceremony<T: DeserializeOwned>(
    app: &App,
    kind: &str,
    id: &str,
) -> Result<T, ApiError> {
    let state: Option<String> = sqlx::query_scalar(
        "DELETE FROM ceremony WHERE id_hash = ? AND kind = ? AND expires_at > ? RETURNING state",
    )
    .bind(digest(id.as_bytes()))
    .bind(kind)
    .bind(now())
    .fetch_optional(&app.db)
    .await?;
    let state = state.ok_or(ApiError::new(ErrorCode::CeremonyExpired))?;
    serde_json::from_str(&state).map_err(ApiError::internal)
}

pub fn random_token() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

pub fn digest(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

pub fn now() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs();
    i64::try_from(secs).expect("seconds fit in i64")
}
