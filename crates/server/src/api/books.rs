use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    documents::{business, today},
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::{Check, field_error},
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    books::{self, AccountType, Books, Chart, Line, NewPosting, Source},
    settings::Taxation,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct BooksView {
    /// None until the owner starts the books.
    books: Option<Books>,
    taxation: Taxation,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StartInput {
    chart: Chart,
    /// ISO date of the first day booked.
    start_date: String,
}

#[utoipa::path(get, path = "/books", tag = "books",
    responses((status = OK, body = BooksView), (status = "default", body = ApiError)))]
pub async fn get_books(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
) -> Result<Json<BooksView>, ApiError> {
    let mut db = app.db.acquire().await?;
    Ok(Json(BooksView {
        books: books::books(&mut db).await?,
        taxation: business(&app).await?.taxation(),
    }))
}

/// Starts the books on a chart of accounts, which never changes afterwards.
#[utoipa::path(post, path = "/books", tag = "books", request_body = StartInput,
    responses((status = OK, body = BooksView), (status = "default", body = ApiError)))]
pub async fn start_books(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<StartInput>,
) -> Result<Json<BooksView>, ApiError> {
    if input.start_date.parse::<jiff::civil::Date>().is_err() {
        return Err(field_error("start_date", FieldError::InvalidFormat));
    }
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    if books::books(&mut tx).await?.is_some() {
        return Err(ApiError::new(ErrorCode::BooksStarted));
    }
    sqlx::query(
        "INSERT INTO books (id, chart, start_date, started_at, started_by) VALUES (1, ?, ?, ?, ?)",
    )
    .bind(input.chart)
    .bind(&input.start_date)
    .bind(now())
    .bind(auth.user.id)
    .execute(&mut *tx)
    .await?;
    books::seed_accounts(&mut tx, input.chart).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "books.start",
        "books",
        None,
        Some(serde_json::json!({ "chart": input.chart, "start_date": input.start_date })),
    )
    .await?;
    let started = books::books(&mut tx).await?;
    tx.commit().await?;
    Ok(Json(BooksView {
        books: started,
        taxation: business.taxation(),
    }))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Account {
    id: i64,
    number: String,
    name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    kind: AccountType,
    /// What the posting rules book on it, such as `receivables` or `revenue:19`.
    role: Option<String>,
    archived: bool,
    /// EUR cents, debit positive.
    balance: i64,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AccountFilter {
    /// Archived accounts instead of active ones.
    #[serde(default)]
    archived: bool,
}

#[utoipa::path(get, path = "/accounts", tag = "books", params(AccountFilter),
    responses((status = OK, body = Vec<Account>), (status = "default", body = ApiError)))]
pub async fn list_accounts(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(filter): Query<AccountFilter>,
) -> Result<Json<Vec<Account>>, ApiError> {
    Ok(Json(
        sqlx::query_as(
            "SELECT a.id, a.number, a.name, a.type, a.role, a.archived,
             coalesce((SELECT sum(amount) FROM posting_line WHERE account_id = a.id), 0) AS balance
             FROM ledger_account a WHERE a.archived = ? ORDER BY a.number",
        )
        .bind(filter.archived)
        .fetch_all(&app.db)
        .await?,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountInput {
    number: Option<String>,
    name: Option<String>,
    #[serde(rename = "type")]
    kind: AccountType,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountChange {
    name: Option<String>,
    #[serde(default)]
    archived: bool,
}

#[utoipa::path(post, path = "/accounts", tag = "books", request_body = AccountInput,
    responses((status = OK, body = Account), (status = "default", body = ApiError)))]
pub async fn create_account(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<AccountInput>,
) -> Result<Json<Account>, ApiError> {
    let mut check = Check::default();
    let number = check.required("number", input.number, 8);
    if !(4..=8).contains(&number.len()) || !number.chars().all(|c| c.is_ascii_digit()) {
        check.fail("number", FieldError::InvalidFormat);
    }
    let name = check.required("name", input.name, 200);
    check.done()?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    started(&mut tx).await?;
    let taken: bool =
        sqlx::query_scalar("SELECT count(*) > 0 FROM ledger_account WHERE number = ?")
            .bind(&number)
            .fetch_one(&mut *tx)
            .await?;
    if taken {
        return Err(field_error("number", FieldError::Taken));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO ledger_account (number, name, type) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(&number)
    .bind(&name)
    .bind(input.kind)
    .fetch_one(&mut *tx)
    .await?;
    let account = account(&mut tx, id).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "account.create",
        &format!("account:{id}"),
        None,
        Some(serde_json::to_value(&account).map_err(ApiError::internal)?),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(account))
}

/// Renames or archives an account; its number and type stay as created.
#[utoipa::path(put, path = "/accounts/{id}", tag = "books", request_body = AccountChange,
    params(("id" = i64, Path)),
    responses((status = OK, body = Account), (status = "default", body = ApiError)))]
