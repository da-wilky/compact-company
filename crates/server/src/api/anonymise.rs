use std::sync::Arc;

use axum::extract::State;
use jiff::civil::Date;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use super::{
    customers::Language,
    documents::{business, today},
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    input::field_error,
    overview::amount_due,
    retention::{DeleteInput, RetentionYear, items, remove, retention_year, unused},
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, now},
    mirror,
};

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
#[schema(as = AnonymisedName)]
pub struct Named {
    id: i64,
    name: String,
}

/// Why a year can't be anonymised yet.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "reason", content = "by")]
#[schema(as = AnonymisationBlocker)]
pub enum Blocker {
    /// An earlier closed year that isn't anonymised; years go oldest first.
    EarlierYear(i16),
    /// An asset acquired by then that is still in the register.
    Asset(String),
    /// An invoice of the year or before that isn't paid in full.
    OpenInvoice(String),
    /// A receipt of the year or before that isn't paid in full.
    OpenReceipt(i64),
}

/// What anonymising a year does, for the owner to see first.
#[derive(Debug, Serialize, ToSchema)]
pub struct Anonymisation {
    /// 1 January after the ten years of § 147 Abs. 3 AO end.
    from: String,
    anonymised_at: Option<i64>,
    blockers: Vec<Blocker>,
    /// Customers, suppliers and users with nothing kept after the year, whose names go.
    customers: Vec<Named>,
    suppliers: Vec<Named>,
    users: Vec<Named>,
}

impl Anonymisation {
    pub fn due(&self, today: Date) -> bool {
        self.anonymised_at.is_none()
            && self.blockers.is_empty()
            && self.from.parse::<Date>().is_ok_and(|d| d <= today)
    }
}

pub(super) async fn preview(
    db: &mut SqliteConnection,
    year: i16,
) -> Result<Anonymisation, ApiError> {
    let end = format!("{year}-12-31");
    let anonymised_at: Option<i64> =
        sqlx::query_scalar("SELECT at FROM anonymised_year WHERE year = ?")
            .bind(year)
            .fetch_optional(&mut *db)
            .await?;
    let mut blockers = Vec::new();
    let earlier: Option<i16> = sqlx::query_scalar(
        "SELECT min(year) FROM closed_year WHERE year < ?
         AND year NOT IN (SELECT year FROM anonymised_year)",
    )
    .bind(year)
    .fetch_one(&mut *db)
    .await?;
    blockers.extend(earlier.map(Blocker::EarlierYear));
    let assets: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM asset WHERE acquired <= ?1 AND (disposed IS NULL OR disposed > ?1)
         ORDER BY acquired",
    )
    .bind(&end)
    .fetch_all(&mut *db)
    .await?;
    blockers.extend(assets.into_iter().map(Blocker::Asset));
    let invoices: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, number FROM document WHERE kind = 'invoice' AND status = 'issued'
         AND issue_date <= ? ORDER BY issue_date",
    )
    .bind(&end)
    .fetch_all(&mut *db)
    .await?;
    for (id, number) in invoices {
        if amount_due(db, id).await?.minor > 0 {
            blockers.push(Blocker::OpenInvoice(number));
        }
    }
    let receipts: Vec<i64> = sqlx::query_scalar(
        "SELECT r.id FROM receipt r WHERE r.status = 'booked' AND r.date <= ?
         AND NOT EXISTS (SELECT 1 FROM receipt_reversal v WHERE v.receipt_id = r.id)
         AND (SELECT sum(net + tax) FROM receipt_line WHERE receipt_id = r.id)
           <> coalesce((SELECT sum(amount) FROM receipt_payment
                        WHERE receipt_id = r.id AND reversed_at IS NULL), 0)
         ORDER BY r.date",
    )
    .bind(&end)
    .fetch_all(&mut *db)
    .await?;
    blockers.extend(receipts.into_iter().map(Blocker::OpenReceipt));
    // max() of SQLite is NULL as soon as one argument is, so every one is coalesced
    let customers = sqlx::query_as(
        "SELECT id, number || ' ' || name AS name FROM customer c WHERE NOT anonymised
         AND NOT EXISTS (SELECT 1 FROM recurring_invoice r WHERE r.customer_id = c.id)
         AND max(date(created_at, 'unixepoch'),
           coalesce((SELECT max(coalesce(d.issue_date, date(d.created_at, 'unixepoch')))
                     FROM document d WHERE d.customer_id = c.id), ''),
           coalesce((SELECT max(p.date) FROM payment p JOIN document d ON d.id = p.document_id
                     WHERE d.customer_id = c.id), ''),
           coalesce((SELECT max(date(e.started_at, 'unixepoch')) FROM time_entry e
                     JOIN project p ON p.id = e.project_id WHERE p.customer_id = c.id), '')
         ) <= ? ORDER BY number",
    )
    .bind(&end)
    .fetch_all(&mut *db)
    .await?;
    let suppliers = sqlx::query_as(
        "SELECT id, name FROM supplier s WHERE NOT anonymised
         AND max(date(created_at, 'unixepoch'),
           coalesce((SELECT max(coalesce(r.date, date(r.created_at, 'unixepoch')))
                     FROM receipt r WHERE r.supplier_id = s.id), ''),
           coalesce((SELECT max(p.date) FROM receipt_payment p JOIN receipt r
                     ON r.id = p.receipt_id WHERE r.supplier_id = s.id), '')
         ) <= ? ORDER BY name",
    )
    .bind(&end)
    .fetch_all(&mut *db)
    .await?;
    let users = sqlx::query_as(
        "SELECT id, name FROM user u WHERE NOT active AND NOT anonymised
         AND max(date(created_at, 'unixepoch'),
           coalesce((SELECT date(max(at), 'unixepoch') FROM audit WHERE user_id = u.id), ''),
           coalesce((SELECT date(max(started_at), 'unixepoch') FROM time_entry
                     WHERE user_id = u.id), '')
         ) <= ? ORDER BY name",
    )
    .bind(&end)
    .fetch_all(&mut *db)
    .await?;
    Ok(Anonymisation {
        from: jiff::civil::date(year + 11, 1, 1).to_string(),
        anonymised_at,
        blockers,
        customers,
        suppliers,
        users,
    })
}

