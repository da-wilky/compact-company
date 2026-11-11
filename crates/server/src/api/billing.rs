use std::{collections::BTreeMap, sync::Arc};

use axum::extract::State;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{
    documents::{Document, business, draft_row, find, snapshot_lines, write_all_lines},
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    documents::{self, Kind, Unit, billed_minutes, hours},
    money::Money,
    rates,
    settings::Business,
};

/// How time entries become invoice lines; entries with different rates never share a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    /// A line per entry.
    Entry,
    Project,
    Todo,
    Activity,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeInput {
    /// ISO date of the first day whose time to bill; all earlier unbilled time when left out.
    from: Option<String>,
    /// ISO date of the last day, inclusive.
    to: Option<String>,
    /// Only this project's time.
    project_id: Option<i64>,
    group: Grouping,
    /// Minutes each entry is rounded up to: 0, 5, 10, 15 or 30.
    #[serde(default)]
    rounding: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct UnbilledProject {
    customer_id: i64,
    customer_name: String,
    project_id: i64,
    project_name: String,
    entries: i64,
    /// Billable minutes, each entry to the minute.
    minutes: i64,
    /// What the time is worth at today's rates; none when no rate applies.
    #[sqlx(skip)]
    amount: Option<Money>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UnbilledFilter {
    customer_id: Option<i64>,
}

/// Billable time, ended and on no invoice, per project.
#[utoipa::path(get, path = "/unbilled", tag = "documents", params(UnbilledFilter),
    responses((status = OK, body = Vec<UnbilledProject>), (status = "default", body = ApiError)))]
pub async fn list_unbilled(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(filter): Query<UnbilledFilter>,
) -> Result<Json<Vec<UnbilledProject>>, ApiError> {
    Ok(Json(
        unbilled(&mut *app.db.acquire().await?, filter.customer_id).await?,
    ))
}

pub async fn unbilled(
    db: &mut sqlx::SqliteConnection,
    customer_id: Option<i64>,
) -> Result<Vec<UnbilledProject>, ApiError> {
    let mut list: Vec<UnbilledProject> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT v.customer_id, v.customer_name, v.project_id, v.project_name,
         count(*) AS entries, sum({MINUTES}) AS minutes
         FROM time_entry_view v WHERE {UNBILLED} AND (?1 IS NULL OR v.customer_id = ?1)
         GROUP BY v.project_id ORDER BY v.customer_name, v.project_name"
    )))
    .bind(customer_id)
    .fetch_all(&mut *db)
    .await?;
    for project in &mut list {
        let parts: Vec<(Option<i64>, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT v.activity_id, sum({MINUTES}) FROM time_entry_view v
             WHERE {UNBILLED} AND v.project_id = ? GROUP BY v.activity_id"
        )))
        .bind(project.project_id)
        .fetch_all(&mut *db)
        .await?;
        for (activity, minutes) in parts {
            if let Some(rate) = rates::resolve(&mut *db, project.project_id, activity).await? {
                let worth = (Decimal::from(rate.minor) * Decimal::from(minutes)
                    / Decimal::from(60))
                .round()
                .try_into()
                .unwrap_or(i64::MAX);
                let amount = project.amount.get_or_insert(Money {
                    minor: 0,
                    currency: rate.currency,
                });
                amount.minor += worth;
            }
        }
    }
    Ok(list)
}

const UNBILLED: &str = "v.billable AND v.ended_at IS NOT NULL
    AND NOT EXISTS (SELECT 1 FROM line_time_entry x JOIN billed_line l ON l.id = x.line_id
        WHERE x.time_entry_id = v.id)";
const MINUTES: &str = "(coalesce(v.billable_secs, v.ended_at - v.started_at) + 30) / 60";