pub async fn put_account(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<AccountChange>,
) -> Result<Json<Account>, ApiError> {
    let mut check = Check::default();
    let name = check.required("name", input.name, 200);
    check.done()?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = account(&mut tx, id).await?;
    if input.archived && before.role.is_some() {
        return Err(field_error("archived", FieldError::Locked));
    }
    sqlx::query("UPDATE ledger_account SET name = ?, archived = ? WHERE id = ?")
        .bind(&name)
        .bind(input.archived)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let after = account(&mut tx, id).await?;
    if let Some((old, new)) = audit::changes(&before, &after) {
        audit::record(
            &mut tx,
            auth.user.id,
            "account.update",
            &format!("account:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(after))
}

async fn account(db: &mut SqliteConnection, id: i64) -> Result<Account, ApiError> {
    sqlx::query_as(
        "SELECT a.id, a.number, a.name, a.type, a.role, a.archived,
         coalesce((SELECT sum(amount) FROM posting_line WHERE account_id = a.id), 0) AS balance
         FROM ledger_account a WHERE a.id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::new(ErrorCode::NotFound))
}

pub async fn started(db: &mut SqliteConnection) -> Result<Books, ApiError> {
    books::books(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::BooksNotStarted))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PostingLine {
    account_id: i64,
    number: String,
    name: String,
    /// EUR cents, debit positive and credit negative.
    amount: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Posting {
    id: i64,
    /// ISO date.
    date: String,
    description: String,
    reverses_id: Option<i64>,
    reversed_by: Option<i64>,
    document_id: Option<i64>,
    document_number: Option<String>,
    payment_id: Option<i64>,
    user_name: String,
    created_at: i64,
    #[sqlx(skip)]
    lines: Vec<PostingLine>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PostingFilter {
    /// ISO date, inclusive.
    from: Option<String>,
    /// ISO date, inclusive.
    to: Option<String>,
    /// Only postings with a line on this account.
    account_id: Option<i64>,
}

const POSTING: &str = "SELECT p.id, p.date, p.description, p.reverses_id,
    (SELECT r.id FROM posting r WHERE r.reverses_id = p.id) AS reversed_by,
    p.document_id, d.number AS document_number, p.payment_id, u.name AS user_name, p.created_at
    FROM posting p JOIN user u ON u.id = p.user_id LEFT JOIN document d ON d.id = p.document_id";

/// The journal in order, oldest first.
#[utoipa::path(get, path = "/postings", tag = "books", params(PostingFilter),
    responses((status = OK, body = Vec<Posting>), (status = "default", body = ApiError)))]
pub async fn list_postings(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(filter): Query<PostingFilter>,
) -> Result<Json<Vec<Posting>>, ApiError> {
    let mut db = app.db.acquire().await?;
    let sql = format!(
        "{POSTING} WHERE (?1 IS NULL OR p.date >= ?1) AND (?2 IS NULL OR p.date <= ?2)
         AND (?3 IS NULL OR EXISTS (SELECT 1 FROM posting_line l
              WHERE l.posting_id = p.id AND l.account_id = ?3))
         ORDER BY p.date, p.id"
    );
    let mut postings: Vec<Posting> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(filter.from)
        .bind(filter.to)
        .bind(filter.account_id)
        .fetch_all(&mut *db)
        .await?;
    for posting in &mut postings {
        posting.lines = lines(&mut db, posting.id).await?;
    }
    Ok(Json(postings))
}

async fn lines(db: &mut SqliteConnection, id: i64) -> Result<Vec<PostingLine>, sqlx::Error> {
    sqlx::query_as(
        "SELECT l.account_id, a.number, a.name, l.amount FROM posting_line l
         JOIN ledger_account a ON a.id = l.account_id WHERE l.posting_id = ?
         ORDER BY l.amount < 0, l.id",
    )
    .bind(id)
    .fetch_all(db)
    .await
}

pub async fn find_posting(db: &mut SqliteConnection, id: i64) -> Result<Posting, ApiError> {
    let sql = format!("{POSTING} WHERE p.id = ?");
    let mut posting: Posting = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    posting.lines = lines(db, id).await?;
    Ok(posting)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PostingLineInput {
    account_id: i64,
    /// EUR cents, debit positive and credit negative.
    amount: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PostingInput {
    /// ISO date.
    date: String,
    description: Option<String>,
    lines: Vec<PostingLineInput>,
}

/// Books a posting by hand, such as an opening balance or a private deposit.
#[utoipa::path(post, path = "/postings", tag = "books", request_body = PostingInput,
    responses((status = OK, body = Posting), (status = "default", body = ApiError)))]
pub async fn create_posting(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<PostingInput>,
) -> Result<Json<Posting>, ApiError> {
    let mut check = Check::default();
    let description = check.required("description", input.description, 500);
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let books = started(&mut tx).await?;
    check_date(&mut check, "date", &input.date, &books);
    let mut sum: i64 = 0;
    for (i, line) in input.lines.iter().enumerate() {
        let open: bool = sqlx::query_scalar(
            "SELECT count(*) > 0 FROM ledger_account WHERE id = ? AND NOT archived",
        )
        .bind(line.account_id)
        .fetch_one(&mut *tx)
        .await?;
        if !open {
            check.fail(&format!("lines.{i}.account_id"), FieldError::InvalidFormat);
        }
        if line.amount == 0 || line.amount.abs() > 100_000_000_000 {
            check.fail(&format!("lines.{i}.amount"), FieldError::InvalidFormat);
        }
        sum = sum.saturating_add(line.amount);
    }
    if input.lines.len() < 2 || sum != 0 {
        check.fail("lines", FieldError::InvalidFormat);
    }
    check.done()?;
    let id = books::book(
        &mut tx,
        auth.user.id,
        NewPosting {
            date: input.date,
            description,
            lines: input
                .lines
                .iter()
                .map(|l| Line {
                    account_id: l.account_id,
                    amount: l.amount,
                })
                .collect(),
            source: Source::default(),
        },
        None,
    )
    .await?;
    let posting = find_posting(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(posting))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReverseInput {
    /// ISO date; today when unset.
    date: Option<String>,
}

/// Reverses a posting booked by hand; postings of documents and payments are reversed by
/// cancelling those.
#[utoipa::path(post, path = "/postings/{id}/reverse", tag = "books", request_body = ReverseInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Posting), (status = "default", body = ApiError)))]
pub async fn reverse_posting(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<ReverseInput>,
) -> Result<Json<Posting>, ApiError> {
    let date = match input.date {
        Some(date) => date,
        None => today(&business(&app).await?)?.to_string(),
    };
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let books = started(&mut tx).await?;
    let mut check = Check::default();
    check_date(&mut check, "date", &date, &books);
    check.done()?;
    let posting = find_posting(&mut tx, id).await?;
    if posting.reversed_by.is_some() || posting.reverses_id.is_some() {
        return Err(ApiError::new(ErrorCode::AlreadyReversed));
    }
    if posting.document_id.is_some() || posting.payment_id.is_some() {
        return Err(ApiError::new(ErrorCode::AutomaticPosting));
    }
    if date < posting.date {
        return Err(field_error("date", FieldError::InvalidFormat));
    }
    let reversal = books::reverse(&mut tx, auth.user.id, id, &date).await?;
    let reversal = find_posting(&mut tx, reversal).await?;
    tx.commit().await?;
    Ok(Json(reversal))
}

/// A posting date must be a date on or after the first day booked.
pub fn check_date(check: &mut Check, key: &str, date: &str, books: &Books) {
    if date.parse::<jiff::civil::Date>().is_err() || date < books.start_date.as_str() {
        check.fail(key, FieldError::InvalidFormat);
    }
}
