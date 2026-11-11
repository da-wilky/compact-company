use std::{collections::BTreeMap, sync::Arc};

use axum::extract::{Multipart, State, multipart::MultipartRejection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    books::{check_date, started},
    business::is_iban,
    documents::{business, rule_error, today},
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::{Check, field_error},
    overview::amount_due,
    receipts::{self, NewReceiptPayment},
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    books::{self, Line, NewPosting, Source},
    money::{DEFAULT_CURRENCY, MAX_AMOUNT},
    rules::{self, Column},
    statements::{self, Format},
};

const MAX_FILE: usize = 20 << 20;

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct BankAccount {
    id: i64,
    name: String,
    iban: Option<String>,
    ledger_account_id: i64,
    ledger_account: String,
    archived: bool,
    /// EUR cents on its ledger account.
    balance: i64,
    /// Imported transactions not matched yet.
    open: i64,
}

const BANK_ACCOUNT: &str = "SELECT b.id, b.name, b.iban, b.ledger_account_id,
    a.number || ' ' || a.name AS ledger_account, b.archived,
    coalesce((SELECT sum(amount) FROM posting_line WHERE account_id = a.id), 0) AS balance,
    (SELECT count(*) FROM bank_transaction t WHERE t.bank_account_id = b.id
     AND NOT EXISTS (SELECT 1 FROM bank_match m WHERE m.bank_transaction_id = t.id
                     AND m.undone_at IS NULL)) AS open
    FROM bank_account b JOIN ledger_account a ON a.id = b.ledger_account_id";

#[utoipa::path(get, path = "/bank-accounts", tag = "bank",
    responses((status = OK, body = Vec<BankAccount>), (status = "default", body = ApiError)))]
pub async fn list_bank_accounts(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
) -> Result<Json<Vec<BankAccount>>, ApiError> {
    let sql = format!("{BANK_ACCOUNT} ORDER BY b.archived, b.name COLLATE NOCASE");
    Ok(Json(
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_all(&app.db)
            .await?,
    ))
}

async fn bank_account(db: &mut SqliteConnection, id: i64) -> Result<BankAccount, ApiError> {
    let sql = format!("{BANK_ACCOUNT} WHERE b.id = ?");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BankAccountInput {
    name: Option<String>,
    iban: Option<String>,
    /// An asset account no other bank account uses, such as 1200 Bank.
    ledger_account_id: i64,
}

#[utoipa::path(post, path = "/bank-accounts", tag = "bank", request_body = BankAccountInput,
    responses((status = OK, body = BankAccount), (status = "default", body = ApiError)))]
pub async fn create_bank_account(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<BankAccountInput>,
) -> Result<Json<BankAccount>, ApiError> {
    let mut check = Check::default();
    let name = check.required("name", input.name, 100);
    let iban = check
        .text("iban", input.iban, 42)
        .map(|v| v.replace(' ', "").to_ascii_uppercase());
    if iban.as_deref().is_some_and(|i| !is_iban(i)) {
        check.fail("iban", FieldError::InvalidFormat);
    }
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    started(&mut tx).await?;
    let free: bool = sqlx::query_scalar(
        "SELECT count(*) > 0 FROM ledger_account a WHERE a.id = ? AND a.type = 'asset'
         AND NOT a.archived AND NOT EXISTS (SELECT 1 FROM bank_account b
                                            WHERE b.ledger_account_id = a.id)",
    )
    .bind(input.ledger_account_id)
    .fetch_one(&mut *tx)
    .await?;
    if !free {
        check.fail("ledger_account_id", FieldError::InvalidFormat);
    }
    let taken: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM bank_account WHERE iban = ?")
        .bind(&iban)
        .fetch_one(&mut *tx)
        .await?;
    if taken {
        check.fail("iban", FieldError::Taken);
    }
    check.done()?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO bank_account (name, iban, ledger_account_id, created_at) VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&name)
    .bind(&iban)
    .bind(input.ledger_account_id)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "bank_account.create",
        &format!("bank_account:{id}"),
        None,
        Some(serde_json::json!({
            "name": name, "iban": iban, "ledger_account_id": input.ledger_account_id
        })),
    )
    .await?;
    let account = bank_account(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(account))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BankAccountChange {
    name: Option<String>,
    #[serde(default)]
    archived: bool,
}

