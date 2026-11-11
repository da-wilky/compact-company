use std::sync::Arc;

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use utoipa::IntoParams;

use super::{
    books::started,
    documents::business,
    error::{ApiError, FieldError, Query},
    input::field_error,
};
use crate::{
    App,
    auth::{Auth, OwnerOrAccountant},
    datev::{self, Batch, Row},
};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Range {
    /// ISO date, inclusive.
    from: String,
    /// ISO date, inclusive, in the same year as `from`.
    to: String,
}

#[derive(sqlx::FromRow)]
struct Booked {
    id: i64,
    date: String,
    description: String,
    reference: Option<String>,
}

/// The journal of a range as a DATEV-Format Buchungsstapel.
#[utoipa::path(get, path = "/exports/datev", tag = "exports", params(Range),
    responses((status = OK, content_type = "text/csv", body = String),
        (status = "default", body = ApiError)))]
pub async fn datev_export(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(range): Query<Range>,
) -> Result<Response, ApiError> {
    let parse = |key: &str, value: &str| {
        value
            .parse::<jiff::civil::Date>()
            .map_err(|_| field_error(key, FieldError::InvalidFormat))
    };
    let (from, to) = (parse("from", &range.from)?, parse("to", &range.to)?);
    if from > to || from.year() != to.year() {
        return Err(field_error("to", FieldError::InvalidFormat));
    }
    let business = business(&app).await?;
    let mut db = app.db.acquire().await?;
    let chart = started(&mut db).await?.chart;
    let bytes = buchungsstapel(&mut db, &business, chart, &range.from, &range.to).await?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "text/csv; charset=windows-1252".to_owned(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"EXTF_Buchungsstapel_{}_{}.csv\"",
                    range.from, range.to
                ),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// The Buchungsstapel of every posting dated from `from` to `to`.
pub async fn buchungsstapel(
    db: &mut sqlx::SqliteConnection,
    business: &crate::settings::Business,
    chart: crate::books::Chart,
    from: &str,
    to: &str,
) -> Result<Vec<u8>, ApiError> {
    let postings: Vec<Booked> = sqlx::query_as(
        "SELECT p.id, p.date, p.description, coalesce(d.number, r.number) AS reference
         FROM posting p
         LEFT JOIN payment y ON y.id = p.payment_id
         LEFT JOIN document d ON d.id = coalesce(p.document_id, y.document_id)
         LEFT JOIN receipt_payment ry ON ry.id = p.receipt_payment_id
         LEFT JOIN receipt r ON r.id = coalesce(p.receipt_id, ry.receipt_id)
         WHERE p.date >= ? AND p.date <= ? ORDER BY p.date, p.id",
    )
    .bind(from)
    .bind(to)
    .fetch_all(&mut *db)
    .await?;
    let mut rows = Vec::new();
    for posting in postings {
        let lines: Vec<(String, i64)> = sqlx::query_as(
            "SELECT a.number, l.amount FROM posting_line l
             JOIN ledger_account a ON a.id = l.account_id WHERE l.posting_id = ? ORDER BY l.id",
        )
        .bind(posting.id)
        .fetch_all(&mut *db)
        .await?;
        for (account, contra, amount) in datev::pair(&lines) {
            rows.push(Row {
                amount,
                suspend_automatic: datev::automatic(chart, &account)
                    || datev::automatic(chart, &contra),
                account,
                contra,
                date: posting.date.clone(),
                reference: posting
                    .reference
                    .clone()
                    .unwrap_or_else(|| format!("B{}", posting.id)),
                text: posting.description.clone(),
            });
        }
    }
    let created = jiff::Timestamp::now()
        .to_zoned(
            jiff::tz::TimeZone::get(business.timezone.as_deref().unwrap_or("Europe/Berlin"))
                .map_err(ApiError::internal)?,
        )
        .strftime("%Y%m%d%H%M%S%3f")
        .to_string();
    Ok(datev::buchungsstapel(
        &Batch {
            consultant: business.datev_consultant_number.unwrap_or(1001),
            client: business.datev_client_number.unwrap_or(1),
            chart,
            from,
            to,
            created: &created,
        },
        &rows,
    ))
}
