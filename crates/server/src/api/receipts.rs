use std::{collections::BTreeMap, sync::Arc};

use axum::{
    extract::{Multipart, State, multipart::MultipartRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    books::started,
    documents::{business, rule_error, today},
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::{Check, field_error},
    suppliers::{self, SupplierFields},
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    books,
    incoming::{self, Format, Incoming},
    money::{DEFAULT_CURRENCY, MAX_AMOUNT},
    rules::{self, Column},
};

const MAX_FILE: usize = 20 << 20;
const MAX_LINES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum ReceiptStatus {
    Draft,
    Booked,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ReceiptLine {
    id: i64,
    description: String,
    account_id: Option<i64>,
    /// In the minor unit of the receipt's currency.
    net: i64,
    /// Percent.
    vat_rate: String,
    /// In the minor unit of the receipt's currency.
    tax: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ReceiptPayment {
    id: i64,
    /// ISO date the money left.
    date: String,
    /// In the minor unit of the receipt's currency.
    amount: i64,
    /// EUR cents that left, for a receipt in another currency.
    eur_amount: Option<i64>,
    account_id: i64,
    /// The bank transaction it came from; undone by undoing that match.
    bank_match_id: Option<i64>,
    note: Option<String>,
    user_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Receipt {
    id: i64,
    status: ReceiptStatus,
    supplier_id: Option<i64>,
    supplier_name: Option<String>,
    /// The supplier's own number for it.
    number: Option<String>,
    /// ISO dates.
    date: Option<String>,
    due_date: Option<String>,
    currency: String,
    /// EUR per unit of a receipt in another currency.
    exchange_rate: Option<String>,
    note: Option<String>,
    file_name: Option<String>,
    file_type: Option<String>,
    /// The e-invoice format it was read from.
    format: Option<Format>,
    lines: Vec<ReceiptLine>,
    /// In the minor unit of its currency.
    net: i64,
    tax: i64,
    gross: i64,
    /// For a booked receipt: what is still owed, in the minor unit of its currency.
    amount_due: Option<i64>,
    payments: Vec<ReceiptPayment>,
    reversed: bool,
    /// Another receipt with the same file, or the same supplier and number.
    duplicate_of: Option<i64>,
    /// For a draft: what is missing before it can be booked.
    missing: BTreeMap<String, FieldError>,
    booked_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ReceiptSummary {
    id: i64,
    status: ReceiptStatus,
    supplier_name: Option<String>,
    number: Option<String>,
    date: Option<String>,
    due_date: Option<String>,
    currency: String,
    gross: i64,
    amount_due: Option<i64>,
    reversed: bool,
    format: Option<Format>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReceiptFilter {
    status: Option<ReceiptStatus>,
    /// Only booked receipts with an amount due.
    #[serde(default)]
    open: bool,
}

/// Gross less the payments of a booked receipt not reversed; nothing for a draft.
const DUE: &str = "CASE WHEN r.status = 'draft' THEN NULL
    WHEN EXISTS (SELECT 1 FROM receipt_reversal v WHERE v.receipt_id = r.id) THEN 0
    ELSE coalesce((SELECT sum(net + tax) FROM receipt_line WHERE receipt_id = r.id), 0)
       - coalesce((SELECT sum(amount) FROM receipt_payment
                   WHERE receipt_id = r.id AND reversed_at IS NULL), 0) END";

#[utoipa::path(get, path = "/receipts", tag = "books", params(ReceiptFilter),
    responses((status = OK, body = Vec<ReceiptSummary>), (status = "default", body = ApiError)))]
pub async fn list_receipts(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(filter): Query<ReceiptFilter>,
) -> Result<Json<Vec<ReceiptSummary>>, ApiError> {
    let sql = format!(
        "SELECT * FROM (SELECT r.id, r.status, s.name AS supplier_name, r.number, r.date,
         r.due_date, r.currency,
         coalesce((SELECT sum(net + tax) FROM receipt_line WHERE receipt_id = r.id), 0) AS gross,
         {DUE} AS amount_due,
         EXISTS (SELECT 1 FROM receipt_reversal v WHERE v.receipt_id = r.id) AS reversed, r.format
         FROM receipt r LEFT JOIN supplier s ON s.id = r.supplier_id)
         WHERE (?1 IS NULL OR status = ?1) AND (NOT ?2 OR amount_due <> 0)
         ORDER BY status = 'booked', coalesce(date, '9999') DESC, id DESC"
    );
    Ok(Json(
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(filter.status)
            .bind(filter.open)
            .fetch_all(&app.db)
            .await?,
    ))
}

#[derive(ToSchema)]
#[allow(
    dead_code,
    reason = "describes the multipart body for the OpenAPI document"
)]
pub struct ReceiptUpload {
    /// A PDF, a photo (JPEG, PNG, WebP) or an e-invoice as XML; none for a receipt without one.
    #[schema(value_type = Option<String>, format = Binary)]
    file: Option<Vec<u8>>,
}

/// The media type of an accepted upload, from its first bytes.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    let text = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let start = text
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(0);
    if bytes.starts_with(b"%PDF") {
        Some("application/pdf")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if text[start..].starts_with(b"<") {
        Some("application/xml")
    } else {
        None
    }
}

/// Creates a draft from an uploaded receipt; an e-invoice fills in its supplier, number, dates
/// and one line per VAT rate.
#[utoipa::path(post, path = "/receipts", tag = "books",
    request_body(content_type = "multipart/form-data", content = ReceiptUpload),
    responses((status = OK, body = Receipt), (status = "default", body = ApiError)))]
