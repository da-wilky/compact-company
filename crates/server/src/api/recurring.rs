use std::sync::Arc;

use axum::{extract::State, http::StatusCode};
use jiff::{ToSpan, civil::Date};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use super::{
    documents::{DocumentInput, LineInput, business, insert_draft, today, validate},
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    documents::{DecimalText, Kind, Status, Unit},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Interval {
    Monthly,
    Quarterly,
    Yearly,
}

/// Which interval a draft's service period is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Period {
    /// The interval starting on the date.
    Advance,
    /// The interval ending the day before the date.
    Arrears,
}

impl Interval {
    fn months(self) -> i32 {
        match self {
            Self::Monthly => 1,
            Self::Quarterly => 3,
            Self::Yearly => 12,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecurringInput {
    customer_id: i64,
    project_id: Option<i64>,
    title: Option<String>,
    intro: Option<String>,
    outro: Option<String>,
    interval: Interval,
    period: Period,
    /// ISO date of the next draft; the dates after it follow at the interval.
    next_date: String,
    /// ISO date of the last date drafted, if any.
    ends_on: Option<String>,
    #[serde(default)]
    paused: bool,
    /// Without time entries.
    lines: Vec<LineInput>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Recurring {
    id: i64,
    customer_id: i64,
    customer_name: String,
    project_id: Option<i64>,
    title: Option<String>,
    intro: Option<String>,
    outro: Option<String>,
    interval: Interval,
    period: Period,
    /// ISO date the next draft is created on; none once past the end.
    next_date: Option<String>,
    ends_on: Option<String>,
    paused: bool,
    lines: Vec<RecurringLine>,
    /// The invoices it produced, newest first.
    invoices: Vec<Produced>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct RecurringLine {
    description: String,
    #[schema(value_type = String)]
    quantity: DecimalText,
    unit: Unit,
    unit_price: Option<i64>,
    #[schema(value_type = Option<String>)]
    vat_rate: Option<DecimalText>,
    project_id: Option<i64>,
    activity_id: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Produced {
    id: i64,
    status: Status,
    number: Option<String>,
    /// ISO date it was drafted for.
    recurring_date: String,
}

#[derive(Debug, sqlx::FromRow)]
struct Row {
    id: i64,
    customer_id: i64,
    project_id: Option<i64>,
    title: Option<String>,
    intro: Option<String>,
    outro: Option<String>,
    interval: Interval,
    period: Period,
    starts_on: String,
    occurrences: i64,
    ends_on: Option<String>,
    paused: bool,
    created_by: i64,
}

fn parse(date: &str) -> Date {
    date.parse().expect("stored dates are ISO dates")
}

impl Row {
    /// The `n`th date counted from the start, so that the 31st stays the 31st where a month has it.
    fn date(&self, n: i64) -> Date {
        let months = i32::try_from(n).expect("occurrences stay small") * self.interval.months();
        parse(&self.starts_on)
            .checked_add(months.months())
            .expect("dates stay within jiff's range")
    }

    fn next(&self) -> Option<Date> {
        let next = self.date(self.occurrences);
        match &self.ends_on {
            Some(end) if next > parse(end) => None,
            _ => Some(next),
        }
    }

    /// The service period of the draft for the current occurrence.
    fn service_period(&self) -> (Date, Date) {
        let n = self.occurrences;
        let day = 1.day();
        match self.period {
            Period::Advance => (self.date(n), self.date(n + 1) - day),
            Period::Arrears => (self.date(n - 1), self.date(n) - day),
        }
    }
}

async fn load(db: &mut SqliteConnection, id: i64) -> Result<Row, ApiError> {
    sqlx::query_as("SELECT * FROM recurring_invoice WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}

async fn find(db: &mut SqliteConnection, id: i64) -> Result<Recurring, ApiError> {
    let row = load(db, id).await?;
    let customer_name: String = sqlx::query_scalar("SELECT name FROM customer WHERE id = ?")
        .bind(row.customer_id)
        .fetch_one(&mut *db)
        .await?;
    let lines = sqlx::query_as(
        "SELECT description, quantity, unit, unit_price, vat_rate, project_id, activity_id
         FROM recurring_line WHERE recurring_id = ? ORDER BY position",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    let invoices = sqlx::query_as(
        "SELECT id, status, number, recurring_date FROM document WHERE recurring_id = ?
         ORDER BY recurring_date DESC",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    Ok(Recurring {
        id,
        customer_id: row.customer_id,
        customer_name,
        project_id: row.project_id,
        next_date: row.next().map(|d| d.to_string()),
        title: row.title,
        intro: row.intro,
        outro: row.outro,
        interval: row.interval,
        period: row.period,
        ends_on: row.ends_on,
        paused: row.paused,
        lines,
        invoices,
    })
}

#[utoipa::path(get, path = "/recurring-invoices", tag = "documents",
    responses((status = OK, body = Vec<Recurring>), (status = "default", body = ApiError)))]
pub async fn list_recurring(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
) -> Result<Json<Vec<Recurring>>, ApiError> {
    let mut db = app.db.acquire().await?;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM recurring_invoice ORDER BY id")
        .fetch_all(&mut *db)
        .await?;
    let mut list = Vec::new();
    for id in ids {
        list.push(find(&mut db, id).await?);
    }
    Ok(Json(list))
}

#[utoipa::path(get, path = "/recurring-invoices/{id}", tag = "documents",
    params(("id" = i64, Path)),
    responses((status = OK, body = Recurring), (status = "default", body = ApiError)))]
pub async fn get_recurring(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(id): Path<i64>,
) -> Result<Json<Recurring>, ApiError> {
    Ok(Json(find(&mut *app.db.acquire().await?, id).await?))
}

/// Sets up a recurring invoice; dates already past are drafted at once.
#[utoipa::path(post, path = "/recurring-invoices", tag = "documents",
    request_body = RecurringInput,
    responses((status = OK, body = Recurring), (status = "default", body = ApiError)))]
pub async fn create_recurring(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<RecurringInput>,
) -> Result<Json<Recurring>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO recurring_invoice (customer_id, interval, period, starts_on, created_by,
         created_at, updated_at) VALUES (?, 'monthly', 'advance', '2000-01-01', ?, ?, ?)
         RETURNING id",
    )
    .bind(input.customer_id)
    .bind(auth.user.id)
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    let after = write(&mut tx, id, input).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "recurring.create",
        &format!("recurring:{id}"),
        None,
        Some(after),
    )
    .await?;
    tx.commit().await?;
    draft_due(&app, today(&business(&app).await?)?).await?;
    Ok(Json(find(&mut *app.db.acquire().await?, id).await?))
}

