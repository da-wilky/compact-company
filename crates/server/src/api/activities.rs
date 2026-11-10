use std::{collections::BTreeSet, sync::Arc};

use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, now},
    money::{self, Money},
};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActivityInput {
    name: Option<String>,
    /// At most one rate per currency, each in that currency's minor unit.
    #[serde(default)]
    rates: Vec<Money>,
    #[serde(default)]
    archived: bool,
}

/// What an edit may change; also the snapshot the audit log compares.
#[derive(Debug, Serialize, ToSchema)]
pub struct ActivityFields {
    name: String,
    rates: Vec<Money>,
    archived: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Activity {
    id: i64,
    #[serde(flatten)]
    fields: ActivityFields,
    created_at: i64,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ActivityFilter {
    /// Archived activities instead of active ones.
    #[serde(default)]
    archived: bool,
}

#[utoipa::path(get, path = "/activities", tag = "projects", params(ActivityFilter),
    responses((status = OK, body = Vec<Activity>), (status = "default", body = ApiError)))]
pub async fn list_activities(
    State(app): State<Arc<App>>,
    _: Auth,
    Query(filter): Query<ActivityFilter>,
) -> Result<Json<Vec<Activity>>, ApiError> {
    let mut db = app.db.acquire().await?;
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM activity WHERE archived = ? ORDER BY name COLLATE NOCASE")
            .bind(filter.archived)
            .fetch_all(&mut *db)
            .await?;
    let mut activities = Vec::with_capacity(ids.len());
    for id in ids {
        activities.push(find(&mut db, id).await?);
    }
    Ok(Json(activities))
}

#[utoipa::path(post, path = "/activities", tag = "projects", request_body = ActivityInput,
    responses((status = OK, body = Activity), (status = "default", body = ApiError)))]
pub async fn create_activity(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<ActivityInput>,
) -> Result<Json<Activity>, ApiError> {
    let fields = validate(input)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO activity (name, archived, created_at) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(&fields.name)
    .bind(fields.archived)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    store_rates(&mut tx, id, &fields.rates).await?;
    let activity = find(&mut tx, id).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "activity.create",
        &format!("activity:{id}"),
        None,
        Some(serde_json::to_value(&activity).map_err(ApiError::internal)?),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(activity))
}

#[utoipa::path(put, path = "/activities/{id}", tag = "projects", request_body = ActivityInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Activity), (status = "default", body = ApiError)))]
pub async fn put_activity(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<ActivityInput>,
) -> Result<Json<Activity>, ApiError> {
    let fields = validate(input)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    sqlx::query("UPDATE activity SET name = ?, archived = ? WHERE id = ?")
        .bind(&fields.name)
        .bind(fields.archived)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    store_rates(&mut tx, id, &fields.rates).await?;
    if let Some((old, new)) = audit::changes(&before.fields, &fields) {
        audit::record(
            &mut tx,
            auth.user.id,
            "activity.update",
            &format!("activity:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    let activity = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(activity))
}

async fn store_rates(db: &mut SqliteConnection, id: i64, rates: &[Money]) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM activity_rate WHERE activity_id = ?")
        .bind(id)
        .execute(&mut *db)
        .await?;
    for rate in rates {
        sqlx::query("INSERT INTO activity_rate (activity_id, currency, hourly_rate) VALUES (?, ?, ?)")
            .bind(id)
            .bind(&rate.currency)
            .bind(rate.minor)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}

pub async fn find(db: &mut SqliteConnection, id: i64) -> Result<Activity, ApiError> {
    let (name, archived, created_at): (String, bool, i64) =
        sqlx::query_as("SELECT name, archived, created_at FROM activity WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let rates = sqlx::query_as::<_, (String, i64)>(
        "SELECT currency, hourly_rate FROM activity_rate WHERE activity_id = ? ORDER BY currency",
    )
    .bind(id)
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|(currency, minor)| Money { minor, currency })
    .collect();
    Ok(Activity {
        id,
        fields: ActivityFields {
            name,
            rates,
            archived,
        },
        created_at,
    })
}

fn validate(input: ActivityInput) -> Result<ActivityFields, ApiError> {
    let mut check = Check::default();
    let mut seen = BTreeSet::new();
    let mut rates = input.rates;
    rates.sort_by(|a, b| a.currency.cmp(&b.currency));
    for rate in &rates {
        if !money::is_currency(&rate.currency) || rate.minor < 0 || !seen.insert(&rate.currency) {
            check.fail("rates", FieldError::InvalidFormat);
        }
    }
    let fields = ActivityFields {
        name: check.required("name", input.name, 200),
        rates,
        archived: input.archived,
    };
    check.done()?;
    Ok(fields)
}