pub async fn create_receipt(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<Receipt>, ApiError> {
    fn malformed(_: impl std::fmt::Display) -> ApiError {
        ApiError::new(ErrorCode::MalformedRequest)
    }
    let mut multipart = multipart.map_err(malformed)?;
    let mut file = None;
    while let Some(field) = multipart.next_field().await.map_err(malformed)? {
        if field.name() == Some("file") {
            let name = field
                .file_name()
                .map(|n| n.chars().take(200).collect::<String>());
            let bytes = field.bytes().await.map_err(malformed)?;
            if !bytes.is_empty() {
                file = Some((name, bytes));
            }
        }
    }
    let (file, incoming) = match file {
        Some((name, bytes)) => {
            if bytes.len() > MAX_FILE {
                return Err(field_error("file", FieldError::TooLong));
            }
            let kind =
                sniff(&bytes).ok_or_else(|| field_error("file", FieldError::InvalidFormat))?;
            let incoming = incoming::read(&bytes);
            if kind == "application/xml" && incoming.is_none() {
                return Err(field_error("file", FieldError::InvalidFormat));
            }
            let hash = app.files.put(bytes).await.map_err(ApiError::internal)?;
            (Some((hash.to_string(), name, kind)), incoming)
        }
        None => (None, None),
    };
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let supplier = match &incoming {
        Some(incoming) => supplier_for(&mut tx, incoming).await?,
        None => None,
    };
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO receipt (status, supplier_id, number, date, due_date, currency, file,
         file_name, file_type, format, created_at, updated_at)
         VALUES ('draft', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(supplier)
    .bind(incoming.as_ref().and_then(|i| i.number.clone()))
    .bind(incoming.as_ref().and_then(|i| valid_date(i.date.clone())))
    .bind(
        incoming
            .as_ref()
            .and_then(|i| valid_date(i.due_date.clone())),
    )
    .bind(
        incoming
            .as_ref()
            .map(|i| i.currency.clone())
            .filter(|c| is_code(c))
            .unwrap_or_else(|| DEFAULT_CURRENCY.to_owned()),
    )
    .bind(file.as_ref().map(|f| f.0.clone()))
    .bind(file.as_ref().and_then(|f| f.1.clone()))
    .bind(file.as_ref().map(|f| f.2))
    .bind(incoming.as_ref().map(|i| i.format))
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    if let Some(incoming) = &incoming {
        let account = default_account(&mut tx, supplier).await?;
        for (position, group) in incoming.groups.iter().enumerate() {
            let description = if group.description.is_empty() {
                format!("{} %", group.rate)
            } else {
                group.description.clone()
            };
            sqlx::query(
                "INSERT INTO receipt_line (receipt_id, position, description, account_id, net,
                 vat_rate, tax) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(i64::try_from(position).expect("lines fit"))
            .bind(description)
            .bind(account)
            .bind(group.net)
            .bind(group.rate.to_string())
            .bind(group.tax)
            .execute(&mut *tx)
            .await?;
        }
    }
    let receipt = find(&mut tx, id).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "receipt.create",
        &format!("receipt:{id}"),
        None,
        Some(serde_json::json!({ "file": file.map(|f| f.0), "format": receipt.format })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(receipt))
}

fn valid_date(date: Option<String>) -> Option<String> {
    date.filter(|d| d.parse::<jiff::civil::Date>().is_ok())
}

fn is_code(currency: &str) -> bool {
    currency.len() == 3 && currency.chars().all(|c| c.is_ascii_uppercase())
}

/// The supplier an e-invoice comes from: known by VAT ID, IBAN or name, else created from it.
async fn supplier_for(
    db: &mut SqliteConnection,
    incoming: &Incoming,
) -> Result<Option<i64>, ApiError> {
    let party = &incoming.supplier;
    let clean = |v: &Option<String>| {
        v.as_ref()
            .map(|v| v.replace(' ', "").to_ascii_uppercase())
            .filter(|v| !v.is_empty())
    };
    let (vat_id, iban) = (clean(&party.vat_id), clean(&party.iban));
    let known: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM supplier WHERE (vat_id = ?1 AND ?1 IS NOT NULL)
         OR (iban = ?2 AND ?2 IS NOT NULL) OR (lower(name) = lower(?3))
         ORDER BY archived, (vat_id = ?1) DESC, (iban = ?2) DESC, id LIMIT 1",
    )
    .bind(&vat_id)
    .bind(&iban)
    .bind(&party.name)
    .fetch_optional(&mut *db)
    .await?;
    if known.is_some() {
        return Ok(known);
    }
    let Some(name) = party.name.clone() else {
        return Ok(None);
    };
    let take = |v: &Option<String>, max: usize| {
        v.as_ref().map(|v| v.chars().take(max).collect::<String>())
    };
    let country = party
        .country
        .clone()
        .filter(|c| c.len() == 2 && c.chars().all(|c| c.is_ascii_uppercase()))
        .unwrap_or_else(|| "DE".to_owned());
    let fields = SupplierFields {
        name: name.chars().take(200).collect(),
        street: take(&party.street, 500),
        postcode: take(&party.postcode, 20),
        city: take(&party.city, 100),
        country,
        vat_id: vat_id.filter(|v| super::business::is_vat_id(v)),
        iban: iban.filter(|v| super::business::is_iban(v)),
        email: take(&party.email, 254),
        account_id: None,
        archived: false,
    };
    Ok(Some(suppliers::insert(db, &fields).await?))
}

/// Where a supplier's lines are booked: its own account, else other operating expenses.
async fn default_account(
    db: &mut SqliteConnection,
    supplier: Option<i64>,
) -> Result<Option<i64>, sqlx::Error> {
    let own: Option<i64> = match supplier {
        Some(id) => {
            sqlx::query_scalar("SELECT account_id FROM supplier WHERE id = ?")
                .bind(id)
                .fetch_one(&mut *db)
                .await?
        }
        None => None,
    };
    match own {
        Some(account) => Ok(Some(account)),
        None => books::account(db, "expenses").await,
    }
}

#[utoipa::path(get, path = "/receipts/{id}", tag = "books", params(("id" = i64, Path)),
    responses((status = OK, body = Receipt), (status = "default", body = ApiError)))]
