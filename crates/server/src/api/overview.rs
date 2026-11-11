use std::sync::Arc;

use axum::{extract::State, http::StatusCode};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use super::{
    billing::{UnbilledProject, unbilled},
    documents::{Document, business, find, row, rule_error, today},
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, Role, now},
    books,
    documents::{Data, Kind, Status},
    money::Money,
    rules,
};

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Payment {
    id: i64,
    /// ISO date the money arrived.
    date: String,
    /// In the minor unit of the invoice's currency.
    amount: i64,
    /// EUR cents that arrived, for an invoice in another currency.
    eur_amount: Option<i64>,
    /// The ledger account it arrived on; none before the books started.
    account_id: Option<i64>,
    note: Option<String>,
    user_name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PaymentInput {
    date: String,
    /// In the minor unit of the invoice's currency.
    amount: i64,
    /// EUR cents that arrived; required for an invoice in another currency once the books are
    /// started.
    eur_amount: Option<i64>,
    /// The ledger account it arrived on, such as the bank or the cash box; the bank when unset.
    account_id: Option<i64>,
    note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OpenInvoice {
    id: i64,
    number: String,
    customer_name: String,
    issue_date: String,
    due_date: Option<String>,
    overdue: bool,
    due: Money,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct RunningTimer {
    id: i64,
    user_name: String,
    project_name: String,
    customer_name: String,
    todo_title: Option<String>,
    description: Option<String>,
    started_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Overview {
    /// Everyone's for the owner, your own for staff.
    running: Vec<RunningTimer>,
    /// For the owner and accountant only.
    unbilled: Vec<UnbilledProject>,
    /// For the owner and accountant only, oldest due first.
    open_invoices: Vec<OpenInvoice>,
}

#[utoipa::path(get, path = "/overview", tag = "overview",
    responses((status = OK, body = Overview), (status = "default", body = ApiError)))]
pub async fn get_overview(
    State(app): State<Arc<App>>,
    auth: Auth,
) -> Result<Json<Overview>, ApiError> {
    let mut db = app.db.acquire().await?;
    let role = auth.user.role;
    let running = sqlx::query_as(
        "SELECT id, user_name, project_name, customer_name, todo_title, description, started_at
         FROM time_entry_view WHERE ended_at IS NULL AND (? OR user_id = ?)
         ORDER BY started_at",
    )
    .bind(role == Role::Owner)
    .bind(auth.user.id)
    .fetch_all(&mut *db)
    .await?;
    let money = matches!(role, Role::Owner | Role::Accountant);
    let (unbilled, open_invoices) = if money {
        let today = today(&business(&app).await?)?.to_string();
        (
            unbilled(&mut db, None).await?,
            open_invoices(&mut db, &today).await?,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    Ok(Json(Overview {
        running,
        unbilled,
        open_invoices,
    }))
}

async fn open_invoices(
    db: &mut SqliteConnection,
    today: &str,
) -> Result<Vec<OpenInvoice>, ApiError> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM document WHERE kind = 'invoice' AND status = 'issued'
         ORDER BY coalesce(json_extract(data, '$.due_date'), issue_date), number",
    )
    .fetch_all(&mut *db)
    .await?;
    let mut open = Vec::new();
    for id in ids {
        let due = amount_due(db, id).await?;
        if due.minor <= 0 {
            continue;
        }
        let (number, customer_name, issue_date, data): (String, String, String, String) =
            sqlx::query_as(
                "SELECT d.number, c.name, d.issue_date, d.data FROM document d
                 JOIN customer c ON c.id = d.customer_id WHERE d.id = ?",
            )
            .bind(id)
            .fetch_one(&mut *db)
            .await?;
        let data: Data = serde_json::from_str(&data).map_err(ApiError::internal)?;
        open.push(OpenInvoice {
            id,
            number,
            customer_name,
            issue_date,
            overdue: data.due_date.as_deref().is_some_and(|d| d < today),
            due_date: data.due_date,
            due,
        });
    }
    Ok(open)
}

fn gross_minor(data: &str) -> Result<i64, ApiError> {
    let data: Data = serde_json::from_str(data).map_err(ApiError::internal)?;
    let gross = data.totals.map_or(Decimal::ZERO, |t| t.gross);
    (gross * Decimal::ONE_HUNDRED)
        .round()
        .try_into()
        .map_err(ApiError::internal)
}

/// What is still owed on an issued invoice: its total less its payments and its cancellation
/// invoices.
pub async fn amount_due(db: &mut SqliteConnection, id: i64) -> Result<Money, ApiError> {
    let (currency, data): (String, String) =
        sqlx::query_as("SELECT currency, data FROM document WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *db)
            .await?;
    let mut due = gross_minor(&data)?;
    let paid: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(amount), 0) FROM payment WHERE document_id = ? AND reversed_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *db)
    .await?;
    due -= paid;
    let cancellations: Vec<String> = sqlx::query_scalar(
        "SELECT data FROM document WHERE source_id = ? AND kind = 'cancellation'
         AND status = 'issued'",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    for cancellation in cancellations {
        due += gross_minor(&cancellation)?;
    }
    Ok(Money {
        minor: due,
        currency,
    })
}

pub async fn payments(db: &mut SqliteConnection, id: i64) -> Result<Vec<Payment>, sqlx::Error> {
    sqlx::query_as(
        "SELECT p.id, p.date, p.amount, p.eur_amount, p.account_id, p.note, u.name AS user_name
         FROM payment p JOIN user u ON u.id = p.user_id
         WHERE p.document_id = ? AND p.reversed_at IS NULL ORDER BY p.date, p.id",
    )
    .bind(id)
    .fetch_all(db)
    .await
}

/// Records money received for an issued invoice.
#[utoipa::path(post, path = "/documents/{id}/payments", tag = "documents",
    params(("id" = i64, Path)), request_body = PaymentInput,
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn add_payment(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<PaymentInput>,
) -> Result<Json<Document>, ApiError> {
    let business = business(&app).await?;
    let mut check = Check::default();
    if input.date.parse::<jiff::civil::Date>().is_err() {
        check.fail("date", FieldError::InvalidFormat);
    }
    if input.amount <= 0 || input.amount > 100_000_000_000 {
        check.fail("amount", FieldError::InvalidFormat);
    }
    let note = check.text("note", input.note, 500);
    if input
        .eur_amount
        .is_some_and(|a| a <= 0 || a > 100_000_000_000)
    {
        check.fail("eur_amount", FieldError::InvalidFormat);
    }
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let invoice = row(&mut tx, id).await?;
    if invoice.kind != Kind::Invoice || invoice.status != Status::Issued {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    }
    let foreign = invoice.currency != crate::money::DEFAULT_CURRENCY;
    let books = books::books(&mut tx).await?;
    let account = match &books {
        Some(_) => Some(match input.account_id {
            Some(account) => {
                let open: bool = sqlx::query_scalar(
                    "SELECT count(*) > 0 FROM ledger_account
                     WHERE id = ? AND type = 'asset' AND NOT archived",
                )
                .bind(account)
                .fetch_one(&mut *tx)
                .await?;
                if !open {
                    check.fail("account_id", FieldError::InvalidFormat);
                }
                account
            }
            None => books::account(&mut tx, "bank")
                .await?
                .ok_or(ApiError::new(ErrorCode::NoLedgerAccount).with_detail("bank".to_owned()))?,
        }),
        None => None,
    };
    let booked = books.is_some_and(|b| input.date >= b.start_date);
    let eur_amount = match (foreign, input.eur_amount) {
        (false, _) => None,
        (true, None) if booked => {
            check.fail("eur_amount", FieldError::Required);
            None
        }
        (true, amount) => amount,
    };
    check.done()?;
    let payment: i64 = sqlx::query_scalar(
        "INSERT INTO payment (document_id, date, amount, eur_amount, account_id, note, user_id,
         created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(id)
    .bind(&input.date)
    .bind(input.amount)
    .bind(eur_amount)
    .bind(account)
    .bind(&note)
    .bind(auth.user.id)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "payment.create",
        &format!("document:{id}"),
        None,
        Some(serde_json::json!({
            "payment": payment, "date": input.date, "amount": input.amount,
            "eur_amount": eur_amount, "account_id": account, "note": note
        })),
    )
    .await?;
    rules::post_payment(&mut tx, &business, auth.user.id, payment)
        .await
        .map_err(rule_error)?;
    let document = find(&mut tx, &business, id).await?;
    tx.commit().await?;
    Ok(Json(document))
}

/// Reverses a payment recorded by mistake, with its postings.
#[utoipa::path(post, path = "/payments/{id}/reverse", tag = "documents",
    params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn reverse_payment(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let today = today(&business(&app).await?)?.to_string();
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before: Option<(i64, String, i64, Option<i64>)> =
        sqlx::query_as("SELECT document_id, date, amount, reversed_at FROM payment WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let (document, date, amount, reversed) = before.ok_or(ApiError::new(ErrorCode::NotFound))?;
    if reversed.is_some() {
        return Err(ApiError::new(ErrorCode::AlreadyReversed));
    }
    sqlx::query("UPDATE payment SET reversed_at = ?, reversed_by = ? WHERE id = ?")
        .bind(now())
        .bind(auth.user.id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    rules::reverse_all(
        &mut tx,
        auth.user.id,
        rules::Column::Payment,
        id,
        today.max(date.clone()).as_str(),
    )
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "payment.reverse",
        &format!("document:{document}"),
        Some(serde_json::json!({ "payment": id, "date": date, "amount": amount })),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
