use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::SqliteConnection;

use crate::{
    books::{self, Line, NewPosting, Source},
    documents::{Data, Kind},
    settings::{Business, Taxation},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no ledger account has the role {0}")]
    NoAccount(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

async fn role(db: &mut SqliteConnection, role: &str) -> Result<i64, Error> {
    books::account(db, role)
        .await?
        .ok_or_else(|| Error::NoAccount(role.to_owned()))
}

fn round(value: Decimal) -> i64 {
    value
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .try_into()
        .expect("amounts stay far below i64")
}

/// Major units to cents; every supported currency has two decimals.
fn minor(major: Decimal) -> i64 {
    round(major * Decimal::ONE_HUNDRED)
}

/// `part` of `whole` applied to `amount`, the whole amount when `whole` is zero.
fn share(amount: i64, part: i64, whole: i64) -> i64 {
    if whole == 0 {
        amount
    } else {
        round(Decimal::from(amount) * Decimal::from(part) / Decimal::from(whole))
    }
}

fn rate_key(rate: Decimal) -> String {
    rate.normalize().to_string()
}

/// Books an issued invoice or cancellation invoice at `rate` EUR per unit of its currency:
/// receivables against revenue per VAT rate and VAT owed, or VAT not yet due under the cash basis.
/// Nothing is booked before the books start or for a document dated before their first day.
pub async fn post_document(
    tx: &mut SqliteConnection,
    business: &Business,
    user_id: i64,
    id: i64,
    data: &Data,
    rate: Decimal,
) -> Result<Option<i64>, Error> {
    let Some(books) = books::books(tx).await? else {
        return Ok(None);
    };
    let date = data
        .issue_date
        .clone()
        .expect("an issued document has its date");
    if date < books.start_date {
        return Ok(None);
    }
    let vat_role = match business.taxation() {
        Taxation::Cash => "vat_pending",
        Taxation::Accrual | Taxation::SmallBusiness => "vat",
    };
    let mut lines = Vec::new();
    let mut receivable = 0;
    for group in &data.vat {
        let net = round(Decimal::from(minor(group.net)) * rate);
        let tax = round(Decimal::from(minor(group.tax)) * rate);
        let revenue = if data.business.small_business {
            "revenue_small_business".to_owned()
        } else {
            format!("revenue:{}", rate_key(group.rate))
        };
        lines.push(Line {
            account_id: role(tx, &revenue).await?,
            amount: -net,
        });
        if tax != 0 {
            lines.push(Line {
                account_id: role(tx, &format!("{vat_role}:{}", rate_key(group.rate))).await?,
                amount: -tax,
            });
        }
        receivable += net + tax;
    }
    lines.push(Line {
        account_id: role(tx, "receivables").await?,
        amount: receivable,
    });
    let kind = match data.kind {
        Kind::Cancellation => "Stornorechnung",
        _ => "Rechnung",
    };
    let posting = NewPosting {
        date,
        description: format!(
            "{kind} {} {}",
            data.number.as_deref().unwrap_or_default(),
            data.customer.name
        ),
        lines,
        source: Source {
            document_id: Some(id),
            payment_id: None,
        },
    };
    Ok(Some(books::book(tx, user_id, posting, None).await?))
}

/// The EUR per unit an invoice was booked at, if it was booked.
pub async fn booked_rate(
    db: &mut SqliteConnection,
    invoice_id: i64,
) -> Result<Option<Decimal>, Error> {
    let receivables = role(db, "receivables").await?;
    let booked: Option<i64> = sqlx::query_scalar(
        "SELECT sum(l.amount) FROM posting_line l JOIN posting p ON p.id = l.posting_id
         WHERE p.document_id = ? AND p.reverses_id IS NULL AND l.account_id = ?",
    )
    .bind(invoice_id)
    .bind(receivables)
    .fetch_one(&mut *db)
    .await?;
    let gross = gross(db, &[invoice_id]).await?;
    Ok(booked
        .filter(|_| gross != 0)
        .map(|eur| Decimal::from(eur) / Decimal::from(gross)))
}

/// The invoice and its issued cancellation invoices.
async fn family(db: &mut SqliteConnection, invoice_id: i64) -> Result<Vec<i64>, sqlx::Error> {
    let mut ids = vec![invoice_id];
    ids.extend(
        sqlx::query_scalar::<_, i64>(
            "SELECT id FROM document WHERE source_id = ? AND kind = 'cancellation'
             AND status = 'issued'",
        )
        .bind(invoice_id)
        .fetch_all(&mut *db)
        .await?,
    );
    Ok(ids)
}

fn ids_sql(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

/// Gross cents of the documents as issued, cancellations negative.
async fn gross(db: &mut SqliteConnection, ids: &[i64]) -> Result<i64, sqlx::Error> {
    let sql = format!(
        "SELECT data FROM document WHERE id IN ({}) AND status = 'issued'",
        ids_sql(ids)
    );
    let datas: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *db)
        .await?;
    Ok(datas
        .iter()
        .map(|d| {
            let data: Data = serde_json::from_str(d).expect("issued data parses");
            data.totals.map_or(0, |t| minor(t.gross))
        })
        .sum())
}

/// What the documents' postings booked on `account`, EUR cents.
async fn booked_on(
    db: &mut SqliteConnection,
    ids: &[i64],
    account: i64,
) -> Result<i64, sqlx::Error> {
    let sql = format!(
        "SELECT coalesce(sum(l.amount), 0) FROM posting_line l
         JOIN posting p ON p.id = l.posting_id
         WHERE p.document_id IN ({}) AND p.payment_id IS NULL AND l.account_id = ?",
        ids_sql(ids)
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(account)
        .fetch_one(db)
        .await
}

/// What the invoice's payments booked on `account`, EUR cents.
async fn paid_on(
    db: &mut SqliteConnection,
    invoice_id: i64,
    account: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT coalesce(sum(l.amount), 0) FROM posting_line l
         JOIN posting p ON p.id = l.posting_id JOIN payment y ON y.id = p.payment_id
         WHERE y.document_id = ? AND l.account_id = ?",
    )
    .bind(invoice_id)
    .bind(account)
    .fetch_one(db)
    .await
}

#[derive(sqlx::FromRow)]
struct Payment {
    document_id: i64,
    date: String,
    amount: i64,
    eur_amount: Option<i64>,
    account_id: Option<i64>,
    number: String,
    customer_name: String,
}

/// Books a payment of an invoice against the account it arrived on, clearing the invoice's
/// receivable in proportion to what is paid of it; the difference to the EUR that arrived is an
/// exchange gain or loss. Under the cash basis the paid share of the VAT becomes owed.
pub async fn post_payment(
    tx: &mut SqliteConnection,
    business: &Business,
    user_id: i64,
    payment_id: i64,
) -> Result<Option<i64>, Error> {
    let Some(books) = books::books(tx).await? else {
        return Ok(None);
    };
    let payment: Payment = sqlx::query_as(
        "SELECT y.document_id, y.date, y.amount, y.eur_amount, y.account_id, d.number,
         c.name AS customer_name FROM payment y JOIN document d ON d.id = y.document_id
         JOIN customer c ON c.id = d.customer_id WHERE y.id = ?",
    )
    .bind(payment_id)
    .fetch_one(&mut *tx)
    .await?;
    if payment.date < books.start_date {
        return Ok(None);
    }
    let account = payment
        .account_id
        .expect("payments carry their account once the books are started");
    let eur = payment.eur_amount.unwrap_or(payment.amount);
    let invoice = payment.document_id;
    let ids = family(tx, invoice).await?;
    let receivables = role(tx, "receivables").await?;
    let paid: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(amount), 0) FROM payment
         WHERE document_id = ? AND reversed_at IS NULL AND date >= ?",
    )
    .bind(invoice)
    .bind(&books.start_date)
    .fetch_one(&mut *tx)
    .await?;
    let gross = gross(tx, &ids).await?;
    let booked = booked_on(tx, &ids, receivables).await?;
    let invoice_booked: bool =
        sqlx::query_scalar("SELECT count(*) > 0 FROM posting WHERE document_id = ?")
            .bind(invoice)
            .fetch_one(&mut *tx)
            .await?;
    // an invoice from before the books started is cleared at what arrived, its receivable
    // being part of the opening balance
    let credit = if invoice_booked {
        share(booked, paid, gross) + paid_on(tx, invoice, receivables).await?
    } else {
        eur
    };
    let mut lines = vec![
        Line {
            account_id: account,
            amount: eur,
        },
        Line {
            account_id: receivables,
            amount: -credit,
        },
    ];
    let difference = eur - credit;
    if difference > 0 {
        lines.push(Line {
            account_id: role(tx, "fx_gains").await?,
            amount: -difference,
        });
    } else if difference < 0 {
        lines.push(Line {
            account_id: role(tx, "fx_losses").await?,
            amount: -difference,
        });
    }
    if business.taxation() == Taxation::Cash {
        let pending: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, role FROM ledger_account WHERE role LIKE 'vat_pending:%' ORDER BY role",
        )
        .fetch_all(&mut *tx)
        .await?;
        for (pending_id, pending_role) in pending {
            let due = if invoice_booked {
                let owed = -booked_on(tx, &ids, pending_id).await?;
                share(owed, paid, gross) - paid_on(tx, invoice, pending_id).await?
            } else {
                let rate: Decimal = pending_role["vat_pending:".len()..]
                    .parse()
                    .expect("roles carry their rate");
                let tax = vat_of(tx, &ids, rate).await?;
                round(
                    Decimal::from(share(tax, payment.amount, gross)) * Decimal::from(eur)
                        / Decimal::from(payment.amount),
                )
            };
            if due != 0 {
                let rate = &pending_role["vat_pending:".len()..];
                lines.push(Line {
                    account_id: pending_id,
                    amount: due,
                });
                lines.push(Line {
                    account_id: role(tx, &format!("vat:{rate}")).await?,
                    amount: -due,
                });
            }
        }
    }
    let posting = NewPosting {
        date: payment.date,
        description: format!("Zahlung {} {}", payment.number, payment.customer_name),
        lines,
        source: Source {
            document_id: None,
            payment_id: Some(payment_id),
        },
    };
    Ok(Some(books::book(tx, user_id, posting, None).await?))
}

/// The VAT cents at `rate` on the documents as issued.
async fn vat_of(db: &mut SqliteConnection, ids: &[i64], rate: Decimal) -> Result<i64, sqlx::Error> {
    let sql = format!("SELECT data FROM document WHERE id IN ({})", ids_sql(ids));
    let datas: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(db)
        .await?;
    Ok(datas
        .iter()
        .flat_map(|d| {
            let data: Data = serde_json::from_str(d).expect("issued data parses");
            data.vat
        })
        .filter(|g| g.rate == rate)
        .map(|g| minor(g.tax))
        .sum())
}

/// Reverses every posting of a payment on `date`.
pub async fn reverse_payment(
    tx: &mut SqliteConnection,
    user_id: i64,
    payment_id: i64,
    date: &str,
) -> Result<(), sqlx::Error> {
    let postings: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM posting p WHERE payment_id = ? AND reverses_id IS NULL
         AND NOT EXISTS (SELECT 1 FROM posting r WHERE r.reverses_id = p.id)",
    )
    .bind(payment_id)
    .fetch_all(&mut *tx)
    .await?;
    for id in postings {
        books::reverse(tx, user_id, id, date).await?;
    }
    Ok(())
}
