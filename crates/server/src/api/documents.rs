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
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    documents::{self, Data, Kind, LineRow, Row, Status, Unit},
    layouts, render,
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
    currency: String,
    /// Major units; none for documents without prices.
    #[sqlx(skip)]
    #[schema(value_type = Option<String>)]
    gross: Option<Decimal>,
    updated_at: i64,
}

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
    let mut list: Vec<DocumentSummary> = sqlx::query_as(
        "SELECT d.id, d.kind, d.status, d.number, d.customer_id, c.name AS customer_name, d.title,
         d.issue_date, d.currency, d.updated_at
         FROM document d JOIN customer c ON c.id = d.customer_id
         WHERE (?1 IS NULL OR d.kind = ?1) AND (?2 IS NULL OR d.status = ?2)
         AND (?3 IS NULL OR d.customer_id = ?3)
         ORDER BY d.status = 'issued', d.issue_date DESC, d.number DESC, d.updated_at DESC",
    )
    .bind(filter.kind)
    .bind(filter.status)
    .bind(filter.customer_id)
    .fetch_all(&mut *db)
    .await?;
    let business = business(&app).await?;
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
    let draft = validate(&mut tx, input).await?;
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO document (kind, status, customer_id, project_id, language, currency, title,
         intro, outro, created_at, updated_at)
         VALUES (?, 'draft', ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(draft.kind)
    .bind(draft.customer_id)
    .bind(draft.project_id)
    .bind(draft.language)
    .bind(&draft.currency)
    .bind(&draft.title)
    .bind(&draft.intro)
    .bind(&draft.outro)
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    write_lines(&mut tx, id, &draft.lines).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "document.create",
        &format!("document:{id}"),
        None,
        Some(serde_json::to_value(&draft).map_err(ApiError::internal)?),
    )
    .await?;
    let document = find(&mut tx, &business, id).await?;
    tx.commit().await?;
    Ok(Json(document))
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
    let draft = validate(&mut tx, input).await?;
    let before = snapshot(&mut tx, &before).await?;
    sqlx::query(
        "UPDATE document SET customer_id = ?, project_id = ?, language = ?, currency = ?,
         title = ?, intro = ?, outro = ?, updated_at = ? WHERE id = ?",
    )
    .bind(draft.customer_id)
    .bind(draft.project_id)
    .bind(draft.language)
    .bind(&draft.currency)
    .bind(&draft.title)
    .bind(&draft.intro)
    .bind(&draft.outro)
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

/// Gives the draft its number and freezes it with its data as of now.
#[utoipa::path(post, path = "/documents/{id}/issue", tag = "documents",
    params(("id" = i64, Path)),
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn issue_document(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<Json<Document>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    issue(&app, &mut tx, &business, auth.user.id, id).await?;
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
            let layout = layouts::for_document(&mut db, row.kind, row.customer_id).await?;
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
) -> Result<Data, ApiError> {
    let row = draft_row(tx, id).await?;
    let mut data = documents::draft_data(tx, business, &row).await?;
    let missing = documents::missing(&data);
    if !missing.is_empty() {
        return Err(ApiError::fields(missing));
    }
    let today = today(business)?;
    let number = documents::next_number(tx, row.kind, business, today.year()).await?;
    data.number = Some(number.clone());
    data.issue_date = Some(today.to_string());
    let layout = layouts::for_document(tx, row.kind, row.customer_id).await?;
    let (rendered, data) = render::render_blocking(layout, data, Vec::new())
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
    let pdf = rendered.pdf().map_err(ApiError::layout)?;
    let pdf = app
        .files
        .put(pdf.into())
        .await
        .map_err(ApiError::internal)?;
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
         issued_at = ?, issued_by = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&number)
    .bind(today.to_string())
    .bind(serde_json::to_string(&data).map_err(ApiError::internal)?)
    .bind(pdf.to_string())
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
    Ok(data)
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
    let (customer_name, issued_at, created_at, updated_at): (String, Option<i64>, i64, i64) =
        sqlx::query_as(
            "SELECT c.name, d.issued_at, d.created_at, d.updated_at
             FROM document d JOIN customer c ON c.id = d.customer_id WHERE d.id = ?",
        )
        .bind(id)
        .fetch_one(&mut *db)
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
            })
            .collect(),
    })
}

async fn write_lines(tx: &mut SqliteConnection, id: i64, lines: &[Line]) -> Result<(), ApiError> {
    for (position, line) in lines.iter().enumerate() {
        sqlx::query(
            "INSERT INTO document_line (document_id, position, description, quantity, unit,
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
    Ok(())
}

async fn validate(db: &mut SqliteConnection, input: DocumentInput) -> Result<Draft, ApiError> {
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
        lines.push(Line {
            description: check.required(&key("description"), line.description, 2000),
            quantity: line.quantity.normalize(),
            unit: line.unit,
            unit_price: line.unit_price,
            vat_rate: line.vat_rate.map(|v| v.normalize()),
            project_id: line.project_id,
            activity_id: line.activity_id,
        });
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
        lines,
    };
    check.done()?;
    Ok(draft)
}
