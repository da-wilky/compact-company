use std::{fmt::Write as _, io::Write as _, sync::Arc};

use axum::{
    body::Bytes,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use super::{
    books::started,
    documents::{business, rule_error, today},
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    exports::buchungsstapel,
    input::field_error,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    files::ContentHash,
    settings::{Business, Taxation},
    tax::{self, Basis},
};

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ClosedYear {
    year: i16,
    /// SHA-256 of the archive.
    archive: String,
    /// Bytes.
    size: i64,
    closed_at: i64,
    closed_by: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Years {
    /// Newest first.
    closed: Vec<ClosedYear>,
    /// The year that may be closed next: finished, booked, and every year before it closed.
    next: Option<i16>,
}

async fn closed(db: &mut SqliteConnection) -> Result<Vec<ClosedYear>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.year, c.archive, c.size, c.closed_at, u.name AS closed_by FROM closed_year c
         JOIN user u ON u.id = c.closed_by ORDER BY c.year DESC",
    )
    .fetch_all(db)
    .await
}

async fn next(db: &mut SqliteConnection, business: &Business) -> Result<Option<i16>, ApiError> {
    let Some(books) = crate::books::books(db).await? else {
        return Ok(None);
    };
    let last: Option<i16> = sqlx::query_scalar("SELECT max(year) FROM closed_year")
        .fetch_one(&mut *db)
        .await?;
    let first: i16 = books.start_date[..4].parse().map_err(ApiError::internal)?;
    let year = last.map_or(first, |y| y + 1);
    Ok((year < today(business)?.year()).then_some(year))
}

#[utoipa::path(get, path = "/years", tag = "books",
    responses((status = OK, body = Years), (status = "default", body = ApiError)))]
pub async fn list_years(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
) -> Result<Json<Years>, ApiError> {
    let business = business(&app).await?;
    let mut db = app.db.acquire().await?;
    Ok(Json(Years {
        closed: closed(&mut db).await?,
        next: next(&mut db, &business).await?,
    }))
}

/// Closes a finished year for good: no posting may be dated in it afterwards, and its archive is
/// kept with a checksum of every file.
#[utoipa::path(post, path = "/years/{year}/close", tag = "books", params(("year" = i16, Path)),
    responses((status = OK, body = ClosedYear), (status = "default", body = ApiError)))]
pub async fn close_year(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(year): Path<i16>,
) -> Result<Json<ClosedYear>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let books = started(&mut tx).await?;
    let done: bool = sqlx::query_scalar("SELECT count(*) > 0 FROM closed_year WHERE year = ?")
        .bind(year)
        .fetch_one(&mut *tx)
        .await?;
    if done {
        return Err(ApiError::new(ErrorCode::YearClosed));
    }
    if next(&mut tx, &business).await? != Some(year) {
        return Err(field_error("year", FieldError::InvalidFormat));
    }
    let closed_at = now();
    let files = archive_files(&app, &mut tx, &business, books.chart, year, closed_at).await?;
    let archive = tar_gz(files, closed_at)?;
    let size = i64::try_from(archive.len()).map_err(ApiError::internal)?;
    let hash = app
        .files
        .put(Bytes::from(archive))
        .await
        .map_err(ApiError::internal)?;
    sqlx::query(
        "INSERT INTO closed_year (year, archive, size, closed_at, closed_by) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(year)
    .bind(hash.to_string())
    .bind(size)
    .bind(closed_at)
    .bind(auth.user.id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "year.close",
        &format!("year:{year}"),
        None,
        Some(serde_json::json!({ "archive": hash.to_string(), "size": size })),
    )
    .await?;
    let closed = closed(&mut tx)
        .await?
        .into_iter()
        .find(|c| c.year == year)
        .expect("the year was just closed");
    tx.commit().await?;
    Ok(Json(closed))
}

#[utoipa::path(get, path = "/years/{year}/archive", tag = "books", params(("year" = i16, Path)),
    responses((status = OK, content_type = "application/gzip", body = Vec<u8>),
        (status = "default", body = ApiError)))]