/// Anonymises a closed year ten years on: its archive and remaining files, time entries,
/// comments and sent-mail logs go; postings keep their amounts and accounts but lose their texts;
/// customers, suppliers and former users with nothing kept after it lose their names.
#[utoipa::path(post, path = "/retention/{year}/anonymise", tag = "storage",
    params(("year" = i16, Path)), request_body = DeleteInput,
    responses((status = OK, body = RetentionYear), (status = "default", body = ApiError)))]
pub async fn anonymise_year(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(year): Path<i16>,
    Json(input): Json<DeleteInput>,
) -> Result<Json<RetentionYear>, ApiError> {
    if !input.no_open_assessment {
        return Err(field_error("no_open_assessment", FieldError::Required));
    }
    let business = business(&app).await?;
    let today = today(&business)?;
    let language = business.language.unwrap_or(Language::En);
    let label = language.pick("anonymisiert", "anonymised");
    let customer_name = format!("{} %s ({label})", language.pick("Kunde", "Customer"));
    let tz = jiff::tz::TimeZone::get(business.timezone.as_deref().unwrap_or("Europe/Berlin"))
        .map_err(ApiError::internal)?;
    let second = |y: i16| -> Result<i64, ApiError> {
        Ok(jiff::civil::date(y, 1, 1)
            .to_zoned(tz.clone())
            .map_err(ApiError::internal)?
            .timestamp()
            .as_second())
    };
    let (start, end) = (second(year)?, second(year + 1)?);
    let (from, to) = (format!("{year}-01-01"), format!("{year}-12-31"));

    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let archive: Option<String> =
        sqlx::query_scalar("SELECT archive FROM year_archive WHERE year = ?")
            .bind(year)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let archive = archive.ok_or(ApiError::new(ErrorCode::YearAnonymised))?;
    let plan = preview(&mut tx, year).await?;
    if !plan.due(today) {
        return Err(ApiError::new(ErrorCode::AnonymisationNotDue));
    }
    sqlx::query("INSERT INTO anonymising (year) VALUES (?)")
        .bind(year)
        .execute(&mut *tx)
        .await?;

    let confirmation = format!("No audit, objection or other open tax assessment concerns {year}.");
    let at = now();
    let deletion: i64 = sqlx::query_scalar(
        "INSERT INTO retention_deletion (year, at, user_id, confirmation) VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(year)
    .bind(at)
    .bind(auth.user.id)
    .bind(&confirmation)
    .fetch_one(&mut *tx)
    .await?;
    let records = items(&mut tx, year, today).await?;
    for item in &records {
        sqlx::query("UPDATE audit SET before = NULL, after = NULL WHERE subject = ?")
            .bind(&item.subject)
            .execute(&mut *tx)
            .await?;
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
    sqlx::query(
        "INSERT INTO anonymised_year (year, at, user_id, confirmation) VALUES (?, ?, ?, ?)",
    )
    .bind(year)
    .bind(at)
    .bind(auth.user.id)
    .bind(&confirmation)
    .execute(&mut *tx)
    .await?;

    for sql in [
        "UPDATE audit SET before = NULL, after = NULL WHERE subject IN
         (SELECT 'time:' || id FROM time_entry WHERE started_at >= ?1 AND started_at < ?2)",
        "DELETE FROM line_time_entry WHERE time_entry_id IN
         (SELECT id FROM time_entry WHERE started_at >= ?1 AND started_at < ?2)",
        "DELETE FROM time_entry WHERE started_at >= ?1 AND started_at < ?2",
        "DELETE FROM todo_comment WHERE created_at >= ?1 AND created_at < ?2",
        "DELETE FROM document_send WHERE at >= ?1 AND at < ?2",
        "UPDATE audit SET before = NULL, after = NULL WHERE at >= ?1 AND at < ?2",
    ] {
        sqlx::query(sql)
            .bind(start)
            .bind(end)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE invitation SET name = ?3 WHERE created_at >= ?1 AND created_at < ?2")
        .bind(start)
        .bind(end)
        .bind(label)
        .execute(&mut *tx)
        .await?;
    for sql in [
        "UPDATE posting SET description = ?3 WHERE date >= ?1 AND date <= ?2",
        "UPDATE receipt_line SET description = ?3 WHERE receipt_id IN (SELECT id
         FROM receipt WHERE coalesce(date, date(created_at, 'unixepoch')) BETWEEN ?1 AND ?2)",
        "UPDATE document_line SET description = ?3 WHERE document_id IN (SELECT id
         FROM document WHERE status = 'issued' AND issue_date BETWEEN ?1 AND ?2)",
    ] {
        sqlx::query(sql)
            .bind(&from)
            .bind(&to)
            .bind(label)
            .execute(&mut *tx)
            .await?;
    }
    for sql in [
        "UPDATE audit SET before = NULL, after = NULL WHERE subject IN
         (SELECT 'posting:' || id FROM posting WHERE date >= ?1 AND date <= ?2
          UNION SELECT 'bank_transaction:' || id FROM bank_transaction
          WHERE booking_date >= ?1 AND booking_date <= ?2)",
        "UPDATE payment SET note = NULL WHERE date >= ?1 AND date <= ?2",
        "UPDATE receipt_payment SET note = NULL WHERE date >= ?1 AND date <= ?2",
        "UPDATE bank_transaction SET counterparty = NULL, counterparty_iban = NULL, purpose = NULL,
         reference = NULL WHERE booking_date >= ?1 AND booking_date <= ?2",
        "UPDATE bank_import SET file_name = NULL WHERE id IN (SELECT import_id FROM bank_transaction
         WHERE booking_date >= ?1 AND booking_date <= ?2)",
        "UPDATE receipt SET note = NULL, file_name = NULL
         WHERE coalesce(date, date(created_at, 'unixepoch')) BETWEEN ?1 AND ?2",
    ] {
        sqlx::query(sql)
            .bind(&from)
            .bind(&to)
            .execute(&mut *tx)
            .await?;
    }
    let documents: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, data FROM document WHERE status = 'issued' AND issue_date BETWEEN ? AND ?",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&mut *tx)
    .await?;
    for (id, data) in documents {
        let mut data: Value = serde_json::from_str(&data).map_err(ApiError::internal)?;
        scrub(&mut data, language);
        sqlx::query(
            "UPDATE document SET title = NULL, intro = NULL, outro = NULL, data = ? WHERE id = ?",
        )
        .bind(data.to_string())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }

    for customer in &plan.customers {
        sqlx::query(
            "UPDATE customer SET name = printf(?, number), street = NULL,
             postcode = NULL, city = NULL, vat_id = NULL, leitweg_id = NULL,
             email = NULL, note = NULL, anonymised = 1 WHERE id = ?",
        )
        .bind(&customer_name)
        .bind(customer.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE project SET name = printf(?, id) WHERE customer_id = ?")
            .bind(format!(
                "{} %d ({label})",
                language.pick("Projekt", "Project")
            ))
            .bind(customer.id)
            .execute(&mut *tx)
            .await?;
        for sql in [
            "UPDATE audit SET before = NULL, after = NULL WHERE subject IN (SELECT 'todo:' || t.id
             FROM todo t JOIN project p ON p.id = t.project_id WHERE p.customer_id = ?1)",
            "DELETE FROM todo_comment WHERE todo_id IN (SELECT t.id FROM todo t
             JOIN project p ON p.id = t.project_id WHERE p.customer_id = ?1)",
            "DELETE FROM todo WHERE project_id IN (SELECT id FROM project WHERE customer_id = ?1)",
            "UPDATE audit SET before = NULL, after = NULL WHERE subject = 'customer:' || ?1
             OR subject IN (SELECT 'project:' || id FROM project WHERE customer_id = ?1)",
        ] {
            sqlx::query(sql).bind(customer.id).execute(&mut *tx).await?;
        }
    }
    for supplier in &plan.suppliers {
        sqlx::query(
            "UPDATE supplier SET name = printf(?, id), street = NULL,
             postcode = NULL, city = NULL, vat_id = NULL, iban = NULL, email = NULL,
             anonymised = 1 WHERE id = ?",
        )
        .bind(format!(
            "{} %d ({label})",
            language.pick("Lieferant", "Supplier")
        ))
        .bind(supplier.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE audit SET before = NULL, after = NULL WHERE subject = 'supplier:' || ?1",
        )
        .bind(supplier.id)
        .execute(&mut *tx)
        .await?;
    }
    for user in &plan.users {
        sqlx::query("UPDATE user SET name = printf(?, id), anonymised = 1 WHERE id = ?")
            .bind(format!("{} %d ({label})", language.pick("Nutzer", "User")))
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE invitation SET name = ? WHERE accepted_by = ?")
            .bind(label)
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
        for sql in [
            "DELETE FROM passkey WHERE user_id = ?1",
            "DELETE FROM session WHERE user_id = ?1",
            "DELETE FROM oidc_identity WHERE user_id = ?1",
            "DELETE FROM api_token WHERE user_id = ?1",
            "UPDATE audit SET before = NULL, after = NULL WHERE subject = 'user:' || ?1",
        ] {
            sqlx::query(sql).bind(user.id).execute(&mut *tx).await?;
        }
    }

    // a version of the process documentation is kept while a year it applied to is
    let superseded: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT v.id, v.data, v.pdf_de, v.pdf_en FROM process_version v
         WHERE (SELECT min(n.valid_from) FROM process_version n WHERE n.id > v.id) <= ?",
    )
    .bind(format!("{}-01-01", year + 1))
    .fetch_all(&mut *tx)
    .await?;
    let mut process_hashes = Vec::new();
    for (id, data, de, en) in superseded {
        let subject = format!("process:{id}");
        for hash in [data, de, en] {
            sqlx::query(
                "INSERT OR IGNORE INTO deleted_file (subject, hash, deletion_id) VALUES (?, ?, ?)",
            )
            .bind(&subject)
            .bind(&hash)
            .bind(deletion)
            .execute(&mut *tx)
            .await?;
            process_hashes.push(hash);
        }
        mirror::queue(&app, &mut tx, &subject).await?;
    }
    for item in &records {
        mirror::queue(&app, &mut tx, &item.subject).await?;
    }
    mirror::queue(&app, &mut tx, &format!("year:{year}")).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "year.anonymise",
        &format!("year:{year}"),
        None,
        Some(serde_json::json!({
            "confirmation": confirmation,
            "files": records.iter().filter(|i| i.deleted_at.is_none()).count(),
            "customers": plan.customers.iter().map(|c| c.id).collect::<Vec<_>>(),
            "suppliers": plan.suppliers.iter().map(|s| s.id).collect::<Vec<_>>(),
            "users": plan.users.iter().map(|u| u.id).collect::<Vec<_>>(),
        })),
    )
    .await?;
    sqlx::query("DELETE FROM anonymising")
        .execute(&mut *tx)
        .await?;
    let hashes: Vec<String> = records
        .iter()
        .flat_map(|i| i.hashes.clone())
        .chain([archive])
        .chain(process_hashes)
        .collect();
    let gone = unused(&mut tx, &hashes).await?;
    tx.commit().await?;
    remove(&app, &gone).await;
    // secure_delete zeroed the old rows in the WAL; this moves them into the database file
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&app.db)
        .await?;
    let mut db = app.db.acquire().await?;
    Ok(Json(retention_year(&mut db, year, today).await?))
}

/// Replaces what may name a person in a document's layout data; numbers, dates and amounts stay.
fn scrub(data: &mut Value, language: Language) {
    let label = language.pick("anonymisiert", "anonymised");
    for key in ["title", "intro", "outro", "timesheet"] {
        if let Some(value) = data.get_mut(key) {
            *value = Value::Null;
        }
    }
    if let Some(customer) = data.get_mut("customer").and_then(Value::as_object_mut) {
        let number = customer.get("number").cloned().unwrap_or_default();
        customer.insert(
            "name".into(),
            Value::from(format!(
                "{} {} ({label})",
                language.pick("Kunde", "Customer"),
                number.as_str().unwrap_or_default()
            )),
        );
        for key in [
            "street",
            "postcode",
            "city",
            "email",
            "vat_id",
            "leitweg_id",
        ] {
            customer.insert(key.into(), Value::Null);
        }
    }
    if let Some(lines) = data.get_mut("lines").and_then(Value::as_array_mut) {
        for line in lines {
            line["description"] = Value::from(label);
        }
    }
}