/// Renames or archives a bank account; its IBAN and ledger account stay.
#[utoipa::path(put, path = "/bank-accounts/{id}", tag = "bank", request_body = BankAccountChange,
    params(("id" = i64, Path)),
    responses((status = OK, body = BankAccount), (status = "default", body = ApiError)))]
pub async fn put_bank_account(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<BankAccountChange>,
) -> Result<Json<BankAccount>, ApiError> {
    let mut check = Check::default();
    let name = check.required("name", input.name, 100);
    check.done()?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = bank_account(&mut tx, id).await?;
    sqlx::query("UPDATE bank_account SET name = ?, archived = ? WHERE id = ?")
        .bind(&name)
        .bind(input.archived)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "bank_account.update",
        &format!("bank_account:{id}"),
        Some(serde_json::json!({ "name": before.name, "archived": before.archived })),
        Some(serde_json::json!({ "name": name, "archived": input.archived })),
    )
    .await?;
    let account = bank_account(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(account))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ImportResult {
    format: Format,
    /// Transactions new to the account.
    imported: i64,
    /// Transactions already imported from an earlier statement.
    skipped: i64,
}

#[derive(ToSchema)]
#[allow(
    dead_code,
    reason = "describes the multipart body for the OpenAPI document"
)]
pub struct StatementUpload {
    /// A CAMT.053 statement or a bank's CSV export.
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

/// Imports a statement; a transaction already imported from an overlapping one is kept once.
#[utoipa::path(post, path = "/bank-accounts/{id}/import", tag = "bank",
    request_body(content_type = "multipart/form-data", content = StatementUpload),
    params(("id" = i64, Path)),
    responses((status = OK, body = ImportResult), (status = "default", body = ApiError)))]