/// Adds the customer's unbilled time to a draft invoice as lines priced at the hourly rate when
/// issued.
#[utoipa::path(post, path = "/documents/{id}/time", tag = "documents",
    params(("id" = i64, Path)), request_body = TimeInput,
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn add_time(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<TimeInput>,
) -> Result<Json<Document>, ApiError> {
    let business = business(&app).await?;
    let mut check = Check::default();
    if ![0, 5, 10, 15, 30].contains(&input.rounding) {
        check.fail("rounding", FieldError::InvalidFormat);
    }
    let from = bound(&business, &mut check, "from", input.from.as_deref(), 0);
    let to = bound(&business, &mut check, "to", input.to.as_deref(), 1);
    check.done()?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let row = draft_row(&mut tx, id).await?;
    if row.kind != Kind::Invoice {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    let entries: Vec<Entry> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT v.id, v.project_id, v.project_name, v.todo_id, v.todo_title, v.activity_id,
         v.activity_name, v.description, v.started_at,
         coalesce(v.billable_secs, v.ended_at - v.started_at) AS secs
         FROM time_entry_view v WHERE {UNBILLED} AND v.customer_id = ?1
         AND (?2 IS NULL OR v.project_id = ?2) AND (?3 IS NULL OR v.started_at >= ?3)
         AND (?4 IS NULL OR v.started_at < ?4) ORDER BY v.started_at"
    )))
    .bind(row.customer_id)
    .bind(input.project_id)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *tx)
    .await?;
    if entries.is_empty() {
        return Err(ApiError::new(ErrorCode::NoUnbilledTime));
    }
    let mut groups: BTreeMap<(i64, i64, i64, Option<i64>), Vec<Entry>> = BTreeMap::new();
    for entry in entries {
        let (order, group) = match input.group {
            Grouping::Entry => (entry.started_at, entry.id),
            Grouping::Project => (0, 0),
            Grouping::Todo => (0, entry.todo_id.unwrap_or(0)),
            Grouping::Activity => (entry.activity_id.unwrap_or(0), 0),
        };
        groups
            .entry((order, group, entry.project_id, entry.activity_id))
            .or_default()
            .push(entry);
    }
    let mut lines = snapshot_lines(&mut tx, id).await?;
    for entries in groups.into_values() {
        let first = &entries[0];
        let minutes: i64 = entries
            .iter()
            .map(|e| billed_minutes(e.secs, input.rounding))
            .sum();
        let what = match input.group {
            Grouping::Entry => {
                let text = first
                    .todo_title
                    .clone()
                    .or_else(|| first.description.clone())
                    .unwrap_or_else(|| first.project_name.clone());
                format!("{}: {text}", documents::day(&business, first.started_at))
            }
            Grouping::Todo => first
                .todo_title
                .clone()
                .unwrap_or_else(|| first.project_name.clone()),
            Grouping::Project | Grouping::Activity => first.project_name.clone(),
        };
        let description = match &first.activity_name {
            Some(activity) if input.group == Grouping::Activity => format!("{activity}: {what}"),
            Some(activity) => format!("{what} ({activity})"),
            None => what,
        };
        lines.push(super::documents::Line::from_time(
            description,
            hours(minutes),
            Unit::Hour,
            first.project_id,
            first.activity_id,
            entries.iter().map(|e| e.id).collect(),
        ));
    }
    write_all_lines(&mut tx, id, &lines).await?;
    sqlx::query("UPDATE document SET rounding = ?, updated_at = ? WHERE id = ?")
        .bind(input.rounding)
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "document.time",
        &format!("document:{id}"),
        None,
        Some(serde_json::json!({ "lines": lines.len(), "rounding": input.rounding })),
    )
    .await?;
    let document = find(&mut tx, &business, id).await?;
    tx.commit().await?;
    Ok(Json(document))
}

#[derive(Debug, sqlx::FromRow)]
struct Entry {
    id: i64,
    project_id: i64,
    project_name: String,
    todo_id: Option<i64>,
    todo_title: Option<String>,
    activity_id: Option<i64>,
    activity_name: Option<String>,
    description: Option<String>,
    started_at: i64,
    secs: i64,
}

/// Midnight starting the given ISO day (plus `days`) in the business's timezone, as Unix seconds.
fn bound(
    business: &Business,
    check: &mut Check,
    key: &str,
    day: Option<&str>,
    days: i64,
) -> Option<i64> {
    let day = day.filter(|d| !d.is_empty())?;
    let Ok(date) = day.parse::<jiff::civil::Date>() else {
        check.fail(key, FieldError::InvalidFormat);
        return None;
    };
    date.saturating_add(jiff::Span::new().days(days))
        .to_zoned(documents::timezone(business))
        .ok()
        .map(|z| z.timestamp().as_second())
}
