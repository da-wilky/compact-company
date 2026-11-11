use std::sync::Arc;

use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use super::{
    documents::{business, today},
    error::{ApiError, Json},
};
use crate::{
    App,
    auth::{Auth, OwnerOrAccountant},
    documents::{Data, amount_due},
    money::Money,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct OpenInvoiceItem {
    id: i64,
    number: String,
    customer_name: String,
    /// ISO dates.
    issue_date: String,
    due_date: Option<String>,
    /// Days past the due date; 0 when not overdue.
    overdue_days: i64,
    due: Money,
    /// The highest level of the dunning letters issued for it, if any.
    dunning_level: Option<i64>,
    /// ISO date of the last dunning letter.
    dunned_on: Option<String>,
    /// A dunning letter still being prepared.
    dunning_draft_id: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct OpenReceiptItem {
    id: i64,
    supplier_name: String,
    number: Option<String>,
    /// ISO dates.
    date: String,
    due_date: Option<String>,
    #[sqlx(skip)]
    overdue_days: i64,
    /// In the minor unit of its currency.
    due: i64,
    currency: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OpenItems {
    /// Issued invoices still owed, longest overdue first.
    invoices: Vec<OpenInvoiceItem>,
    /// Booked receipts still to pay, longest overdue first.
    receipts: Vec<OpenReceiptItem>,
}

fn overdue(today: jiff::civil::Date, due_date: Option<&str>) -> i64 {
    due_date
        .and_then(|d| d.parse::<jiff::civil::Date>().ok())
        .map_or(0, |due| i64::from((today - due).get_days()).max(0))
}

#[utoipa::path(get, path = "/open-items", tag = "books",
    responses((status = OK, body = OpenItems), (status = "default", body = ApiError)))]
pub async fn get_open_items(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
) -> Result<Json<OpenItems>, ApiError> {
    let today = today(&business(&app).await?)?;
    let mut db = app.db.acquire().await?;
    let rows: Vec<(i64, String, String, String, String, String)> = sqlx::query_as(
        "SELECT d.id, d.number, c.name, d.issue_date, d.currency, d.data FROM document d
         JOIN customer c ON c.id = d.customer_id
         WHERE d.kind = 'invoice' AND d.status = 'issued'",
    )
    .fetch_all(&mut *db)
    .await?;
    let mut invoices = Vec::new();
    for (id, number, customer_name, issue_date, currency, data) in rows {
        let due = amount_due(&mut db, id).await?;
        if due <= 0 {
            continue;
        }
        let data: Data = serde_json::from_str(&data).map_err(ApiError::internal)?;
        let (dunning_level, dunned_on): (Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT max(dunning_level), max(issue_date) FROM document
             WHERE source_id = ? AND kind = 'dunning' AND status = 'issued'",
        )
        .bind(id)
        .fetch_one(&mut *db)
        .await?;
        let dunning_draft_id = sqlx::query_scalar(
            "SELECT id FROM document WHERE source_id = ? AND kind = 'dunning'
             AND status = 'draft' ORDER BY id DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *db)
        .await?;
        invoices.push(OpenInvoiceItem {
            id,
            number,
            customer_name,
            issue_date,
            overdue_days: overdue(today, data.due_date.as_deref()),
            due_date: data.due_date,
            due: Money {
                minor: due,
                currency,
            },
            dunning_level,
            dunned_on,
            dunning_draft_id,
        });
    }
    invoices.sort_by(|a, b| {
        b.overdue_days
            .cmp(&a.overdue_days)
            .then_with(|| a.due_date.cmp(&b.due_date))
    });
    let mut receipts: Vec<OpenReceiptItem> = sqlx::query_as(
        "SELECT * FROM (SELECT r.id, s.name AS supplier_name, r.number, r.date, r.due_date,
         r.currency, (SELECT sum(net + tax) FROM receipt_line WHERE receipt_id = r.id)
         - coalesce((SELECT sum(amount) FROM receipt_payment
                     WHERE receipt_id = r.id AND reversed_at IS NULL), 0) AS due
         FROM receipt r JOIN supplier s ON s.id = r.supplier_id WHERE r.status = 'booked'
         AND NOT EXISTS (SELECT 1 FROM receipt_reversal v WHERE v.receipt_id = r.id))
         WHERE due <> 0",
    )
    .fetch_all(&mut *db)
    .await?;
    for receipt in &mut receipts {
        receipt.overdue_days = overdue(today, receipt.due_date.as_deref());
    }
    receipts.sort_by(|a, b| {
        b.overdue_days
            .cmp(&a.overdue_days)
            .then_with(|| a.due_date.cmp(&b.due_date))
    });
    Ok(Json(OpenItems { invoices, receipts }))
}