pub async fn import_statement(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<ImportResult>, ApiError> {
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
            file = Some((name, field.bytes().await.map_err(malformed)?));
        }
    }
    let (name, bytes) = file.ok_or_else(|| field_error("file", FieldError::Required))?;
    if bytes.len() > MAX_FILE {
        return Err(field_error("file", FieldError::TooLong));
    }
    let (format, statement) = statements::read(&bytes).map_err(|error| {
        field_error("file", FieldError::InvalidFormat).with_detail(error.to_string())
    })?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let account = bank_account(&mut tx, id).await?;
    if let (Some(own), Some(theirs)) = (&account.iban, &statement.iban)
        && own != theirs
    {
        return Err(field_error("file", FieldError::InvalidFormat)
            .with_detail(format!("the statement is for {theirs}")));
    }
    let hash = app.files.put(bytes).await.map_err(ApiError::internal)?;
    let import: i64 = sqlx::query_scalar(
        "INSERT INTO bank_import (bank_account_id, file, file_name, format, imported, skipped,
         user_id, created_at) VALUES (?, ?, ?, ?, 0, 0, ?, ?) RETURNING id",
    )
    .bind(id)
    .bind(hash.to_string())
    .bind(&name)
    .bind(format)
    .bind(auth.user.id)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let (mut imported, mut skipped) = (0, 0);
    for entry in &statement.entries {
        let key = [
            entry.booking_date.as_str(),
            &entry.amount.to_string(),
            entry.counterparty.as_deref().unwrap_or_default(),
            entry.counterparty_iban.as_deref().unwrap_or_default(),
            entry.purpose.as_deref().unwrap_or_default(),
            entry.reference.as_deref().unwrap_or_default(),
        ]
        .join("\u{1f}");
        // identical movements on one day are told apart by their order in the statement
        let occurrence = seen.entry(key.clone()).or_default();
        *occurrence += 1;
        let fingerprint = hex::encode(Sha256::digest(format!("{key}\u{1f}{occurrence}")));
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO bank_transaction (bank_account_id, import_id, booking_date,
             value_date, amount, counterparty, counterparty_iban, purpose, reference, fingerprint)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(import)
        .bind(&entry.booking_date)
        .bind(&entry.value_date)
        .bind(entry.amount)
        .bind(&entry.counterparty)
        .bind(&entry.counterparty_iban)
        .bind(&entry.purpose)
        .bind(&entry.reference)
        .bind(fingerprint)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 1 {
            imported += 1;
        } else {
            skipped += 1;
        }
    }
    sqlx::query("UPDATE bank_import SET imported = ?, skipped = ? WHERE id = ?")
        .bind(imported)
        .bind(skipped)
        .bind(import)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "bank.import",
        &format!("bank_account:{id}"),
        None,
        Some(serde_json::json!({
            "import": import, "file": hash.to_string(), "imported": imported, "skipped": skipped
        })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(ImportResult {
        format,
        imported,
        skipped,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Invoice,
    Receipt,
    Account,
}

/// What part of a bank transaction pays, in EUR cents signed as the transaction.
#[derive(Debug, Serialize, ToSchema)]
pub struct MatchPart {
    kind: PartKind,
    /// The invoice, receipt or ledger account.
    id: i64,
    label: String,
    amount: i64,
    /// In the minor unit of the invoice's or receipt's currency.
    document_amount: Option<i64>,
    currency: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct BankTransaction {
    id: i64,
    bank_account_id: i64,
    /// ISO dates.
    booking_date: String,
    value_date: Option<String>,
    /// EUR cents, money in positive.
    amount: i64,
    counterparty: Option<String>,
    counterparty_iban: Option<String>,
    purpose: Option<String>,
    reference: Option<String>,
    match_id: Option<i64>,
    #[sqlx(skip)]
    parts: Vec<MatchPart>,
}

const TRANSACTION: &str = "SELECT t.id, t.bank_account_id, t.booking_date, t.value_date, t.amount,
    t.counterparty, t.counterparty_iban, t.purpose, t.reference,
    (SELECT m.id FROM bank_match m WHERE m.bank_transaction_id = t.id AND m.undone_at IS NULL)
    AS match_id FROM bank_transaction t";

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TransactionFilter {
    bank_account_id: Option<i64>,
    /// Only transactions not matched yet.
    #[serde(default)]
    open: bool,
    /// ISO dates, inclusive.
    from: Option<String>,
    to: Option<String>,
}

/// Bank transactions, newest first.
#[utoipa::path(get, path = "/bank-transactions", tag = "bank", params(TransactionFilter),
    responses((status = OK, body = Vec<BankTransaction>), (status = "default", body = ApiError)))]
pub async fn list_transactions(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(filter): Query<TransactionFilter>,
) -> Result<Json<Vec<BankTransaction>>, ApiError> {
    let mut db = app.db.acquire().await?;
    let sql = format!(
        "SELECT * FROM ({TRANSACTION}) WHERE (?1 IS NULL OR bank_account_id = ?1)
         AND (NOT ?2 OR match_id IS NULL) AND (?3 IS NULL OR booking_date >= ?3)
         AND (?4 IS NULL OR booking_date <= ?4) ORDER BY booking_date DESC, id DESC"
    );
    let mut transactions: Vec<BankTransaction> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(filter.bank_account_id)
        .bind(filter.open)
        .bind(filter.from)
        .bind(filter.to)
        .fetch_all(&mut *db)
        .await?;
    for transaction in &mut transactions {
        if let Some(id) = transaction.match_id {
            transaction.parts = parts(&mut db, id).await?;
        }
    }
    Ok(Json(transactions))
}

async fn transaction(db: &mut SqliteConnection, id: i64) -> Result<BankTransaction, ApiError> {
    let sql = format!("{TRANSACTION} WHERE t.id = ?");
    let mut transaction: BankTransaction = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    if let Some(id) = transaction.match_id {
        transaction.parts = parts(db, id).await?;
    }
    Ok(transaction)
}

async fn parts(db: &mut SqliteConnection, match_id: i64) -> Result<Vec<MatchPart>, sqlx::Error> {
    let mut parts = Vec::new();
    let invoices: Vec<(i64, String, i64, i64, String)> = sqlx::query_as(
        "SELECT p.document_id, d.number, coalesce(p.eur_amount, p.amount), p.amount, d.currency
         FROM payment p JOIN document d ON d.id = p.document_id
         WHERE p.bank_match_id = ? ORDER BY p.id",
    )
    .bind(match_id)
    .fetch_all(&mut *db)
    .await?;
    for (id, number, eur, amount, currency) in invoices {
        parts.push(MatchPart {
            kind: PartKind::Invoice,
            id,
            label: number,
            amount: eur,
            document_amount: Some(amount),
            currency: Some(currency),
        });
    }
    let receipts: Vec<(i64, String, Option<String>, i64, i64, String)> = sqlx::query_as(
        "SELECT y.receipt_id, s.name, r.number, coalesce(y.eur_amount, y.amount), y.amount,
         r.currency FROM receipt_payment y JOIN receipt r ON r.id = y.receipt_id
         JOIN supplier s ON s.id = r.supplier_id WHERE y.bank_match_id = ? ORDER BY y.id",
    )
    .bind(match_id)
    .fetch_all(&mut *db)
    .await?;
    for (id, supplier, number, eur, amount, currency) in receipts {
        parts.push(MatchPart {
            kind: PartKind::Receipt,
            id,
            label: number.map_or(supplier.clone(), |n| format!("{supplier} {n}")),
            amount: -eur,
            document_amount: Some(amount),
            currency: Some(currency),
        });
    }
    let accounts: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT a.id, a.number || ' ' || a.name, -l.amount FROM posting p
         JOIN posting_line l ON l.posting_id = p.id JOIN ledger_account a ON a.id = l.account_id
         WHERE p.bank_match_id = ? AND p.reverses_id IS NULL
         AND a.id NOT IN (SELECT ledger_account_id FROM bank_account) ORDER BY l.id",
    )
    .bind(match_id)
    .fetch_all(&mut *db)
    .await?;
    for (id, label, amount) in accounts {
        parts.push(MatchPart {
            kind: PartKind::Account,
            id,
            label,
            amount,
            document_amount: None,
            currency: None,
        });
    }
    Ok(parts)
}

