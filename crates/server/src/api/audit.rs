use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use super::error::{ApiError, Json, Query};
use crate::{
    App,
    auth::{Auth, OwnerOrAccountant},
};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditFilter {
    /// `user:3` for one subject, or `user` for every user.
    subject: Option<String>,
    user_id: Option<i64>,
    action: Option<String>,
    /// Unix seconds, inclusive.
    from: Option<i64>,
    /// Unix seconds, exclusive.
    to: Option<i64>,
    /// Only entries older than this id, to page backwards.
    before_id: Option<i64>,
    /// At most 200; 100 by default.
    limit: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct AuditEntry {
    id: i64,
    at: i64,
    user_id: i64,
    user_name: String,
    action: String,
    subject: String,
    before: Option<Value>,
    after: Option<Value>,
}

type Row = (
    i64,
    i64,
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

/// Newest first.
#[utoipa::path(get, path = "/audit", tag = "audit", params(AuditFilter),
    responses((status = OK, body = Vec<AuditEntry>), (status = "default", body = ApiError)))]
pub async fn list_audit(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(filter): Query<AuditFilter>,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT audit.id, audit.at, audit.user_id, user.name, audit.action, audit.subject,
                audit.before, audit.after
         FROM audit JOIN user ON user.id = audit.user_id
         WHERE (?1 IS NULL OR audit.subject = ?1 OR audit.subject LIKE ?1 || ':%')
           AND (?2 IS NULL OR audit.user_id = ?2)
           AND (?3 IS NULL OR audit.action = ?3)
           AND (?4 IS NULL OR audit.at >= ?4)
           AND (?5 IS NULL OR audit.at < ?5)
           AND (?6 IS NULL OR audit.id < ?6)
         ORDER BY audit.id DESC LIMIT ?7",
    )
    .bind(&filter.subject)
    .bind(filter.user_id)
    .bind(&filter.action)
    .bind(filter.from)
    .bind(filter.to)
    .bind(filter.before_id)
    .bind(filter.limit.unwrap_or(100).clamp(1, 200))
    .fetch_all(&app.db)
    .await?;
    let parse = |text: Option<String>| -> Result<Option<Value>, ApiError> {
        text.map(|t| serde_json::from_str(&t))
            .transpose()
            .map_err(ApiError::internal)
    };
    let entries = rows
        .into_iter()
        .map(
            |(id, at, user_id, user_name, action, subject, before, after)| {
                Ok(AuditEntry {
                    id,
                    at,
                    user_id,
                    user_name,
                    action,
                    subject,
                    before: parse(before)?,
                    after: parse(after)?,
                })
            },
        )
        .collect::<Result<_, ApiError>>()?;
    Ok(Json(entries))
}
