use std::{collections::BTreeMap, sync::Arc};

use axum::{
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    customers::Language,
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::{Check, field_error},
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    books,
    documents::{self, Data, Kind, LineRow, Pricing, Row, Status, Unit},
    einvoice, layouts, render, rules,
    settings::Business,
};

const MAX_LINES: usize = 500;
const MAX_AMOUNT: i64 = 100_000_000_000;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentInput {
    /// Fixed once the draft exists.
    kind: Kind,
    customer_id: i64,
    project_id: Option<i64>,
    /// The customer's language when left out.
    language: Option<Language>,
    title: Option<String>,
    intro: Option<String>,
    outro: Option<String>,
    /// Quotes only: ISO date until which the quote stands; 30 days after issuing when left out.
    valid_until: Option<String>,
    /// Quotes only; fixed when left out.
    pricing: Option<Pricing>,
    /// Invoices only: ISO dates of the service period when it has no time entries.
    service_from: Option<String>,
    service_to: Option<String>,
    /// Invoices only: whether a timesheet of its time entries is attached.
    #[serde(default)]
    timesheet: bool,
    #[serde(default)]
    lines: Vec<LineInput>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LineInput {
    description: Option<String>,
    #[schema(value_type = String)]
    quantity: Decimal,
    unit: Unit,
    /// In the minor unit of the document's currency; without one, a line with a project is
    /// priced at the hourly rate when issued.
    unit_price: Option<i64>,
    /// Percent; the business default when left out.
    #[schema(value_type = Option<String>)]
    vat_rate: Option<Decimal>,
    project_id: Option<i64>,
    activity_id: Option<i64>,
    /// Invoices only: the billable time entries this line charges.
    #[serde(default)]
    time_entry_ids: Vec<i64>,
    /// Cancellation invoices only: the invoice line it reverses; its text and amounts are
    /// taken from that line.
    cancels_line_id: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Document {
    id: i64,
    kind: Kind,
    status: Status,
    customer_id: i64,
    customer_name: String,
    project_id: Option<i64>,
    language: Language,
    currency: String,
    title: Option<String>,
    intro: Option<String>,
    outro: Option<String>,
    number: Option<String>,
    issue_date: Option<String>,
    lines: Vec<LineRow>,
    /// What the layout renders: as issued, or for a draft as it would be issued now.
    data: Data,
    /// What must be filled in before the draft can be issued; empty once issued.
    missing: BTreeMap<String, FieldError>,
    valid_until: Option<String>,
    pricing: Option<Pricing>,
    /// The document this one follows from.
    source_id: Option<i64>,
    due_date: Option<String>,
    service_from: Option<String>,
    service_to: Option<String>,
    timesheet: bool,
    /// Minutes each time entry is rounded up to.
    rounding: i64,
    quote_state: Option<QuoteState>,
    /// Documents that follow from this one.
    follow_ups: Vec<FollowUp>,
    /// Every attempt to email it, newest first.
    sent: Vec<super::sending::Sent>,
    /// Money received, for an issued invoice.
    payments: Vec<super::overview::Payment>,
    /// What is still owed on an issued invoice.
    amount_due: Option<crate::money::Money>,
    /// The uploaded layout it was issued with; none for the built-in one.
    layout: Option<String>,
    issued_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct DocumentSummary {
    id: i64,
    kind: Kind,
    status: Status,
    number: Option<String>,
    customer_id: i64,
    customer_name: String,
    title: Option<String>,
    issue_date: Option<String>,
    valid_until: Option<String>,
    quote_state: Option<QuoteState>,
    currency: String,
    /// Major units; none for documents without prices.
    #[sqlx(skip)]
    #[schema(value_type = Option<String>)]
    gross: Option<Decimal>,
    updated_at: i64,
}

/// Where an issued quote stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum QuoteState {
    Open,
    Accepted,
    Declined,
    Expired,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct FollowUp {
    id: i64,
    kind: Kind,
    status: Status,
    number: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OutcomeInput {
    /// `accepted` or `declined`.
    outcome: QuoteState,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FollowUpInput {
    kind: Kind,
}

/// SQL for an issued quote's state, given the document as `d` and today as `?`.
const QUOTE_STATE: &str = "CASE WHEN d.kind = 'quote' AND d.status = 'issued' THEN coalesce(
    (SELECT outcome FROM quote_outcome WHERE document_id = d.id),
    CASE WHEN d.valid_until < ? THEN 'expired' ELSE 'open' END) END";

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DocumentFilter {
    kind: Option<Kind>,
    status: Option<Status>,
    customer_id: Option<i64>,
}

#[utoipa::path(get, path = "/documents", tag = "documents", params(DocumentFilter),
    responses((status = OK, body = Vec<DocumentSummary>), (status = "default", body = ApiError)))]
pub async fn list_documents(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(filter): Query<DocumentFilter>,
) -> Result<Json<Vec<DocumentSummary>>, ApiError> {
    let mut db = app.db.acquire().await?;
    let business = business(&app).await?;
    let sql = format!(
        "SELECT d.id, d.kind, d.status, d.number, d.customer_id, c.name AS customer_name, d.title,
         d.issue_date, d.currency, d.valid_until, {QUOTE_STATE} AS quote_state, d.updated_at
         FROM document d JOIN customer c ON c.id = d.customer_id
         WHERE (?2 IS NULL OR d.kind = ?2) AND (?3 IS NULL OR d.status = ?3)
         AND (?4 IS NULL OR d.customer_id = ?4)
         ORDER BY d.status = 'issued', d.issue_date DESC, d.number DESC, d.updated_at DESC"
    );
    let mut list: Vec<DocumentSummary> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(today(&business)?.to_string())
        .bind(filter.kind)
        .bind(filter.status)
        .bind(filter.customer_id)
        .fetch_all(&mut *db)
        .await?;
    for summary in &mut list {
        let row = row(&mut db, summary.id).await?;
        let data = data(&mut db, &business, &row).await?;
        summary.gross = data.totals.map(|t| t.gross);
    }
    Ok(Json(list))
}

#[utoipa::path(get, path = "/documents/{id}", tag = "documents", params(("id" = i64, Path)),
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn get_document(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(id): Path<i64>,
) -> Result<Json<Document>, ApiError> {
    let business = business(&app).await?;
    Ok(Json(
        find(&mut *app.db.acquire().await?, &business, id).await?,
    ))
}

#[utoipa::path(post, path = "/documents", tag = "documents", request_body = DocumentInput,
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn create_document(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<DocumentInput>,
) -> Result<Json<Document>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    if matches!(input.kind, Kind::Cancellation | Kind::Dunning) {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    let draft = validate(&mut tx, input, None).await?;
    let id = insert_draft(&mut tx, auth.user.id, &draft).await?;
    let document = find(&mut tx, &business, id).await?;
    tx.commit().await?;
    Ok(Json(document))
}

/// A new draft with the same customer, texts and lines.
#[utoipa::path(post, path = "/documents/{id}/duplicate", tag = "documents",
    params(("id" = i64, Path)),
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn duplicate_document(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<Json<Document>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let source = row(&mut tx, id).await?;
    if source.kind == Kind::Dunning {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    let original = snapshot(&mut tx, &source).await?;
    let archived: bool = sqlx::query_scalar("SELECT archived FROM customer WHERE id = ?")
        .bind(original.customer_id)
        .fetch_one(&mut *tx)
        .await?;
    if archived {
        return Err(ApiError::new(ErrorCode::Archived));
    }
    let copy = Draft {
        valid_until: None,
        source_id: None,
        lines: original
            .lines
            .into_iter()
            .map(|line| Line {
                time_entry_ids: Vec::new(),
                ..line
            })
            .collect(),
        ..original
    };
    let copy_id = insert_draft(&mut tx, auth.user.id, &copy).await?;
    let document = find(&mut tx, &business, copy_id).await?;
    tx.commit().await?;
    Ok(Json(document))
}

/// A draft that follows from an issued document: an order confirmation, invoice, delivery note
/// or acceptance report for an accepted quote; an invoice, delivery note or acceptance report
/// for an order confirmation; a cancellation invoice, delivery note or acceptance report for an
/// invoice.
#[utoipa::path(post, path = "/documents/{id}/follow-up", tag = "documents",
    params(("id" = i64, Path)), request_body = FollowUpInput,
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn follow_up(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<FollowUpInput>,
) -> Result<Json<Document>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let source = row(&mut tx, id).await?;
    let allowed = match source.kind {
        Kind::Quote => &[
            Kind::OrderConfirmation,
            Kind::Invoice,
            Kind::DeliveryNote,
            Kind::AcceptanceReport,
        ][..],
        Kind::OrderConfirmation => &[Kind::Invoice, Kind::DeliveryNote, Kind::AcceptanceReport],
        Kind::Invoice => &[
            Kind::Cancellation,
            Kind::DeliveryNote,
            Kind::AcceptanceReport,
            Kind::Dunning,
        ],
        _ => &[],
    };
    if source.status != Status::Issued || !allowed.contains(&input.kind) {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    if source.kind == Kind::Quote {
        let outcome: Option<QuoteState> =
            sqlx::query_scalar("SELECT outcome FROM quote_outcome WHERE document_id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        if outcome != Some(QuoteState::Accepted) {
            return Err(ApiError::new(ErrorCode::QuoteNotAccepted));
        }
    }
    let original = snapshot(&mut tx, &source).await?;
    if input.kind == Kind::Dunning {
        let level = next_dunning_level(&mut tx, id).await?;
        let draft = Draft {
            kind: Kind::Dunning,
            title: None,
            intro: Some(dunning_intro(original.language, level).to_owned()),
            outro: None,
            valid_until: None,
            pricing: None,
            source_id: Some(id),
            service_from: None,
            service_to: None,
            timesheet: false,
            rounding: 0,
            dunning_level: Some(level),
            lines: Vec::new(),
            ..original
        };
        let dunning = insert_draft(&mut tx, auth.user.id, &draft).await?;
        let document = find(&mut tx, &business, dunning).await?;
        tx.commit().await?;
        return Ok(Json(document));
    }
    let source_data = data(&mut tx, &business, &source).await?;
    let priced = input.kind.priced();
    // an invoice for a quote by effort charges the time spent, added to it afterwards
    let by_effort = original.pricing == Some(Pricing::Effort) && input.kind == Kind::Invoice;
    let cancels = input.kind == Kind::Cancellation;
    let lines = if cancels {
        let wanted = unreversed(&mut tx, id).await?;
        if wanted.is_empty() {
            return Err(ApiError::new(ErrorCode::AlreadyCancelled));
        }
        reversal_lines(&mut tx, id, &wanted).await?
    } else {
        original
            .lines
            .into_iter()
            .filter(|_| !by_effort)
            .map(|line| Line {
                unit_price: line.unit_price.filter(|_| priced),
                vat_rate: line.vat_rate.filter(|_| priced),
                time_entry_ids: Vec::new(),
                ..line
            })
            .collect()
    };
    let draft = Draft {
        kind: input.kind,
        intro: None,
        outro: None,
        valid_until: None,
        pricing: None,
        source_id: Some(id),
        service_from: source_data.service_from.filter(|_| cancels),
        service_to: source_data.service_to.filter(|_| cancels),
        timesheet: false,
        lines,
        ..original
    };
    let follow_up_id = insert_draft(&mut tx, auth.user.id, &draft).await?;
    let document = find(&mut tx, &business, follow_up_id).await?;
    tx.commit().await?;
    Ok(Json(document))
}

/// Records that the customer accepted or declined an issued quote.
#[utoipa::path(post, path = "/documents/{id}/outcome", tag = "documents",
    params(("id" = i64, Path)), request_body = OutcomeInput,
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn set_outcome(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<OutcomeInput>,
) -> Result<Json<Document>, ApiError> {
    if !matches!(input.outcome, QuoteState::Accepted | QuoteState::Declined) {
        return Err(field_error("outcome", FieldError::InvalidFormat));
    }
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let quote = row(&mut tx, id).await?;
    if quote.kind != Kind::Quote || quote.status != Status::Issued {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    let done = sqlx::query(
        "INSERT INTO quote_outcome (document_id, outcome, decided_at, decided_by)
         VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(input.outcome)
    .bind(now())
    .bind(auth.user.id)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::new(ErrorCode::QuoteDecided));
    }
    audit::record(
        &mut tx,
        auth.user.id,
        "quote.outcome",
        &format!("document:{id}"),
        None,
        Some(serde_json::json!({ "outcome": input.outcome })),
    )
    .await?;
    let document = find(&mut tx, &business, id).await?;
    tx.commit().await?;
    Ok(Json(document))
}

async fn insert_draft(
    tx: &mut SqliteConnection,
    user_id: i64,
    draft: &Draft,
) -> Result<i64, ApiError> {
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO document (kind, status, customer_id, project_id, language, currency, title,
         intro, outro, valid_until, pricing, source_id, service_from, service_to, timesheet,
         rounding, dunning_level, created_at, updated_at)
         VALUES (?, 'draft', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(draft.kind)
    .bind(draft.customer_id)
    .bind(draft.project_id)
    .bind(draft.language)
    .bind(&draft.currency)
    .bind(&draft.title)
    .bind(&draft.intro)
    .bind(&draft.outro)
    .bind(&draft.valid_until)
    .bind(draft.pricing)
    .bind(draft.source_id)
    .bind(&draft.service_from)
    .bind(&draft.service_to)
    .bind(draft.timesheet)
    .bind(draft.rounding)
    .bind(draft.dunning_level)
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    write_lines(tx, id, &draft.lines).await?;
    audit::record(
        tx,
        user_id,
        "document.create",
        &format!("document:{id}"),
        None,
        Some(serde_json::to_value(draft).map_err(ApiError::internal)?),
    )
    .await?;
    Ok(id)
}

#[utoipa::path(put, path = "/documents/{id}", tag = "documents", request_body = DocumentInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn put_document(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<DocumentInput>,
) -> Result<Json<Document>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = draft_row(&mut tx, id).await?;
    if input.kind != before.kind {
        let mut check = Check::default();
        check.fail("kind", FieldError::Locked);
        check.done()?;
    }
    let mut draft = validate(&mut tx, input, Some(id)).await?;
    let before = snapshot(&mut tx, &before).await?;
    draft.source_id = before.source_id;
    draft.rounding = before.rounding;
    draft.dunning_level = before.dunning_level;
    if draft.kind == Kind::Dunning && !draft.lines.is_empty() {
        return Err(field_error("lines", FieldError::InvalidFormat));
    }
    if let (Kind::Cancellation, Some(invoice)) = (draft.kind, draft.source_id) {
        let wanted: Vec<i64> = draft
            .lines
            .iter()
            .filter_map(|l| l.cancels_line_id)
            .collect();
        draft.lines = reversal_lines(&mut tx, invoice, &wanted).await?;
    }
    sqlx::query(
        "UPDATE document SET customer_id = ?, project_id = ?, language = ?, currency = ?,
         title = ?, intro = ?, outro = ?, valid_until = ?, pricing = ?, service_from = ?,
         service_to = ?, timesheet = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(draft.customer_id)
    .bind(draft.project_id)
    .bind(draft.language)
    .bind(&draft.currency)
    .bind(&draft.title)
    .bind(&draft.intro)
    .bind(&draft.outro)
    .bind(&draft.valid_until)
    .bind(draft.pricing)
    .bind(&draft.service_from)
    .bind(&draft.service_to)
    .bind(draft.timesheet)
    .bind(now())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM document_line WHERE document_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    write_lines(&mut tx, id, &draft.lines).await?;
    if let Some((old, new)) = audit::changes(&before, &draft) {
        audit::record(
            &mut tx,
            auth.user.id,
            "document.update",
            &format!("document:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    let document = find(&mut tx, &business, id).await?;
    tx.commit().await?;
    Ok(Json(document))
}

/// Only drafts can be deleted.
#[utoipa::path(delete, path = "/documents/{id}", tag = "documents", params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn delete_document(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let row = draft_row(&mut tx, id).await?;
    let before = snapshot(&mut tx, &row).await?;
    sqlx::query("DELETE FROM document WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "document.delete",
        &format!("document:{id}"),
        Some(serde_json::to_value(&before).map_err(ApiError::internal)?),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct IssueParams {
    /// EUR per unit of the document's currency, for booking an invoice in another currency
    /// (ADR 0020); a cancellation invoice takes its invoice's.
    #[param(value_type = Option<String>)]
    exchange_rate: Option<Decimal>,
}

/// Gives the draft its number and freezes it with its data as of now; an invoice is booked.
#[utoipa::path(post, path = "/documents/{id}/issue", tag = "documents",
    params(("id" = i64, Path), IssueParams),
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn issue_document(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Query(params): Query<IssueParams>,
) -> Result<Json<Document>, ApiError> {
    if params
        .exchange_rate
        .is_some_and(|r| r <= Decimal::ZERO || r > Decimal::from(1_000_000) || r.scale() > 6)
    {
        return Err(field_error("exchange_rate", FieldError::InvalidFormat));
    }
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    issue(
        &app,
        &mut tx,
        &business,
        auth.user.id,
        id,
        params.exchange_rate,
    )
    .await?;
    let document = find(&mut tx, &business, id).await?;
    tx.commit().await?;
    Ok(Json(document))
}

/// The document as PDF: as issued, or for a draft as it would look now.
#[utoipa::path(get, path = "/documents/{id}/pdf", tag = "documents", params(("id" = i64, Path)),
    responses((status = OK, content_type = "application/pdf", body = Vec<u8>),
        (status = "default", body = ApiError)))]
pub async fn document_pdf(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let mut db = app.db.acquire().await?;
    let row = row(&mut db, id).await?;
    let (bytes, name) = match &row.pdf {
        Some(hash) => {
            let hash = hash.parse().map_err(ApiError::internal)?;
            let bytes = app.files.get(hash).await.map_err(ApiError::internal)?;
            (bytes.to_vec(), row.number.clone().unwrap_or_default())
        }
        None => {
            let business = business(&app).await?;
            let data = data(&mut db, &business, &row).await?;
            let layout = layouts::for_document(&app, &mut db, row.kind, row.customer_id).await?;
            drop(db);
            let (rendered, _) = render::render_blocking(layout, data, Vec::new())
                .await
                .map_err(ApiError::layout)?;
            (
                rendered.pdf().map_err(ApiError::layout)?,
                format!("draft-{id}"),
            )
        }
    };
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{name}.pdf\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Issues a draft within `tx`: numbers it, stores each line's price and VAT rate, renders it,
/// and keeps the layout data and PDF as issued.
pub async fn issue(
    app: &App,
    tx: &mut SqliteConnection,
    business: &Business,
    user_id: i64,
    id: i64,
    exchange_rate: Option<Decimal>,
) -> Result<Data, ApiError> {
    let row = draft_row(tx, id).await?;
    let rate = booking_rate(tx, business, &row, exchange_rate).await?;
    if let (Kind::Cancellation, Some(invoice)) = (row.kind, row.source_id) {
        let wanted: Vec<i64> = documents::lines(tx, id)
            .await?
            .iter()
            .filter_map(|l| l.cancels_line_id)
            .collect();
        reversal_lines(tx, invoice, &wanted).await?;
    }
    let mut data = documents::draft_data(tx, business, &row).await?;
    let missing = documents::missing(&data);
    if !missing.is_empty() {
        return Err(ApiError::fields(missing));
    }
    let today = today(business)?;
    let valid_until = match (row.kind, row.valid_until.as_deref()) {
        (Kind::Quote, Some(date)) if date < today.to_string().as_str() => {
            return Err(field_error("valid_until", FieldError::InvalidFormat));
        }
        (Kind::Quote, Some(date)) => Some(date.to_owned()),
        (Kind::Quote, None) => {
            let days = i64::from(business.quote_valid_days.unwrap_or(30));
            Some(
                today
                    .saturating_add(jiff::Span::new().days(days))
                    .to_string(),
            )
        }
        _ => None,
    };
    let number = documents::next_number(tx, row.kind, business, today.year()).await?;
    data.number = Some(number.clone());
    data.issue_date = Some(today.to_string());
    data.valid_until = valid_until.clone();
    // a dunning letter gives a week to pay
    let days = match row.kind {
        Kind::Invoice => Some(i64::from(business.payment_days.unwrap_or(14))),
        Kind::Dunning => Some(7),
        _ => None,
    };
    let due_date = days.map(|days| {
        today
            .saturating_add(jiff::Span::new().days(days))
            .to_string()
    });
    if let Some(dunning) = &mut data.dunning {
        if dunning.amount_due <= Decimal::ZERO {
            return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
        }
        dunning.pay_until = due_date.clone();
    } else {
        data.due_date = due_date.clone();
    }
    if matches!(row.kind, Kind::Invoice | Kind::Cancellation) && data.service_from.is_none() {
        data.service_from = Some(today.to_string());
        data.service_to = Some(today.to_string());
    }
    let layout = layouts::for_document(app, tx, row.kind, row.customer_id).await?;
    let layout_name = layout.name.clone();
    let e_invoice = matches!(row.kind, Kind::Invoice | Kind::Cancellation);
    let attachments = if e_invoice {
        vec![render::Attachment {
            name: einvoice::ZUGFERD_NAME.to_owned(),
            description: "Factur-X / ZUGFeRD".to_owned(),
            bytes: einvoice::cii(&data, einvoice::Flavour::Zugferd),
        }]
    } else {
        Vec::new()
    };
    let (rendered, data) = render::render_blocking(layout, data, attachments)
        .await
        .map_err(ApiError::layout)?;
    let not_shown = render::missing(&data, &rendered);
    if !not_shown.is_empty() {
        return Err(ApiError::fields(
            not_shown
                .into_iter()
                .map(|key| (key, FieldError::NotShown))
                .collect(),
        ));
    }
    let mut pdf = rendered.pdf().map_err(ApiError::layout)?;
    if e_invoice {
        pdf = einvoice::mark_zugferd(&pdf).map_err(ApiError::internal)?;
    }
    let pdf = app
        .files
        .put(pdf.into())
        .await
        .map_err(ApiError::internal)?;
    let xml = match e_invoice && data.customer.xrechnung {
        true => Some(
            app.files
                .put(einvoice::cii(&data, einvoice::Flavour::Xrechnung).into())
                .await
                .map_err(ApiError::internal)?
                .to_string(),
        ),
        false => None,
    };
    for line in documents::lines(tx, id).await? {
        let (price, vat) = documents::effective(tx, business, &line).await?;
        sqlx::query("UPDATE document_line SET unit_price = ?, vat_rate = ? WHERE id = ?")
            .bind(price)
            .bind(vat.to_string())
            .bind(line.id)
            .execute(&mut *tx)
            .await?;
    }
    let at = now();
    sqlx::query(
        "UPDATE document SET status = 'issued', number = ?, issue_date = ?, data = ?, pdf = ?,
         layout = ?, valid_until = ?, due_date = ?, xml = ?, issued_at = ?, issued_by = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&number)
    .bind(today.to_string())
    .bind(serde_json::to_string(&data).map_err(ApiError::internal)?)
    .bind(pdf.to_string())
    .bind(layout_name)
    .bind(valid_until)
    .bind(due_date)
    .bind(xml)
    .bind(at)
    .bind(user_id)
    .bind(at)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        tx,
        user_id,
        "document.issue",
        &format!("document:{id}"),
        None,
        Some(serde_json::json!({ "number": number, "issue_date": data.issue_date })),
    )
    .await?;
    if let Some(rate) = rate {
        rules::post_document(tx, business, user_id, id, &data, rate)
            .await
            .map_err(rule_error)?;
    }
    Ok(data)
}

/// The EUR per unit an invoice or cancellation invoice is booked at; none for documents that are
/// not booked, or while the books are not started.
async fn booking_rate(
    tx: &mut SqliteConnection,
    business: &Business,
    row: &Row,
    given: Option<Decimal>,
) -> Result<Option<Decimal>, ApiError> {
    if !matches!(row.kind, Kind::Invoice | Kind::Cancellation) {
        return Ok(None);
    }
    let Some(books) = books::books(tx).await? else {
        return Ok(None);
    };
    if today(business)?.to_string() < books.start_date {
        return Ok(None);
    }
    if row.currency == crate::money::DEFAULT_CURRENCY {
        return Ok(Some(Decimal::ONE));
    }
    if let Some(rate) = given {
        return Ok(Some(rate));
    }
    let booked = match (row.kind, row.source_id) {
        (Kind::Cancellation, Some(invoice)) => {
            rules::booked_rate(tx, invoice).await.map_err(rule_error)?
        }
        _ => None,
    };
    booked
        .map(Some)
        .ok_or_else(|| field_error("exchange_rate", FieldError::Required))
}

pub fn rule_error(error: rules::Error) -> ApiError {
    match error {
        rules::Error::NoAccount(role) => {
            ApiError::new(ErrorCode::NoLedgerAccount).with_detail(role)
        }
        rules::Error::Db(error) => ApiError::internal(error),
    }
}

pub fn today(business: &Business) -> Result<jiff::civil::Date, ApiError> {
    let tz = jiff::tz::TimeZone::get(business.timezone.as_deref().unwrap_or("Europe/Berlin"))
        .map_err(ApiError::internal)?;
    Ok(jiff::Timestamp::now().to_zoned(tz).date())
}

pub async fn business(app: &App) -> Result<Business, ApiError> {
    app.settings.business().await.map_err(ApiError::internal)
}

pub async fn row(db: &mut SqliteConnection, id: i64) -> Result<Row, ApiError> {
    sqlx::query_as("SELECT * FROM document WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}

pub async fn draft_row(db: &mut SqliteConnection, id: i64) -> Result<Row, ApiError> {
    let row = row(db, id).await?;
    if row.status == Status::Issued {
        return Err(ApiError::new(ErrorCode::Issued));
    }
    Ok(row)
}

pub async fn data(
    db: &mut SqliteConnection,
    business: &Business,
    row: &Row,
) -> Result<Data, ApiError> {
    match &row.data {
        Some(data) => serde_json::from_str(data).map_err(ApiError::internal),
        None => Ok(documents::draft_data(db, business, row).await?),
    }
}

pub async fn find(
    db: &mut SqliteConnection,
    business: &Business,
    id: i64,
) -> Result<Document, ApiError> {
    let row = row(db, id).await?;
    let data = data(db, business, &row).await?;
    let missing = match row.status {
        Status::Draft => documents::missing(&data),
        Status::Issued => BTreeMap::new(),
    };
    let sql = format!(
        "SELECT {QUOTE_STATE}, c.name, d.issued_at, d.created_at, d.updated_at
         FROM document d JOIN customer c ON c.id = d.customer_id WHERE d.id = ?"
    );
    let (quote_state, customer_name, issued_at, created_at, updated_at): (
        Option<QuoteState>,
        String,
        Option<i64>,
        i64,
        i64,
    ) = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(today(business)?.to_string())
        .bind(id)
        .fetch_one(&mut *db)
        .await?;
    let follow_ups = sqlx::query_as(
        "SELECT id, kind, status, number FROM document WHERE source_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    Ok(Document {
        id,
        kind: row.kind,
        status: row.status,
        customer_id: row.customer_id,
        customer_name,
        project_id: row.project_id,
        language: row.language,
        currency: row.currency,
        title: row.title,
        intro: row.intro,
        outro: row.outro,
        number: row.number,
        issue_date: row.issue_date,
        lines: documents::lines(db, id).await?,
        data,
        missing,
        valid_until: row.valid_until,
        pricing: row.pricing,
        source_id: row.source_id,
        due_date: row.due_date,
        service_from: row.service_from,
        service_to: row.service_to,
        timesheet: row.timesheet,
        rounding: row.rounding,
        quote_state,
        follow_ups,
        sent: super::sending::sent(db, id).await?,
        payments: super::overview::payments(db, id).await?,
        amount_due: match (row.kind, row.status) {
            (Kind::Invoice, Status::Issued) => Some(super::overview::amount_due(db, id).await?),
            _ => None,
        },
        layout: row.layout,
        issued_at,
        created_at,
        updated_at,
    })
}

/// A validated draft; also the snapshot the audit log compares.
#[derive(Debug, Serialize)]
pub struct Draft {
    kind: Kind,
    customer_id: i64,
    project_id: Option<i64>,
    language: Language,
    currency: String,
    title: Option<String>,
    intro: Option<String>,
    outro: Option<String>,
    valid_until: Option<String>,
    pricing: Option<Pricing>,
    source_id: Option<i64>,
    service_from: Option<String>,
    service_to: Option<String>,
    timesheet: bool,
    rounding: i64,
    dunning_level: Option<i64>,
    lines: Vec<Line>,
}

#[derive(Debug, Serialize)]
pub struct Line {
    description: String,
    quantity: Decimal,
    unit: Unit,
    unit_price: Option<i64>,
    vat_rate: Option<Decimal>,
    project_id: Option<i64>,
    activity_id: Option<i64>,
    time_entry_ids: Vec<i64>,
    cancels_line_id: Option<i64>,
}

async fn snapshot(db: &mut SqliteConnection, row: &Row) -> Result<Draft, ApiError> {
    Ok(Draft {
        kind: row.kind,
        customer_id: row.customer_id,
        project_id: row.project_id,
        language: row.language,
        currency: row.currency.clone(),
        title: row.title.clone(),
        intro: row.intro.clone(),
        outro: row.outro.clone(),
        valid_until: row.valid_until.clone(),
        pricing: row.pricing,
        source_id: row.source_id,
        service_from: row.service_from.clone(),
        service_to: row.service_to.clone(),
        timesheet: row.timesheet,
        rounding: row.rounding,
        dunning_level: row.dunning_level,
        lines: documents::lines(db, row.id)
            .await?
            .into_iter()
            .map(|l| Line {
                description: l.description,
                quantity: l.quantity.0,
                unit: l.unit,
                unit_price: l.unit_price,
                vat_rate: l.vat_rate.map(|v| v.0),
                project_id: l.project_id,
                activity_id: l.activity_id,
                time_entry_ids: l.time_entry_ids,
                cancels_line_id: l.cancels_line_id,
            })
            .collect(),
    })
}

async fn write_lines(tx: &mut SqliteConnection, id: i64, lines: &[Line]) -> Result<(), ApiError> {
    for (position, line) in lines.iter().enumerate() {
        let line_id: i64 = sqlx::query_scalar(
            "INSERT INTO document_line (document_id, position, description, quantity, unit,
             unit_price, vat_rate, project_id, activity_id, cancels_line_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
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
        .bind(line.cancels_line_id)
        .fetch_one(&mut *tx)
        .await?;
        for entry in &line.time_entry_ids {
            sqlx::query("INSERT INTO line_time_entry (line_id, time_entry_id) VALUES (?, ?)")
                .bind(line_id)
                .bind(entry)
                .execute(&mut *tx)
                .await?;
        }
    }
    Ok(())
}

async fn validate(
    db: &mut SqliteConnection,
    input: DocumentInput,
    document_id: Option<i64>,
) -> Result<Draft, ApiError> {
    let mut check = Check::default();
    let customer: Option<(Language, String, bool)> =
        sqlx::query_as("SELECT language, currency, archived FROM customer WHERE id = ?")
            .bind(input.customer_id)
            .fetch_optional(&mut *db)
            .await?;
    let Some((customer_language, currency, archived)) = customer else {
        return Err(ApiError::fields(BTreeMap::from([(
            "customer_id".to_owned(),
            FieldError::InvalidFormat,
        )])));
    };
    if archived {
        return Err(ApiError::new(ErrorCode::Archived));
    }
    let projects: Vec<(i64, bool)> =
        sqlx::query_as("SELECT id, archived FROM project WHERE customer_id = ?")
            .bind(input.customer_id)
            .fetch_all(&mut *db)
            .await?;
    let project_ok = |id: i64| projects.iter().any(|(p, _)| *p == id);
    if input.project_id.is_some_and(|p| !project_ok(p)) {
        check.fail("project_id", FieldError::InvalidFormat);
    }
    let activities: Vec<i64> = sqlx::query_scalar("SELECT id FROM activity")
        .fetch_all(&mut *db)
        .await?;
    if input.lines.len() > MAX_LINES {
        check.fail("lines", FieldError::TooLong);
    }
    let invoice = input.kind == Kind::Invoice;
    let mut seen = std::collections::BTreeSet::new();
    let mut lines = Vec::new();
    for (i, line) in input.lines.into_iter().enumerate() {
        let key = |field: &str| format!("lines.{i}.{field}");
        if line.quantity.is_zero()
            || line.quantity.abs() > Decimal::from(1_000_000)
            || line.quantity.scale() > 4
        {
            check.fail(&key("quantity"), FieldError::InvalidFormat);
        }
        if line.unit_price.is_some_and(|p| p.abs() > MAX_AMOUNT) {
            check.fail(&key("unit_price"), FieldError::InvalidFormat);
        }
        if line
            .vat_rate
            .is_some_and(|v| v.is_sign_negative() || v > Decimal::ONE_HUNDRED || v.scale() > 2)
        {
            check.fail(&key("vat_rate"), FieldError::InvalidFormat);
        }
        if line.project_id.is_some_and(|p| !project_ok(p)) {
            check.fail(&key("project_id"), FieldError::InvalidFormat);
        }
        if line.activity_id.is_some_and(|a| !activities.contains(&a)) {
            check.fail(&key("activity_id"), FieldError::InvalidFormat);
        }
        if line.cancels_line_id.is_some() != (input.kind == Kind::Cancellation) {
            check.fail(&key("cancels_line_id"), FieldError::InvalidFormat);
        }
        for entry in &line.time_entry_ids {
            let usable: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM time_entry e JOIN project p ON p.id = e.project_id
                 WHERE e.id = ?1 AND e.billable AND e.ended_at IS NOT NULL AND p.customer_id = ?2
                 AND NOT EXISTS (SELECT 1 FROM line_time_entry x JOIN billed_line l ON l.id = x.line_id
                     WHERE x.time_entry_id = e.id AND l.document_id IS NOT ?3))",
            )
            .bind(entry)
            .bind(input.customer_id)
            .bind(document_id)
            .fetch_one(&mut *db)
            .await?;
            if !invoice || !usable || !seen.insert(*entry) {
                check.fail(&key("time_entry_ids"), FieldError::InvalidFormat);
            }
        }
        lines.push(Line {
            description: check.required(&key("description"), line.description, 2000),
            quantity: line.quantity.normalize(),
            unit: line.unit,
            unit_price: line.unit_price,
            vat_rate: line.vat_rate.map(|v| v.normalize()),
            project_id: line.project_id,
            activity_id: line.activity_id,
            time_entry_ids: line.time_entry_ids,
            cancels_line_id: line.cancels_line_id,
        });
    }
    let quote = input.kind == Kind::Quote;
    if input
        .valid_until
        .as_deref()
        .is_some_and(|d| !quote || d.parse::<jiff::civil::Date>().is_err())
    {
        check.fail("valid_until", FieldError::InvalidFormat);
    }
    if input.pricing.is_some() && !quote {
        check.fail("pricing", FieldError::InvalidFormat);
    }
    for (key, date) in [
        ("service_from", &input.service_from),
        ("service_to", &input.service_to),
    ] {
        if date
            .as_deref()
            .is_some_and(|d| !invoice || d.parse::<jiff::civil::Date>().is_err())
        {
            check.fail(key, FieldError::InvalidFormat);
        }
    }
    if input.service_from.is_some() != input.service_to.is_some()
        || input.service_from > input.service_to
    {
        check.fail("service_to", FieldError::InvalidFormat);
    }
    let draft = Draft {
        kind: input.kind,
        customer_id: input.customer_id,
        project_id: input.project_id,
        language: input.language.unwrap_or(customer_language),
        currency,
        title: check.text("title", input.title, 200),
        intro: check.text("intro", input.intro, 10_000),
        outro: check.text("outro", input.outro, 10_000),
        valid_until: input.valid_until.filter(|d| !d.is_empty()),
        pricing: if quote {
            Some(input.pricing.unwrap_or(Pricing::Fixed))
        } else {
            None
        },
        source_id: None,
        service_from: input.service_from,
        service_to: input.service_to,
        timesheet: invoice && input.timesheet,
        rounding: 0,
        dunning_level: None,
        lines,
    };
    check.done()?;
    Ok(draft)
}

impl Line {
    /// A line charging time entries at the hourly rate of their project and activity.
    pub fn from_time(
        description: String,
        quantity: Decimal,
        unit: Unit,
        project_id: i64,
        activity_id: Option<i64>,
        time_entry_ids: Vec<i64>,
    ) -> Self {
        Self {
            description,
            quantity: quantity.normalize(),
            unit,
            unit_price: None,
            vat_rate: None,
            project_id: Some(project_id),
            activity_id,
            time_entry_ids,
            cancels_line_id: None,
        }
    }
}

pub async fn snapshot_lines(db: &mut SqliteConnection, id: i64) -> Result<Vec<Line>, ApiError> {
    let row = row(db, id).await?;
    Ok(snapshot(db, &row).await?.lines)
}

/// Replaces a draft's lines.
pub async fn write_all_lines(
    tx: &mut SqliteConnection,
    id: i64,
    lines: &[Line],
) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM document_line WHERE document_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    write_lines(tx, id, lines).await
}

/// Lines reversing the given lines of an issued invoice, with its stored text and amounts; a line
/// an issued cancellation invoice already reverses cannot be reversed again.
async fn reversal_lines(
    db: &mut SqliteConnection,
    invoice_id: i64,
    wanted: &[i64],
) -> Result<Vec<Line>, ApiError> {
    let mut lines = Vec::new();
    for line in documents::lines(db, invoice_id).await? {
        if !wanted.contains(&line.id) {
            continue;
        }
        let reversed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_line c JOIN document d ON d.id = c.document_id
             WHERE c.cancels_line_id = ? AND d.status = 'issued')",
        )
        .bind(line.id)
        .fetch_one(&mut *db)
        .await?;
        if reversed {
            return Err(ApiError::new(ErrorCode::AlreadyCancelled));
        }
        lines.push(Line {
            description: line.description,
            quantity: -line.quantity.0,
            unit: line.unit,
            unit_price: line.unit_price,
            vat_rate: line.vat_rate.map(|v| v.0),
            project_id: line.project_id,
            activity_id: line.activity_id,
            time_entry_ids: Vec::new(),
            cancels_line_id: Some(line.id),
        });
    }
    if lines.len() != wanted.len() {
        return Err(field_error("lines", FieldError::InvalidFormat));
    }
    Ok(lines)
}

/// The lines of an issued invoice no issued cancellation invoice reverses yet.
async fn unreversed(db: &mut SqliteConnection, invoice_id: i64) -> Result<Vec<i64>, ApiError> {
    Ok(
        sqlx::query_scalar("SELECT id FROM billed_line WHERE document_id = ? ORDER BY position")
            .bind(invoice_id)
            .fetch_all(db)
            .await?,
    )
}

/// The e-invoice XML of an invoice or cancellation invoice: the XRechnung as issued, or else the
/// EN 16931 XML it carries as ZUGFeRD.
#[utoipa::path(get, path = "/documents/{id}/xml", tag = "documents", params(("id" = i64, Path)),
    responses((status = OK, content_type = "application/xml", body = String),
        (status = "default", body = ApiError)))]