/// An open invoice or receipt the transaction may pay, best first.
#[derive(Debug, Serialize, ToSchema)]
pub struct Candidate {
    kind: PartKind,
    id: i64,
    label: String,
    /// ISO date.
    date: String,
    /// Still owed, in the minor unit of its currency.
    due: i64,
    currency: String,
    /// EUR cents of the transaction proposed for it, signed as the transaction.
    amount: i64,
    /// How well it fits: the number in the purpose, the amount, the name or IBAN.
    score: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BankTransactionDetail {
    #[serde(flatten)]
    transaction: BankTransaction,
    /// For a transaction not matched yet.
    candidates: Vec<Candidate>,
}

#[utoipa::path(get, path = "/bank-transactions/{id}", tag = "bank", params(("id" = i64, Path)),
    responses((status = OK, body = BankTransactionDetail), (status = "default", body = ApiError)))]
pub async fn get_transaction(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(id): Path<i64>,
) -> Result<Json<BankTransactionDetail>, ApiError> {
    let mut db = app.db.acquire().await?;
    detail(&mut db, id).await.map(Json)
}

async fn detail(db: &mut SqliteConnection, id: i64) -> Result<BankTransactionDetail, ApiError> {
    let transaction = transaction(db, id).await?;
    let candidates = match transaction.match_id {
        Some(_) => Vec::new(),
        None => candidates(db, &transaction).await?,
    };
    Ok(BankTransactionDetail {
        transaction,
        candidates,
    })
}