pub async fn year_archive(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(year): Path<i16>,
) -> Result<Response, ApiError> {
    let hash: String = sqlx::query_scalar("SELECT archive FROM closed_year WHERE year = ?")
        .bind(year)
        .fetch_optional(&app.db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let hash: ContentHash = hash.parse().map_err(ApiError::internal)?;
    let bytes = app.files.get(hash).await.map_err(ApiError::internal)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/gzip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"compact-company-{year}.tar.gz\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

fn file_name(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn cents(amount: i64) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    format!("{sign}{},{:02}", amount.abs() / 100, amount.abs() % 100)
}

fn csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// Everything a year's books rest on, by path in the archive.
async fn archive_files(
    app: &App,
    db: &mut SqliteConnection,
    business: &Business,
    chart: crate::books::Chart,
    year: i16,
    closed_at: i64,
) -> Result<Vec<(String, Vec<u8>)>, ApiError> {
    let (from, to) = (format!("{year}-01-01"), format!("{year}-12-31"));
    let mut files = Vec::new();

    let lines: Vec<JournalLine> = sqlx::query_as(
        "SELECT p.date, p.id, p.description, p.reverses_id, a.number, a.name, l.amount
         FROM posting p JOIN posting_line l ON l.posting_id = p.id
         JOIN ledger_account a ON a.id = l.account_id
         WHERE p.date >= ? AND p.date <= ? ORDER BY p.date, p.id, l.id",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&mut *db)
    .await?;
    let mut journal =
        String::from("Datum;Buchung;Beschreibung;Storno von;Konto;Kontoname;Soll;Haben\n");
    for line in lines {
        let amount = line.amount;
        writeln!(
            journal,
            "{};{};{};{};{};{};{};{}",
            line.date,
            line.id,
            csv(&line.description),
            line.reverses_id.map(|r| r.to_string()).unwrap_or_default(),
            line.number,
            csv(&line.name),
            if amount > 0 {
                cents(amount)
            } else {
                String::new()
            },
            if amount < 0 {
                cents(-amount)
            } else {
                String::new()
            },
        )
        .expect("writing to a string");
    }
    files.push(("journal.csv".to_owned(), journal.into_bytes()));

    let accounts: Vec<(String, String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT a.number, a.name, a.type, a.euer_line,
         coalesce((SELECT sum(l.amount) FROM posting_line l JOIN posting p ON p.id = l.posting_id
                   WHERE l.account_id = a.id AND p.date <= ?), 0)
         FROM ledger_account a ORDER BY a.number",
    )
    .bind(&to)
    .fetch_all(&mut *db)
    .await?;
    let mut chart_csv =
        String::from("Konto;Name;Art;EÜR-Zeile;Saldo zum Jahresende (Soll positiv)\n");
    for (number, name, kind, line, balance) in accounts {
        writeln!(
            chart_csv,
            "{number};{};{kind};{};{}",
            csv(&name),
            line.unwrap_or_default(),
            cents(balance)
        )
        .expect("writing to a string");
    }
    files.push(("accounts.csv".to_owned(), chart_csv.into_bytes()));

    files.push((
        format!("datev/EXTF_Buchungsstapel_{year}.csv"),
        buchungsstapel(db, business, chart, &from, &to).await?,
    ));
    let basis = match business.taxation() {
        Taxation::Accrual => Some(Basis::Accrual),
        Taxation::Cash => Some(Basis::Cash),
        Taxation::SmallBusiness => None,
    };
    if let Some(basis) = basis {
        let figures = tax::vat_return(db, basis, &from, &to)
            .await
            .map_err(rule_error)?;
        files.push((
            "tax/vat-return.json".to_owned(),
            serde_json::to_vec_pretty(&figures).map_err(ApiError::internal)?,
        ));
    }
    let euer = tax::euer(db, business.small_business(), &from, &to)
        .await
        .map_err(rule_error)?;
    files.push((
        "tax/euer.json".to_owned(),
        serde_json::to_vec_pretty(&euer).map_err(ApiError::internal)?,
    ));

    let documents: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT number, pdf, xml FROM document WHERE status = 'issued' AND pdf IS NOT NULL
         AND issue_date >= ? AND issue_date <= ? ORDER BY number",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&mut *db)
    .await?;
    for (number, pdf, xml) in documents {
        let name = file_name(&number);
        files.push((format!("documents/{name}.pdf"), stored(app, &pdf).await?));
        if let Some(xml) = xml {
            files.push((format!("documents/{name}.xml"), stored(app, &xml).await?));
        }
    }
    let receipts: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, file, file_name FROM receipt WHERE status = 'booked' AND file IS NOT NULL
         AND date >= ? AND date <= ? ORDER BY id",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&mut *db)
    .await?;
    for (id, file, name) in receipts {
        let name = file_name(name.as_deref().unwrap_or("file"));
        files.push((format!("receipts/{id}-{name}"), stored(app, &file).await?));
    }
    let statements: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT i.id, i.file, i.file_name FROM bank_import i WHERE EXISTS (
           SELECT 1 FROM bank_transaction t WHERE t.import_id = i.id
           AND t.booking_date >= ? AND t.booking_date <= ?) ORDER BY i.id",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&mut *db)
    .await?;
    for (id, file, name) in statements {
        let name = file_name(name.as_deref().unwrap_or("statement"));
        files.push((format!("bank/{id}-{name}"), stored(app, &file).await?));
    }

    let tz = jiff::tz::TimeZone::get(business.timezone.as_deref().unwrap_or("Europe/Berlin"))
        .map_err(ApiError::internal)?;
    let second = |y: i16| -> Result<i64, ApiError> {
        Ok(jiff::civil::date(y, 1, 1)
            .to_zoned(tz.clone())
            .map_err(ApiError::internal)?
            .timestamp()
            .as_second())
    };
    let entries: Vec<String> = sqlx::query_scalar(
        "SELECT json_object('at', a.at, 'user', u.name, 'action', a.action,
           'subject', a.subject, 'before', json(a.before), 'after', json(a.after))
         FROM audit a JOIN user u ON u.id = a.user_id WHERE a.at >= ? AND a.at <= ? ORDER BY a.id",
    )
    .bind(second(year)?)
    .bind(closed_at)
    .fetch_all(&mut *db)
    .await?;
    files.push((
        "audit.json".to_owned(),
        format!("[{}]\n", entries.join(",\n")).into_bytes(),
    ));
    Ok(files)
}

#[derive(sqlx::FromRow)]
struct JournalLine {
    date: String,
    id: i64,
    description: String,
    reverses_id: Option<i64>,
    number: String,
    name: String,
    amount: i64,
}

async fn stored(app: &App, hash: &str) -> Result<Vec<u8>, ApiError> {
    let hash: ContentHash = hash.parse().map_err(ApiError::internal)?;
    Ok(app
        .files
        .get(hash)
        .await
        .map_err(ApiError::internal)?
        .to_vec())
}

/// A gzipped tar of the files with `SHA256SUMS`, in the format `sha256sum -c` checks.
fn tar_gz(files: Vec<(String, Vec<u8>)>, mtime: i64) -> Result<Vec<u8>, ApiError> {
    let mut sums = String::new();
    for (path, bytes) in &files {
        writeln!(sums, "{}  {path}", ContentHash::of(bytes)).expect("writing to a string");
    }
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    let mtime = u64::try_from(mtime).map_err(ApiError::internal)?;
    for (path, bytes) in files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .chain([("SHA256SUMS", sums.as_bytes())])
    {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(mtime);
        header.set_cksum();
        tar.append_data(&mut header, path, bytes)
            .map_err(ApiError::internal)?;
    }
    let mut gz = tar.into_inner().map_err(ApiError::internal)?;
    gz.flush().map_err(ApiError::internal)?;
    gz.finish().map_err(ApiError::internal)
}