/// Changes a recurring invoice; drafts already created stay as they are. A new next date
/// restarts the dates from it.
#[utoipa::path(put, path = "/recurring-invoices/{id}", tag = "documents",
    params(("id" = i64, Path)), request_body = RecurringInput,
    responses((status = OK, body = Recurring), (status = "default", body = ApiError)))]
pub async fn put_recurring(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<RecurringInput>,
) -> Result<Json<Recurring>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = serde_json::to_value(find(&mut tx, id).await?).map_err(ApiError::internal)?;
    let after = write(&mut tx, id, input).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "recurring.update",
        &format!("recurring:{id}"),
        Some(before),
        Some(after),
    )
    .await?;
    tx.commit().await?;
    draft_due(&app, today(&business(&app).await?)?).await?;
    Ok(Json(find(&mut *app.db.acquire().await?, id).await?))
}

/// Only a recurring invoice that has produced nothing can be deleted; otherwise end it.
#[utoipa::path(delete, path = "/recurring-invoices/{id}", tag = "documents",
    params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn delete_recurring(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    if !before.invoices.is_empty() {
        return Err(ApiError::new(ErrorCode::HasInvoices));
    }
    sqlx::query("DELETE FROM recurring_invoice WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "recurring.delete",
        &format!("recurring:{id}"),
        Some(serde_json::to_value(before).map_err(ApiError::internal)?),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Validates `input` as an invoice draft would be and stores it as recurring invoice `id`.
async fn write(
    tx: &mut SqliteConnection,
    id: i64,
    input: RecurringInput,
) -> Result<serde_json::Value, ApiError> {
    let mut check = Check::default();
    let last: Option<String> =
        sqlx::query_scalar("SELECT max(recurring_date) FROM document WHERE recurring_id = ?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    // dates count from it, so it bounds how far back drafts go; a date already drafted would
    // be billed twice
    let next = input
        .next_date
        .parse::<Date>()
        .ok()
        .filter(|d| d.year() >= 2000 && last.as_ref().is_none_or(|l| *d > parse(l)));
    if next.is_none() {
        check.fail("next_date", FieldError::InvalidFormat);
    }
    let ends_on = input.ends_on.filter(|d| !d.is_empty());
    match ends_on.as_deref().map(str::parse::<Date>) {
        Some(Err(_)) => check.fail("ends_on", FieldError::InvalidFormat),
        Some(Ok(end)) if next.is_some_and(|next| end < next) => {
            check.fail("ends_on", FieldError::InvalidFormat);
        }
        _ => {}
    }
    for (i, line) in input.lines.iter().enumerate() {
        if !line.time_entry_ids.is_empty() {
            check.fail(
                &format!("lines.{i}.time_entry_ids"),
                FieldError::InvalidFormat,
            );
        }
    }
    if input.lines.is_empty() {
        check.fail("lines", FieldError::Required);
    }
    check.done()?;
    let draft = validate(
        tx,
        DocumentInput {
            kind: Kind::Invoice,
            customer_id: input.customer_id,
            project_id: input.project_id,
            language: None,
            title: input.title,
            intro: input.intro,
            outro: input.outro,
            valid_until: None,
            pricing: None,
            service_from: None,
            service_to: None,
            timesheet: false,
            invoice_type: None,
            lines: input.lines,
        },
        None,
    )
    .await?;
    let next = next.expect("checked above").to_string();
    let current = load(tx, id).await?;
    let unchanged = current.next().map(|d| d.to_string()).as_deref() == Some(next.as_str())
        && current.interval == input.interval;
    let (starts_on, occurrences) = if unchanged {
        (current.starts_on, current.occurrences)
    } else {
        (next, 0)
    };
    sqlx::query(
        "UPDATE recurring_invoice SET customer_id = ?, project_id = ?, title = ?, intro = ?,
         outro = ?, interval = ?, period = ?, starts_on = ?, occurrences = ?, ends_on = ?,
         paused = ?, updated_at = ? WHERE id = ?",
    )
    .bind(draft.customer_id)
    .bind(draft.project_id)
    .bind(&draft.title)
    .bind(&draft.intro)
    .bind(&draft.outro)
    .bind(input.interval)
    .bind(input.period)
    .bind(starts_on)
    .bind(occurrences)
    .bind(&ends_on)
    .bind(input.paused)
    .bind(now())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM recurring_line WHERE recurring_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for (position, line) in draft.lines.iter().enumerate() {
        sqlx::query(
            "INSERT INTO recurring_line (recurring_id, position, description, quantity, unit,
             unit_price, vat_rate, project_id, activity_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(i64::try_from(position).expect("lines are capped"))
        .bind(&line.description)
        .bind(line.quantity.to_string())
        .bind(line.unit)
        .bind(line.unit_price)
        .bind(line.vat_rate.map(|v| v.to_string()))
        .bind(line.project_id)
        .bind(line.activity_id)
        .execute(&mut *tx)
        .await?;
    }
    serde_json::to_value(find(tx, id).await?).map_err(ApiError::internal)
}

/// Drafts every date of every running recurring invoice up to `today`, each once, and moves its
/// next date on. One whose draft no longer validates, such as for an archived customer, is paused.
pub async fn draft_due(app: &App, today: Date) -> Result<usize, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let rows: Vec<Row> =
        sqlx::query_as("SELECT * FROM recurring_invoice WHERE NOT paused ORDER BY id")
            .fetch_all(&mut *tx)
            .await?;
    let mut drafted = 0;
    for mut row in rows {
        while let Some(date) = row.next().filter(|d| *d <= today) {
            match draft(&mut tx, &row, date).await {
                Ok(_) => drafted += 1,
                Err(e) if e.code == ErrorCode::Internal => return Err(e),
                Err(e) => {
                    sqlx::query("UPDATE recurring_invoice SET paused = 1 WHERE id = ?")
                        .bind(row.id)
                        .execute(&mut *tx)
                        .await?;
                    audit::record(
                        &mut tx,
                        row.created_by,
                        "recurring.pause",
                        &format!("recurring:{}", row.id),
                        None,
                        Some(serde_json::json!({ "date": date.to_string(), "error": e })),
                    )
                    .await?;
                    break;
                }
            }
            row.occurrences += 1;
            sqlx::query("UPDATE recurring_invoice SET occurrences = ? WHERE id = ?")
                .bind(row.occurrences)
                .bind(row.id)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(drafted)
}

async fn draft(tx: &mut SqliteConnection, row: &Row, date: Date) -> Result<i64, ApiError> {
    let lines: Vec<RecurringLine> = sqlx::query_as(
        "SELECT description, quantity, unit, unit_price, vat_rate, project_id, activity_id
         FROM recurring_line WHERE recurring_id = ? ORDER BY position",
    )
    .bind(row.id)
    .fetch_all(&mut *tx)
    .await?;
    let (from, to) = row.service_period();
    let draft = validate(
        tx,
        DocumentInput {
            kind: Kind::Invoice,
            customer_id: row.customer_id,
            project_id: row.project_id,
            language: None,
            title: row.title.clone(),
            intro: row.intro.clone(),
            outro: row.outro.clone(),
            valid_until: None,
            pricing: None,
            service_from: Some(from.to_string()),
            service_to: Some(to.to_string()),
            timesheet: false,
            invoice_type: None,
            lines: lines
                .into_iter()
                .map(|l| LineInput {
                    description: Some(l.description),
                    quantity: l.quantity.0,
                    unit: l.unit,
                    unit_price: l.unit_price,
                    vat_rate: l.vat_rate.map(|v| v.0),
                    project_id: l.project_id,
                    activity_id: l.activity_id,
                    time_entry_ids: Vec::new(),
                    cancels_line_id: None,
                })
                .collect(),
        },
        None,
    )
    .await?;
    let id = insert_draft(tx, row.created_by, &draft).await?;
    sqlx::query("UPDATE document SET recurring_id = ?, recurring_date = ? WHERE id = ?")
        .bind(row.id)
        .bind(date.to_string())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    Ok(id)
}

/// A draft from a recurring invoice, waiting to be issued.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct WaitingDraft {
    id: i64,
    customer_name: String,
    title: Option<String>,
    /// ISO date it was drafted for.
    recurring_date: String,
}

pub async fn waiting(db: &mut SqliteConnection) -> Result<Vec<WaitingDraft>, sqlx::Error> {
    sqlx::query_as(
        "SELECT d.id, c.name AS customer_name, d.title, d.recurring_date FROM document d
         JOIN customer c ON c.id = d.customer_id
         WHERE d.recurring_id IS NOT NULL AND d.status = 'draft' ORDER BY d.recurring_date, d.id",
    )
    .fetch_all(db)
    .await
}