/// Lowercase letters and digits only, so `RE-2026-001` is found in `re 2026 001`.
fn plain(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn score(
    transaction: &BankTransaction,
    number: Option<&str>,
    name: &str,
    iban: Option<&str>,
    due: Option<i64>,
) -> u32 {
    let purpose = plain(transaction.purpose.as_deref().unwrap_or_default());
    let counterparty = plain(transaction.counterparty.as_deref().unwrap_or_default());
    let mut score = 0;
    if number
        .map(plain)
        .is_some_and(|n| n.len() >= 3 && purpose.contains(&n))
    {
        score += 50;
    }
    if due.is_some_and(|d| d == transaction.amount.abs()) {
        score += 30;
    }
    if iban.is_some_and(|i| Some(i) == transaction.counterparty_iban.as_deref()) {
        score += 30;
    }
    let word = name.split_whitespace().map(plain).find(|w| w.len() >= 4);
    if word.is_some_and(|w| counterparty.contains(&w) || purpose.contains(&w)) {
        score += 15;
    }
    score
}

async fn candidates(
    db: &mut SqliteConnection,
    transaction: &BankTransaction,
) -> Result<Vec<Candidate>, ApiError> {
    let sign = transaction.amount.signum();
    let mut candidates = Vec::new();
    if sign > 0 {
        let invoices: Vec<(i64, String, String, String)> = sqlx::query_as(
            "SELECT d.id, d.number, d.issue_date, c.name FROM document d
             JOIN customer c ON c.id = d.customer_id
             WHERE d.kind = 'invoice' AND d.status = 'issued'",
        )
        .fetch_all(&mut *db)
        .await?;
        for (id, number, date, customer) in invoices {
            let due = amount_due(db, id).await?;
            if due.minor <= 0 {
                continue;
            }
            let eur = (due.currency == DEFAULT_CURRENCY).then_some(due.minor);
            candidates.push(Candidate {
                kind: PartKind::Invoice,
                id,
                score: score(transaction, Some(&number), &customer, None, eur),
                label: format!("{number} {customer}"),
                date,
                due: due.minor,
                amount: eur.map_or(transaction.amount, |e| e.min(transaction.amount)),
                currency: due.currency,
            });
        }
    } else {
        let receipts: Vec<OpenReceipt> = sqlx::query_as(
            "SELECT r.id, r.number, r.date, s.name AS supplier, s.iban, r.currency,
                 (SELECT sum(net + tax) FROM receipt_line WHERE receipt_id = r.id)
                 - coalesce((SELECT sum(amount) FROM receipt_payment
                             WHERE receipt_id = r.id AND reversed_at IS NULL), 0) AS due
                 FROM receipt r JOIN supplier s ON s.id = r.supplier_id
                 WHERE r.status = 'booked'
                 AND NOT EXISTS (SELECT 1 FROM receipt_reversal v WHERE v.receipt_id = r.id)",
        )
        .fetch_all(&mut *db)
        .await?;
        for OpenReceipt {
            id,
            number,
            date,
            supplier,
            iban,
            currency,
            due,
        } in receipts
        {
            if due <= 0 {
                continue;
            }
            let eur = (currency == DEFAULT_CURRENCY).then_some(due);
            candidates.push(Candidate {
                kind: PartKind::Receipt,
                id,
                score: score(
                    transaction,
                    number.as_deref(),
                    &supplier,
                    iban.as_deref(),
                    eur,
                ),
                label: number.map_or(supplier.clone(), |n| format!("{supplier} {n}")),
                date,
                due,
                amount: eur.map_or(transaction.amount, |e| (-e).max(transaction.amount)),
                currency,
            });
        }
    }
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.date.cmp(&b.date)));
    Ok(candidates)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchPartInput {
    kind: PartKind,
    /// The invoice, receipt or ledger account.
    id: i64,
    /// EUR cents of the transaction, signed as the transaction.
    amount: i64,
    /// For an invoice or receipt in another currency: what it settles in that currency.
    document_amount: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchInput {
    /// Their amounts add up to the transaction's.
    parts: Vec<MatchPartInput>,
}

/// Matches a transaction: payments of the invoices and receipts it pays, the rest booked on
/// ledger accounts such as bank fees or private withdrawals.
#[utoipa::path(post, path = "/bank-transactions/{id}/match", tag = "bank",
    request_body = MatchInput, params(("id" = i64, Path)),
    responses((status = OK, body = BankTransactionDetail), (status = "default", body = ApiError)))]