pub async fn get_receipt(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(id): Path<i64>,
) -> Result<Json<Receipt>, ApiError> {
    Ok(Json(find(&mut *app.db.acquire().await?, id).await?))
}

#[derive(sqlx::FromRow)]
struct Row {
    status: ReceiptStatus,
    supplier_id: Option<i64>,
    supplier_name: Option<String>,
    number: Option<String>,
    date: Option<String>,
    due_date: Option<String>,
    currency: String,
    exchange_rate: Option<String>,
    note: Option<String>,
    file: Option<String>,
    file_name: Option<String>,
    file_type: Option<String>,
    format: Option<Format>,
    booked_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    reversed: bool,
    amount_due: Option<i64>,
}

pub async fn find(db: &mut SqliteConnection, id: i64) -> Result<Receipt, ApiError> {
    let sql = format!(
        "SELECT r.status, r.supplier_id, s.name AS supplier_name, r.number, r.date, r.due_date,
         r.currency, r.exchange_rate, r.note, r.file, r.file_name, r.file_type, r.format,
         r.booked_at, r.created_at, r.updated_at,
         EXISTS (SELECT 1 FROM receipt_reversal v WHERE v.receipt_id = r.id) AS reversed,
         {DUE} AS amount_due
         FROM receipt r LEFT JOIN supplier s ON s.id = r.supplier_id WHERE r.id = ?"
    );
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let lines: Vec<ReceiptLine> = sqlx::query_as(
        "SELECT id, description, account_id, net, vat_rate, tax FROM receipt_line
         WHERE receipt_id = ? ORDER BY position",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    let payments = sqlx::query_as(
        "SELECT p.id, p.date, p.amount, p.eur_amount, p.account_id, p.bank_match_id, p.note,
         u.name AS user_name
         FROM receipt_payment p JOIN user u ON u.id = p.user_id
         WHERE p.receipt_id = ? AND p.reversed_at IS NULL ORDER BY p.date, p.id",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    let duplicate_of = sqlx::query_scalar(
        "SELECT id FROM receipt WHERE id <> ?1 AND ((file = ?2 AND ?2 IS NOT NULL)
         OR (supplier_id = ?3 AND number = ?4 AND ?4 IS NOT NULL)) ORDER BY id LIMIT 1",
    )
    .bind(id)
    .bind(&row.file)
    .bind(row.supplier_id)
    .bind(&row.number)
    .fetch_optional(&mut *db)
    .await?;
    let missing = match row.status {
        ReceiptStatus::Draft => missing(db, &row, &lines).await?,
        ReceiptStatus::Booked => BTreeMap::new(),
    };
    let (net, tax) = lines
        .iter()
        .fold((0, 0), |(n, t), l| (n + l.net, t + l.tax));
    Ok(Receipt {
        id,
        status: row.status,
        supplier_id: row.supplier_id,
        supplier_name: row.supplier_name,
        number: row.number,
        date: row.date,
        due_date: row.due_date,
        currency: row.currency,
        exchange_rate: row.exchange_rate,
        note: row.note,
        file_name: row.file_name,
        file_type: row.file_type,
        format: row.format,
        lines,
        net,
        tax,
        gross: net + tax,
        amount_due: row.amount_due,
        payments,
        reversed: row.reversed,
        duplicate_of,
        missing,
        booked_at: row.booked_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// What a draft lacks before it can be booked.
async fn missing(
    db: &mut SqliteConnection,
    row: &Row,
    lines: &[ReceiptLine],
) -> Result<BTreeMap<String, FieldError>, ApiError> {
    let mut missing = BTreeMap::new();
    if row.supplier_id.is_none() {
        missing.insert("supplier_id".to_owned(), FieldError::Required);
    }
    let start = books::books(db).await?.map(|b| b.start_date);
    match (&row.date, &start) {
        (None, _) => {
            missing.insert("date".to_owned(), FieldError::Required);
        }
        (Some(date), Some(start)) if date < start => {
            missing.insert("date".to_owned(), FieldError::InvalidFormat);
        }
        _ => {}
    }
    if row.currency != DEFAULT_CURRENCY && row.exchange_rate.is_none() {
        missing.insert("exchange_rate".to_owned(), FieldError::Required);
    }
    if lines.is_empty() {
        missing.insert("lines".to_owned(), FieldError::Required);
    }
    for (i, line) in lines.iter().enumerate() {
        if line.account_id.is_none() {
            missing.insert(format!("lines.{i}.account_id"), FieldError::Required);
        }
    }
    Ok(missing)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiptLineInput {
    description: Option<String>,
    account_id: Option<i64>,
    /// In the minor unit of the receipt's currency.
    net: i64,
    /// Percent.
    #[schema(value_type = String)]
    vat_rate: Decimal,
    /// In the minor unit of the receipt's currency; the net amount at the VAT rate when unset.
    tax: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiptInput {
    supplier_id: Option<i64>,
    number: Option<String>,
    date: Option<String>,
    due_date: Option<String>,
    /// ISO 4217; EUR when unset.
    currency: Option<String>,
    /// EUR per unit of the currency.
    #[schema(value_type = Option<String>)]
    exchange_rate: Option<Decimal>,
    note: Option<String>,
    #[serde(default)]
    lines: Vec<ReceiptLineInput>,
}

#[utoipa::path(put, path = "/receipts/{id}", tag = "books", request_body = ReceiptInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Receipt), (status = "default", body = ApiError)))]
pub async fn put_receipt(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<ReceiptInput>,
) -> Result<Json<Receipt>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = draft(&mut tx, id).await?;
    let mut check = Check::default();
    let number = check.text("number", input.number, 100);
    let note = check.text("note", input.note, 1000);
    for (key, date) in [("date", &input.date), ("due_date", &input.due_date)] {
        if date
            .as_deref()
            .is_some_and(|d| d.parse::<jiff::civil::Date>().is_err())
        {
            check.fail(key, FieldError::InvalidFormat);
        }
    }
    let currency = input
        .currency
        .unwrap_or_else(|| DEFAULT_CURRENCY.to_owned());
    if !is_code(&currency) {
        check.fail("currency", FieldError::InvalidFormat);
    }
    let exchange_rate = input.exchange_rate.filter(|_| currency != DEFAULT_CURRENCY);
    if exchange_rate
        .is_some_and(|r| r <= Decimal::ZERO || r > Decimal::from(1_000_000) || r.scale() > 6)
    {
        check.fail("exchange_rate", FieldError::InvalidFormat);
    }
    if let Some(supplier) = input.supplier_id {
        let known: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM supplier WHERE id = ?")
            .bind(supplier)
            .fetch_one(&mut *tx)
            .await?;
        if !known {
            check.fail("supplier_id", FieldError::InvalidFormat);
        }
    }
    if input.lines.len() > MAX_LINES {
        check.fail("lines", FieldError::TooLong);
    }
    let mut lines = Vec::new();
    for (i, line) in input.lines.into_iter().enumerate() {
        let description = check.required(&format!("lines.{i}.description"), line.description, 500);
        if let Some(account) = line.account_id {
            let open: bool = sqlx::query_scalar(
                "SELECT count(*) > 0 FROM ledger_account WHERE id = ? AND NOT archived",
            )
            .bind(account)
            .fetch_one(&mut *tx)
            .await?;
            if !open {
                check.fail(&format!("lines.{i}.account_id"), FieldError::InvalidFormat);
            }
        }
        if line.net.abs() > MAX_AMOUNT {
            check.fail(&format!("lines.{i}.net"), FieldError::InvalidFormat);
        }
        if line.vat_rate.is_sign_negative()
            || line.vat_rate > Decimal::ONE_HUNDRED
            || line.vat_rate.scale() > 2
        {
            check.fail(&format!("lines.{i}.vat_rate"), FieldError::InvalidFormat);
        }
        let tax = line.tax.unwrap_or_else(|| {
            (Decimal::from(line.net) * line.vat_rate / Decimal::ONE_HUNDRED)
                .round()
                .try_into()
                .unwrap_or(0)
        });
        if tax.abs() > MAX_AMOUNT {
            check.fail(&format!("lines.{i}.tax"), FieldError::InvalidFormat);
        }
        lines.push((
            description,
            line.account_id,
            line.net,
            line.vat_rate.normalize(),
            tax,
        ));
    }
    check.done()?;
    sqlx::query(
        "UPDATE receipt SET supplier_id = ?, number = ?, date = ?, due_date = ?, currency = ?,
         exchange_rate = ?, note = ?, updated_at = ? WHERE id = ?",
    )
    .bind(input.supplier_id)
    .bind(&number)
    .bind(&input.date)
    .bind(&input.due_date)
    .bind(&currency)
    .bind(exchange_rate.map(|r| r.to_string()))
    .bind(&note)
    .bind(now())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM receipt_line WHERE receipt_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for (position, (description, account, net, rate, tax)) in lines.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO receipt_line (receipt_id, position, description, account_id, net,
             vat_rate, tax) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(i64::try_from(position).expect("lines fit"))
        .bind(description)
        .bind(account)
        .bind(net)
        .bind(rate.to_string())
        .bind(tax)
        .execute(&mut *tx)
        .await?;
    }
    let after = find(&mut tx, id).await?;
    if let Some((old, new)) = audit::changes(&snapshot(&before), &snapshot(&after)) {
        audit::record(
            &mut tx,
            auth.user.id,
            "receipt.update",
            &format!("receipt:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(after))
}

/// What the audit log compares of a draft.
fn snapshot(receipt: &Receipt) -> serde_json::Value {
    serde_json::json!({
        "supplier_id": receipt.supplier_id, "number": receipt.number, "date": receipt.date,
        "due_date": receipt.due_date, "currency": receipt.currency,
        "exchange_rate": receipt.exchange_rate, "note": receipt.note,
        "lines": receipt.lines.iter().map(|l| serde_json::json!({
            "description": l.description, "account_id": l.account_id, "net": l.net,
            "vat_rate": l.vat_rate, "tax": l.tax
        })).collect::<Vec<_>>()
    })
}

async fn draft(db: &mut SqliteConnection, id: i64) -> Result<Receipt, ApiError> {
    let receipt = find(db, id).await?;
    if receipt.status != ReceiptStatus::Draft {
        return Err(ApiError::new(ErrorCode::Issued));
    }
    Ok(receipt)
}

#[utoipa::path(delete, path = "/receipts/{id}", tag = "books", params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn delete_receipt(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = draft(&mut tx, id).await?;
    sqlx::query("DELETE FROM receipt WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "receipt.delete",
        &format!("receipt:{id}"),
        Some(snapshot(&before)),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BookInput {
    /// The account it was paid from on its date, such as the cash box for a ticket; open until
    /// paid when unset.
    paid_from: Option<i64>,
}

/// Books a draft; it never changes afterwards.
#[utoipa::path(post, path = "/receipts/{id}/book", tag = "books", request_body = BookInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Receipt), (status = "default", body = ApiError)))]
pub async fn book_receipt(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<BookInput>,
) -> Result<Json<Receipt>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    started(&mut tx).await?;
    let receipt = draft(&mut tx, id).await?;
    if !receipt.missing.is_empty() {
        return Err(ApiError::fields(receipt.missing));
    }
    if let Some(account) = input.paid_from {
        payment_account(&mut tx, account).await?;
    }
    rules::post_receipt(&mut tx, &business, auth.user.id, id)
        .await
        .map_err(rule_error)?;
    let at = now();
    sqlx::query("UPDATE receipt SET status = 'booked', booked_at = ?, booked_by = ? WHERE id = ?")
        .bind(at)
        .bind(auth.user.id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE supplier SET account_id = ? WHERE id = ? AND account_id IS NULL")
        .bind(receipt.lines[0].account_id)
        .bind(receipt.supplier_id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "receipt.book",
        &format!("receipt:{id}"),
        None,
        Some(snapshot(&receipt)),
    )
    .await?;
    if let Some(account) = input.paid_from {
        let payment = NewReceiptPayment {
            receipt: id,
            date: receipt.date.clone().expect("a booked receipt has its date"),
            amount: receipt.gross,
            eur_amount: eur_of(&receipt, receipt.gross),
            account,
            note: None,
            bank_match: None,
        };
        pay(&mut tx, auth.user.id, payment).await?;
    }
    let receipt = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(receipt))
}

fn eur_of(receipt: &Receipt, amount: i64) -> Option<i64> {
    let rate: Decimal = receipt.exchange_rate.as_deref()?.parse().ok()?;
    (Decimal::from(amount) * rate).round().try_into().ok()
}

async fn payment_account(db: &mut SqliteConnection, account: i64) -> Result<(), ApiError> {
    let open: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM ledger_account WHERE id = ? AND type = 'asset' AND NOT archived",
    )
    .bind(account)
    .fetch_one(db)
    .await?;
    if open {
        Ok(())
    } else {
        Err(field_error("account_id", FieldError::InvalidFormat))
    }
}

#[derive(Debug, Serialize)]
pub struct NewReceiptPayment {
    pub receipt: i64,
    pub date: String,
    pub amount: i64,
    pub eur_amount: Option<i64>,
    pub account: i64,
    pub note: Option<String>,
    pub bank_match: Option<i64>,
}

/// Records and books a payment of a receipt.
pub async fn pay(
    tx: &mut SqliteConnection,
    user_id: i64,
    payment: NewReceiptPayment,
) -> Result<i64, ApiError> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO receipt_payment (receipt_id, date, amount, eur_amount, account_id, note,
         user_id, created_at, bank_match_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(payment.receipt)
    .bind(&payment.date)
    .bind(payment.amount)
    .bind(payment.eur_amount)
    .bind(payment.account)
    .bind(&payment.note)
    .bind(user_id)
    .bind(now())
    .bind(payment.bank_match)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        tx,
        user_id,
        "receipt_payment.create",
        &format!("receipt:{}", payment.receipt),
        None,
        Some(serde_json::to_value(&payment).map_err(ApiError::internal)?),
    )
    .await?;
    rules::post_receipt_payment(tx, user_id, id)
        .await
        .map_err(rule_error)?;
    Ok(id)
}

/// Reverses a receipt booked by mistake; its payments must be reversed first.
#[utoipa::path(post, path = "/receipts/{id}/reverse", tag = "books", params(("id" = i64, Path)),
    responses((status = OK, body = Receipt), (status = "default", body = ApiError)))]
pub async fn reverse_receipt(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<Json<Receipt>, ApiError> {
    let today = today(&business(&app).await?)?.to_string();
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let receipt = find(&mut tx, id).await?;
    if receipt.status != ReceiptStatus::Booked || receipt.reversed {
        return Err(ApiError::new(ErrorCode::AlreadyReversed));
    }
    if !receipt.payments.is_empty() {
        return Err(ApiError::new(ErrorCode::HasPayments));
    }
    let date = receipt.date.clone().unwrap_or_default().max(today);
    rules::reverse_all(&mut tx, auth.user.id, Column::Receipt, id, &date).await?;
    sqlx::query(
        "INSERT INTO receipt_reversal (receipt_id, reversed_at, reversed_by) VALUES (?, ?, ?)",
    )
    .bind(id)
    .bind(now())
    .bind(auth.user.id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "receipt.reverse",
        &format!("receipt:{id}"),
        None,
        Some(serde_json::json!({ "date": date })),
    )
    .await?;
    let receipt = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(receipt))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiptPaymentInput {
    date: String,
    /// In the minor unit of the receipt's currency; negative for a refund.
    amount: i64,
    /// EUR cents that left; required for a receipt in another currency.
    eur_amount: Option<i64>,
    /// The account it left, such as the bank or the cash box; the bank when unset.
    account_id: Option<i64>,
    note: Option<String>,
}

/// Records money paid for a booked receipt.
#[utoipa::path(post, path = "/receipts/{id}/payments", tag = "books",
    request_body = ReceiptPaymentInput, params(("id" = i64, Path)),
    responses((status = OK, body = Receipt), (status = "default", body = ApiError)))]