pub async fn document_xml(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let mut db = app.db.acquire().await?;
    let row = row(&mut db, id).await?;
    if !matches!(row.kind, Kind::Invoice | Kind::Cancellation) {
        return Err(ApiError::new(ErrorCode::NotFound));
    }
    let bytes = match &row.xml {
        Some(hash) => app
            .files
            .get(hash.parse().map_err(ApiError::internal)?)
            .await
            .map_err(ApiError::internal)?
            .to_vec(),
        None => {
            let data = data(&mut db, &business(&app).await?, &row).await?;
            let flavour = match data.customer.xrechnung && row.status == Status::Draft {
                true => einvoice::Flavour::Xrechnung,
                false => einvoice::Flavour::Zugferd,
            };
            einvoice::cii(&data, flavour)
        }
    };
    let name = row.number.unwrap_or_else(|| format!("draft-{id}"));
    Ok((
        [
            (header::CONTENT_TYPE, "application/xml".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}.xml\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// The level of the next dunning letter for an invoice that is still owed: one above the
/// highest issued, at most three.
async fn next_dunning_level(db: &mut SqliteConnection, invoice: i64) -> Result<i64, ApiError> {
    if documents::amount_due(db, invoice).await? <= 0 {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    let issued: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(dunning_level), 0) FROM document
         WHERE source_id = ? AND kind = 'dunning' AND status = 'issued'",
    )
    .bind(invoice)
    .fetch_one(db)
    .await?;
    if issued >= 3 {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    Ok(issued + 1)
}

fn dunning_intro(language: Language, level: i64) -> &'static str {
    match (language, level) {
        (Language::De, 1) => {
            "Sehr geehrte Damen und Herren,\n\nsicher ist es Ihnen im Alltag entgangen: Für die folgende Rechnung ist bei uns noch keine vollständige Zahlung eingegangen. Bitte überweisen Sie den offenen Betrag bis zum genannten Datum. Sollten Sie inzwischen gezahlt haben, betrachten Sie dieses Schreiben bitte als gegenstandslos."
        }
        (Language::De, 2) => {
            "Sehr geehrte Damen und Herren,\n\nleider ist der offene Betrag der folgenden Rechnung trotz unserer Zahlungserinnerung noch nicht bei uns eingegangen. Bitte überweisen Sie ihn bis zum genannten Datum."
        }
        (Language::De, _) => {
            "Sehr geehrte Damen und Herren,\n\ntrotz Zahlungserinnerung und Mahnung ist der offene Betrag der folgenden Rechnung noch nicht bei uns eingegangen. Bitte überweisen Sie ihn bis zum genannten Datum. Andernfalls sehen wir uns gezwungen, weitere Schritte einzuleiten."
        }
        (Language::En, 1) => {
            "Dear Sir or Madam,\n\nperhaps it has slipped your attention: we have not yet received full payment of the invoice below. Please transfer the amount due by the date given. If you have paid in the meantime, please disregard this letter."
        }
        (Language::En, 2) => {
            "Dear Sir or Madam,\n\ndespite our reminder, the amount due on the invoice below has not reached us yet. Please transfer it by the date given."
        }
        (Language::En, _) => {
            "Dear Sir or Madam,\n\ndespite our reminder and dunning letter, the amount due on the invoice below has not reached us yet. Please transfer it by the date given; otherwise we will have to take further steps."
        }
    }
}