pub async fn match_transaction(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<MatchInput>,
) -> Result<Json<BankTransactionDetail>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let books = started(&mut tx).await?;
    let transaction = transaction(&mut tx, id).await?;
    if transaction.match_id.is_some() {
        return Err(ApiError::new(ErrorCode::BankMatched));
    }
    let mut check = Check::default();
    check_date(
        &mut check,
        "booking_date",
        &transaction.booking_date,
        &books,
    );
    let bounded = |a: i64| a != 0 && a.abs() <= MAX_AMOUNT;
    if input.parts.is_empty()
        || input.parts.len() > 50
        || input
            .parts
            .iter()
            .any(|p| !bounded(p.amount) || p.document_amount.is_some_and(|d| !bounded(d)))
        || input.parts.iter().map(|p| p.amount).sum::<i64>() != transaction.amount
    {
        check.fail("parts", FieldError::InvalidFormat);
    }
    check.done()?;
    let bank: i64 = sqlx::query_scalar("SELECT ledger_account_id FROM bank_account WHERE id = ?")
        .bind(transaction.bank_account_id)
        .fetch_one(&mut *tx)
        .await?;
    let match_id: i64 = sqlx::query_scalar(
        "INSERT INTO bank_match (bank_transaction_id, user_id, created_at) VALUES (?, ?, ?)
         RETURNING id",
    )
    .bind(id)
    .bind(auth.user.id)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    let date = transaction.booking_date.clone();
    let mut lines = Vec::new();
    for (i, part) in input.parts.iter().enumerate() {
        let key = format!("parts.{i}");
        match part.kind {
            PartKind::Invoice => {
                let currency: Option<String> = sqlx::query_scalar(
                    "SELECT currency FROM document WHERE id = ? AND kind = 'invoice'
                     AND status = 'issued'",
                )
                .bind(part.id)
                .fetch_optional(&mut *tx)
                .await?;
                let currency =
                    currency.ok_or_else(|| field_error(&key, FieldError::InvalidFormat))?;
                let foreign = currency != DEFAULT_CURRENCY;
                let amount = if foreign {
                    part.document_amount
                } else {
                    Some(part.amount)
                };
                let Some(amount) = amount.filter(|a| *a > 0 && part.amount > 0) else {
                    return Err(field_error(&key, FieldError::InvalidFormat));
                };
                let payment: i64 = sqlx::query_scalar(
                    "INSERT INTO payment (document_id, date, amount, eur_amount, account_id,
                     user_id, created_at, bank_match_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                     RETURNING id",
                )
                .bind(part.id)
                .bind(&date)
                .bind(amount)
                .bind(foreign.then_some(part.amount))
                .bind(bank)
                .bind(auth.user.id)
                .bind(now())
                .bind(match_id)
                .fetch_one(&mut *tx)
                .await?;
                rules::post_payment(&mut tx, &business, auth.user.id, payment)
                    .await
                    .map_err(rule_error)?;
            }
            PartKind::Receipt => {
                let currency: Option<String> = sqlx::query_scalar(
                    "SELECT currency FROM receipt r WHERE id = ? AND status = 'booked'
                     AND NOT EXISTS (SELECT 1 FROM receipt_reversal v WHERE v.receipt_id = r.id)",
                )
                .bind(part.id)
                .fetch_optional(&mut *tx)
                .await?;
                let currency =
                    currency.ok_or_else(|| field_error(&key, FieldError::InvalidFormat))?;
                let foreign = currency != DEFAULT_CURRENCY;
                let amount = if foreign {
                    part.document_amount
                } else {
                    Some(-part.amount)
                };
                let Some(amount) = amount.filter(|a| *a != 0) else {
                    return Err(field_error(&key, FieldError::InvalidFormat));
                };
                let payment = NewReceiptPayment {
                    receipt: part.id,
                    date: date.clone(),
                    amount,
                    eur_amount: foreign.then_some(-part.amount),
                    account: bank,
                    note: None,
                    bank_match: Some(match_id),
                };
                receipts::pay(&mut tx, auth.user.id, payment).await?;
            }
            PartKind::Account => {
                let open: bool = sqlx::query_scalar(
                    "SELECT count(*) > 0 FROM ledger_account WHERE id = ? AND NOT archived",
                )
                .bind(part.id)
                .fetch_one(&mut *tx)
                .await?;
                if !open || part.id == bank {
                    return Err(field_error(&key, FieldError::InvalidFormat));
                }
                lines.push(Line {
                    account_id: bank,
                    amount: part.amount,
                });
                lines.push(Line {
                    account_id: part.id,
                    amount: -part.amount,
                });
            }
        }
    }
    if !lines.is_empty() {
        let description: String = [
            transaction.counterparty.as_deref(),
            transaction.purpose.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
        let posting = NewPosting {
            date: date.clone(),
            description: if description.is_empty() {
                "Bankumsatz".to_owned()
            } else {
                description
            },
            lines,
            source: Source {
                bank_match_id: Some(match_id),
                ..Source::default()
            },
        };
        books::book(&mut tx, auth.user.id, posting, None).await?;
    }
    audit::record(
        &mut tx,
        auth.user.id,
        "bank.match",
        &format!("bank_transaction:{id}"),
        None,
        Some(serde_json::json!({
            "match": match_id,
            "parts": input.parts.iter().map(|p| serde_json::json!({
                "kind": p.kind, "id": p.id, "amount": p.amount, "document_amount": p.document_amount
            })).collect::<Vec<_>>()
        })),
    )
    .await?;
    let detail = detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

/// Undoes a match: its payments and postings are reversed and the transaction is open again.
#[utoipa::path(post, path = "/bank-transactions/{id}/unmatch", tag = "bank",
    params(("id" = i64, Path)),
    responses((status = OK, body = BankTransactionDetail), (status = "default", body = ApiError)))]