pub async fn add_receipt_payment(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<ReceiptPaymentInput>,
) -> Result<Json<Receipt>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let books = started(&mut tx).await?;
    let receipt = find(&mut tx, id).await?;
    if receipt.status != ReceiptStatus::Booked || receipt.reversed {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    let mut check = Check::default();
    super::books::check_date(&mut check, "date", &input.date, &books);
    if input.amount == 0 || input.amount.abs() > MAX_AMOUNT {
        check.fail("amount", FieldError::InvalidFormat);
    }
    let note = check.text("note", input.note, 500);
    let foreign = receipt.currency != DEFAULT_CURRENCY;
    let eur_amount = match (foreign, input.eur_amount) {
        (false, _) => None,
        (true, None) => {
            check.fail("eur_amount", FieldError::Required);
            None
        }
        (true, Some(eur))
            if eur == 0 || eur.abs() > MAX_AMOUNT || eur.signum() != input.amount.signum() =>
        {
            check.fail("eur_amount", FieldError::InvalidFormat);
            None
        }
        (true, eur) => eur,
    };
    check.done()?;
    let account = match input.account_id {
        Some(account) => {
            payment_account(&mut tx, account).await?;
            account
        }
        None => books::account(&mut tx, "bank")
            .await?
            .ok_or(ApiError::new(ErrorCode::NoLedgerAccount).with_detail("bank".to_owned()))?,
    };
    let payment = NewReceiptPayment {
        receipt: id,
        date: input.date,
        amount: input.amount,
        eur_amount,
        account,
        note,
        bank_match: None,
    };
    pay(&mut tx, auth.user.id, payment).await?;
    let receipt = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(receipt))
}

