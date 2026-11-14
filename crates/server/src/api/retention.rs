use std::{collections::BTreeSet, fmt::Write as _, sync::Arc};

use axum::{body::Bytes, extract::State};
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use super::{
    documents::{business, today},
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    input::field_error,
    years::{PRUNED_SUMS, tar_gz, unpack},
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, now},
    documents::Kind,
    files::ContentHash,
    mirror,
};

/// How long a record must be kept (§ 147 Abs. 3 AO, § 14b Abs. 1 UStG; ADR 0025).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = RetentionClass)]
pub enum Class {
    /// Invoices issued and received, bank statements: 8 years.
    BookingDocument,
    /// Quotes, order confirmations, acceptance reports, dunning letters: 6 years.
    BusinessLetter,
    /// A delivery note whose invoice is issued: until then (§ 147 Abs. 3 Sätze 3–4 AO).
    DeliveryNote,
}

impl Class {
    fn of(kind: Kind) -> Self {
        match kind {
            Kind::Invoice | Kind::Cancellation => Self::BookingDocument,
            Kind::Quote | Kind::OrderConfirmation | Kind::AcceptanceReport | Kind::Dunning => {
                Self::BusinessLetter
            }
            Kind::DeliveryNote => Self::DeliveryNote,
        }
    }

    /// The first day the record of a year may go: 1 January after its period ends.
    fn deletable_from(self, year: i16) -> Date {
        let years = match self {
            Self::BookingDocument => 8,
            Self::BusinessLetter => 6,
            Self::DeliveryNote => 0,
        };
        jiff::civil::date(year + years + 1, 1, 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = RetentionKind)]
pub enum ItemKind {
    Quote,
    OrderConfirmation,
    Invoice,
    Cancellation,
    DeliveryNote,
    AcceptanceReport,
    Dunning,
    Receipt,
    BankStatement,
}

impl From<Kind> for ItemKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Quote => Self::Quote,
            Kind::OrderConfirmation => Self::OrderConfirmation,
            Kind::Invoice => Self::Invoice,
            Kind::Cancellation => Self::Cancellation,
            Kind::DeliveryNote => Self::DeliveryNote,
            Kind::AcceptanceReport => Self::AcceptanceReport,
            Kind::Dunning => Self::Dunning,
        }
    }
}

/// Why a record past its date is kept.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "reason", content = "by")]
#[schema(as = RetentionHold)]
pub enum Hold {
    /// It documents an asset still in the register, or disposed of fewer than 8 years ago.
    Asset(String),
    /// A partial invoice whose final invoice is still kept.
    FinalInvoice(String),
    /// A document of a later year that is still kept refers to it.
    LaterDocument(String),
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = RetentionItem)]
pub struct Item {
    /// Such as `document:12`, `receipt:3` or `bank_import:2`.
    pub(super) subject: String,
    kind: ItemKind,
    /// The document number, the receipt's supplier and number, or the bank account.
    label: String,
    date: String,
    class: Class,
    deletable_from: String,
    held: Option<Hold>,
    pub(super) deleted_at: Option<i64>,
    #[serde(skip)]
    pub(super) hashes: Vec<String>,
}