pub async fn unmatch_transaction(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<Json<BankTransactionDetail>, ApiError> {
    let today = today(&business(&app).await?)?.to_string();
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let transaction = transaction(&mut tx, id).await?;
    let match_id = transaction
        .match_id
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let date = today.max(transaction.booking_date.clone());
    let at = now();
    let payments: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM payment WHERE bank_match_id = ? AND reversed_at IS NULL",
    )
    .bind(match_id)
    .fetch_all(&mut *tx)
    .await?;
    for payment in payments {
        sqlx::query("UPDATE payment SET reversed_at = ?, reversed_by = ? WHERE id = ?")
            .bind(at)
            .bind(auth.user.id)
            .bind(payment)
            .execute(&mut *tx)
            .await?;
        rules::reverse_all(&mut tx, auth.user.id, Column::Payment, payment, &date).await?;
    }
    let receipt_payments: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM receipt_payment WHERE bank_match_id = ? AND reversed_at IS NULL",
    )
    .bind(match_id)
    .fetch_all(&mut *tx)
    .await?;
    for payment in receipt_payments {
        sqlx::query("UPDATE receipt_payment SET reversed_at = ?, reversed_by = ? WHERE id = ?")
            .bind(at)
            .bind(auth.user.id)
            .bind(payment)
            .execute(&mut *tx)
            .await?;
        rules::reverse_all(
            &mut tx,
            auth.user.id,
            Column::ReceiptPayment,
            payment,
            &date,
        )
        .await?;
    }
    rules::reverse_all(&mut tx, auth.user.id, Column::BankMatch, match_id, &date).await?;
    sqlx::query("UPDATE bank_match SET undone_at = ?, undone_by = ? WHERE id = ?")
        .bind(at)
        .bind(auth.user.id)
        .bind(match_id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "bank.unmatch",
        &format!("bank_transaction:{id}"),
        Some(serde_json::json!({ "match": match_id })),
        None,
    )
    .await?;
    let detail = detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(sqlx::FromRow)]
struct OpenReceipt {
    id: i64,
    number: Option<String>,
    date: String,
    supplier: String,
    iban: Option<String>,
    currency: String,
    due: i64,
}