/// Reverses a payment of a receipt recorded by mistake, with its postings.
#[utoipa::path(post, path = "/receipt-payments/{id}/reverse", tag = "books",
    params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn reverse_receipt_payment(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let today = today(&business(&app).await?)?.to_string();
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let payment: rules::Recorded = sqlx::query_as(
        "SELECT receipt_id AS owner, date, amount, reversed_at, bank_match_id FROM receipt_payment
         WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::new(ErrorCode::NotFound))?;
    if payment.reversed_at.is_some() {
        return Err(ApiError::new(ErrorCode::AlreadyReversed));
    }
    if payment.bank_match_id.is_some() {
        return Err(ApiError::new(ErrorCode::BankMatched));
    }
    let (receipt, date, amount) = (payment.owner, payment.date, payment.amount);
    sqlx::query("UPDATE receipt_payment SET reversed_at = ?, reversed_by = ? WHERE id = ?")
        .bind(now())
        .bind(auth.user.id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let on = today.max(date.clone());
    rules::reverse_all(&mut tx, auth.user.id, Column::ReceiptPayment, id, &on).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "receipt_payment.reverse",
        &format!("receipt:{receipt}"),
        Some(serde_json::json!({ "payment": id, "date": date, "amount": amount })),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The uploaded file; an e-invoice's XML as plain text, so that it never runs as a page.
#[utoipa::path(get, path = "/receipts/{id}/file", tag = "books", params(("id" = i64, Path)),
    responses((status = OK, content_type = "application/octet-stream", body = Vec<u8>),
        (status = "default", body = ApiError)))]
pub async fn receipt_file(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let file: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT file, file_type FROM receipt WHERE id = ?")
            .bind(id)
            .fetch_optional(&app.db)
            .await?;
    let (Some(hash), kind) = file.ok_or(ApiError::new(ErrorCode::NotFound))? else {
        return Err(ApiError::new(ErrorCode::NotFound));
    };
    let bytes = app
        .files
        .get(hash.parse().map_err(ApiError::internal)?)
        .await
        .map_err(ApiError::internal)?;
    let kind = kind
        .as_deref()
        .filter(|k| {
            matches!(
                *k,
                "application/pdf" | "image/jpeg" | "image/png" | "image/webp"
            )
        })
        .unwrap_or("text/plain; charset=utf-8");
    Ok((
        [
            (header::CONTENT_TYPE, kind.to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"receipt-{id}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}