impl Item {
    fn deletable(&self, today: Date) -> bool {
        self.deleted_at.is_none()
            && self.held.is_none()
            && self
                .deletable_from
                .parse::<Date>()
                .is_ok_and(|d| d <= today)
    }
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
#[schema(as = RetentionDeletion)]
pub struct Deletion {
    at: i64,
    user: String,
    confirmation: String,
    files: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RetentionYear {
    year: i16,
    /// Whether files were deleted from its archive.
    pruned: bool,
    /// Records with a file, oldest first.
    items: Vec<Item>,
    /// Records whose file may be deleted today.
    deletable: usize,
    deletions: Vec<Deletion>,
    anonymisation: super::anonymise::Anonymisation,
}

#[derive(sqlx::FromRow)]
struct DocumentRow {
    id: i64,
    kind: Kind,
    number: String,
    issue_date: String,
    pdf: String,
    xml: Option<String>,
    source_id: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct ReceiptRow {
    id: i64,
    file: String,
    day: String,
    supplier: Option<String>,
    number: Option<String>,
}

pub(super) async fn deleted_at(
    db: &mut SqliteConnection,
    subject: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT max(r.at) FROM deleted_file d JOIN retention_deletion r ON r.id = d.deletion_id
         WHERE d.subject = ?",
    )
    .bind(subject)
    .fetch_one(db)
    .await
}

/// A retained document of a later year, or the final invoice, that keeps a document.
async fn document_hold(
    db: &mut SqliteConnection,
    id: i64,
    year: i16,
) -> Result<Option<Hold>, sqlx::Error> {
    let final_invoice: Option<String> = sqlx::query_scalar(
        "SELECT d.number FROM final_partial p JOIN document d ON d.id = p.final_id
         WHERE p.partial_id = ? AND NOT EXISTS (
           SELECT 1 FROM deleted_file f WHERE f.subject = 'document:' || d.id)",
    )
    .bind(id)
    .fetch_optional(&mut *db)
    .await?;
    if let Some(number) = final_invoice {
        return Ok(Some(Hold::FinalInvoice(number)));
    }
    let later: Option<String> = sqlx::query_scalar(
        "SELECT d.number FROM document d WHERE d.source_id = ? AND d.status = 'issued'
         AND d.issue_date > ? AND NOT EXISTS (
           SELECT 1 FROM deleted_file f WHERE f.subject = 'document:' || d.id)
         ORDER BY d.issue_date DESC LIMIT 1",
    )
    .bind(id)
    .bind(format!("{year}-12-31"))
    .fetch_optional(&mut *db)
    .await?;
    Ok(later.map(Hold::LaterDocument))
}

pub(super) async fn items(
    db: &mut SqliteConnection,
    year: i16,
    today: Date,
) -> Result<Vec<Item>, ApiError> {
    let (from, to) = (format!("{year}-01-01"), format!("{year}-12-31"));
    let mut items = Vec::new();

    let documents: Vec<DocumentRow> = sqlx::query_as(
        "SELECT id, kind, number, issue_date, pdf, xml, source_id FROM document
         WHERE status = 'issued' AND pdf IS NOT NULL AND issue_date >= ? AND issue_date <= ?
         ORDER BY issue_date, number",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&mut *db)
    .await?;
    for d in documents {
        let subject = format!("document:{}", d.id);
        let mut class = Class::of(d.kind);
        if class == Class::DeliveryNote {
            let invoiced: bool = sqlx::query_scalar(
                "SELECT count(*) > 0 FROM document WHERE kind = 'invoice' AND status = 'issued'
                 AND source_id = ?",
            )
            .bind(d.source_id)
            .fetch_one(&mut *db)
            .await?;
            if !invoiced {
                class = Class::BusinessLetter;
            }
        }
        items.push(Item {
            held: document_hold(db, d.id, year).await?,
            deleted_at: deleted_at(db, &subject).await?,
            subject,
            kind: d.kind.into(),
            label: d.number,
            date: d.issue_date,
            class,
            deletable_from: class.deletable_from(year).to_string(),
            hashes: [Some(d.pdf), d.xml].into_iter().flatten().collect(),
        });
    }

    let receipts: Vec<ReceiptRow> = sqlx::query_as(
        "SELECT r.id, r.file, coalesce(r.date, date(r.created_at, 'unixepoch')) AS day,
         s.name AS supplier,
         r.number FROM receipt r LEFT JOIN supplier s ON s.id = r.supplier_id
         WHERE r.file IS NOT NULL AND day >= ? AND day <= ? ORDER BY day, r.id",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&mut *db)
    .await?;
    for ReceiptRow {
        id,
        file,
        day: date,
        supplier,
        number,
    } in receipts
    {
        let subject = format!("receipt:{id}");
        let asset: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT a.name, a.disposed FROM asset a JOIN receipt_line l ON l.id = a.receipt_line_id
             WHERE l.receipt_id = ? ORDER BY a.disposed IS NOT NULL, a.disposed DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *db)
        .await?;
        let held = asset
            .filter(|(_, disposed)| {
                disposed
                    .as_deref()
                    .and_then(|d| d.parse::<Date>().ok())
                    .is_none_or(|d| Class::BookingDocument.deletable_from(d.year()) > today)
            })
            .map(|(name, _)| Hold::Asset(name));
        let label = [supplier, number]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        items.push(Item {
            deleted_at: deleted_at(db, &subject).await?,
            subject,
            kind: ItemKind::Receipt,
            label,
            date,
            class: Class::BookingDocument,
            deletable_from: Class::BookingDocument.deletable_from(year).to_string(),
            held,
            hashes: vec![file],
        });
    }

    let statements: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT i.id, i.file, a.name,
         coalesce(max(t.booking_date), date(i.created_at, 'unixepoch')) AS day
         FROM bank_import i JOIN bank_account a ON a.id = i.bank_account_id
         LEFT JOIN bank_transaction t ON t.import_id = i.id
         GROUP BY i.id HAVING day >= ? AND day <= ? ORDER BY day, i.id",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&mut *db)
    .await?;
    for (id, file, account, date) in statements {
        let subject = format!("bank_import:{id}");
        items.push(Item {
            deleted_at: deleted_at(db, &subject).await?,
            subject,
            kind: ItemKind::BankStatement,
            label: account,
            date,
            class: Class::BookingDocument,
            deletable_from: Class::BookingDocument.deletable_from(year).to_string(),
            held: None,
            hashes: vec![file],
        });
    }
    Ok(items)
}

pub(super) async fn retention_year(
    db: &mut SqliteConnection,
    year: i16,
    today: Date,
) -> Result<RetentionYear, ApiError> {
    let items = items(db, year, today).await?;
    let pruned: bool = sqlx::query_scalar("SELECT pruned FROM year_archive WHERE year = ?")
        .bind(year)
        .fetch_one(&mut *db)
        .await?;
    let deletions = sqlx::query_as(
        "SELECT r.at, u.name AS user, r.confirmation,
         (SELECT count(*) FROM deleted_file d WHERE d.deletion_id = r.id) AS files
         FROM retention_deletion r JOIN user u ON u.id = r.user_id WHERE r.year = ? ORDER BY r.id",
    )
    .bind(year)
    .fetch_all(&mut *db)
    .await?;
    Ok(RetentionYear {
        year,
        pruned,
        deletable: items.iter().filter(|i| i.deletable(today)).count(),
        items,
        deletions,
        anonymisation: super::anonymise::preview(db, year).await?,
    })
}

/// Every closed year with what may be deleted from when, and what is held back and why.
#[utoipa::path(get, path = "/retention", tag = "storage",
    responses((status = OK, body = Vec<RetentionYear>), (status = "default", body = ApiError)))]
pub async fn list_retention(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<Vec<RetentionYear>>, ApiError> {
    let today = today(&business(&app).await?)?;
    let mut db = app.db.acquire().await?;
    let years: Vec<i16> = sqlx::query_scalar("SELECT year FROM closed_year ORDER BY year DESC")
        .fetch_all(&mut *db)
        .await?;
    let mut list = Vec::new();
    for year in years {
        list.push(retention_year(&mut db, year, today).await?);
    }
    Ok(Json(list))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteInput {
    /// The owner confirms that no audit, objection or other open assessment concerns the year.
    pub(super) no_open_assessment: bool,
}

/// Deletes the files of a closed year whose retention period has passed; their records stay.
#[utoipa::path(post, path = "/retention/{year}/delete", tag = "storage",
    params(("year" = i16, Path)), request_body = DeleteInput,
    responses((status = OK, body = RetentionYear), (status = "default", body = ApiError)))]
pub async fn delete_expired(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(year): Path<i16>,
    Json(input): Json<DeleteInput>,
) -> Result<Json<RetentionYear>, ApiError> {
    if !input.no_open_assessment {
        return Err(field_error("no_open_assessment", FieldError::Required));
    }
    let today = today(&business(&app).await?)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let archive: Option<String> =
        sqlx::query_scalar("SELECT archive FROM year_archive WHERE year = ?")
            .bind(year)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let archive = archive.ok_or(ApiError::new(ErrorCode::YearAnonymised))?;
    let due: Vec<Item> = items(&mut tx, year, today)
        .await?
        .into_iter()
        .filter(|i| i.deletable(today))
        .collect();
    if due.is_empty() {
        return Err(ApiError::new(ErrorCode::NothingToDelete));
    }
    let confirmation = format!("No audit, objection or other open tax assessment concerns {year}.");
    let deletion: i64 = sqlx::query_scalar(
        "INSERT INTO retention_deletion (year, at, user_id, confirmation) VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(year)
    .bind(now())
    .bind(auth.user.id)
    .bind(&confirmation)
    .fetch_one(&mut *tx)
    .await?;
    for item in &due {
        for hash in &item.hashes {
            sqlx::query(
                "INSERT OR IGNORE INTO deleted_file (subject, hash, deletion_id) VALUES (?, ?, ?)",
            )
            .bind(&item.subject)
            .bind(hash)
            .bind(deletion)
            .execute(&mut *tx)
            .await?;
        }
    }
    let hashes: Vec<String> = due.iter().flat_map(|i| i.hashes.clone()).collect();
    let gone = unused(&mut tx, &hashes).await?;
    let pruned = prune(&app, &archive, &gone, today).await?;
    let size = i64::try_from(pruned.len()).map_err(ApiError::internal)?;
    let pruned = app
        .files
        .put(Bytes::from(pruned))
        .await
        .map_err(ApiError::internal)?;
    sqlx::query(
        "INSERT INTO pruned_archive (year, archive, size, deletion_id) VALUES (?, ?, ?, ?)",
    )
    .bind(year)
    .bind(pruned.to_string())
    .bind(size)
    .bind(deletion)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "retention.delete",
        &format!("year:{year}"),
        Some(serde_json::json!({ "archive": archive })),
        Some(serde_json::json!({
            "confirmation": confirmation,
            "archive": pruned.to_string(),
            "deleted": due.iter().map(|i| serde_json::json!({
                "subject": i.subject, "hashes": i.hashes
            })).collect::<Vec<_>>(),
        })),
    )
    .await?;
    for item in &due {
        mirror::queue(&app, &mut tx, &item.subject).await?;
    }
    mirror::queue(&app, &mut tx, &format!("year:{year}")).await?;
    let mut leftovers = gone;
    leftovers.extend(unused(&mut tx, std::slice::from_ref(&archive)).await?);
    tx.commit().await?;
    remove(&app, &leftovers).await;
    let mut db = app.db.acquire().await?;
    Ok(Json(retention_year(&mut db, year, today).await?))
}

/// Of these hashes, the ones no record needs any more.
pub(super) async fn unused(
    db: &mut SqliteConnection,
    hashes: &[String],
) -> Result<BTreeSet<String>, sqlx::Error> {
    let mut unused = BTreeSet::new();
    for hash in hashes {
        let used: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM stored_file WHERE hash = ?")
            .bind(hash)
            .fetch_one(&mut *db)
            .await?;
        if !used {
            unused.insert(hash.clone());
        }
    }
    Ok(unused)
}

/// Deletes files from the store once the records are committed; a file left behind only takes
/// space, so a failure is logged rather than undoing the deletion.
pub(super) async fn remove(app: &App, hashes: &BTreeSet<String>) {
    for hash in hashes {
        let Ok(hash) = hash.parse::<ContentHash>() else {
            continue;
        };
        if let Err(e) = app.files.delete(hash).await {
            eprintln!("cannot delete stored file {hash}: {e}");
        }
    }
}

/// The archive without the files of `gone`: it keeps the original `SHA256SUMS`, lists what was
/// deleted in `DELETED`, and checksums what it still holds in `SHA256SUMS.pruned`.
async fn prune(
    app: &App,
    archive: &str,
    gone: &BTreeSet<String>,
    today: Date,
) -> Result<Vec<u8>, ApiError> {
    let hash: ContentHash = archive.parse().map_err(ApiError::internal)?;
    let bytes = app.files.get(hash).await.map_err(ApiError::internal)?;
    let mut deleted = String::new();
    let mut files = Vec::new();
    for (path, bytes) in unpack(&bytes).map_err(ApiError::internal)? {
        let hash = ContentHash::of(&bytes).to_string();
        if path == PRUNED_SUMS {
            continue;
        }
        if path == DELETED {
            deleted.push_str(&String::from_utf8_lossy(&bytes));
        } else if gone.contains(&hash) {
            writeln!(deleted, "{hash}  {path}  {today}").expect("writing to a string");
        } else {
            files.push((path, bytes));
        }
    }
    files.push((DELETED.to_owned(), deleted.into_bytes()));
    tar_gz(files, now(), PRUNED_SUMS)
}

/// In a pruned archive: checksum, path and date of every file deleted from it.
const DELETED: &str = "DELETED";

/// Refuses a record whose file was deleted after its retention period.
pub async fn kept(db: &mut SqliteConnection, subject: &str) -> Result<(), ApiError> {
    match deleted_at(db, subject).await? {
        Some(_) => Err(ApiError::new(ErrorCode::FileDeleted)),
        None => Ok(()),
    }
}
