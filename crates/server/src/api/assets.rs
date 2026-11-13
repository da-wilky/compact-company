use std::{collections::BTreeSet, sync::Arc};

use axum::extract::State;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    books::{check_date, started},
    documents::{business, rule_error, today},
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::{Check, field_error},
};
use crate::{
    App,
    assets::{self, Asset, AssetKind, LOW_VALUE_LIMIT, Method, Register},
    audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    books,
    rules::round,
    settings::Business,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ScheduleYear {
    year: i16,
    /// EUR cents.
    depreciation: i64,
    /// EUR cents written off as the asset left the business.
    disposal: i64,
    /// EUR cents at the end of the year.
    book_value: i64,
    booked: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AssetView {
    #[serde(flatten)]
    asset: Asset,
    receipt_id: Option<i64>,
    schedule: Vec<ScheduleYear>,
    /// Once anything is booked only the name changes.
    booked: bool,
}

/// A booked receipt's line on a fixed-asset account that no asset comes from yet.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetCandidate {
    receipt_line_id: i64,
    receipt_id: i64,
    supplier_name: String,
    /// ISO date.
    date: String,
    description: String,
    account_id: i64,
    /// EUR cents, as an asset's cost and net.
    cost: i64,
    net: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Assets {
    assets: Vec<AssetView>,
    candidates: Vec<AssetCandidate>,
    /// Finished years with depreciation still to book.
    due: Vec<i16>,
}

async fn view(db: &mut SqliteConnection, asset: Asset) -> Result<AssetView, ApiError> {
    let booked = assets::booked_years(db, asset.id).await?;
    let receipt_id: Option<i64> =
        sqlx::query_scalar("SELECT receipt_id FROM receipt_line WHERE id = ?")
            .bind(asset.receipt_line_id)
            .fetch_optional(&mut *db)
            .await?;
    let schedule = asset
        .years()
        .into_iter()
        .map(|year| ScheduleYear {
            year,
            depreciation: asset.depreciation(year),
            disposal: asset.disposal(year),
            book_value: asset.book_value(year),
            booked: booked.contains(&year),
        })
        .collect();
    Ok(AssetView {
        receipt_id,
        schedule,
        booked: !booked.is_empty(),
        asset,
    })
}

async fn find(db: &mut SqliteConnection, id: i64) -> Result<Asset, ApiError> {
    assets::find(db, id)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}

#[derive(sqlx::FromRow)]
struct CandidateRow {
    receipt_line_id: i64,
    receipt_id: i64,
    supplier_name: String,
    date: String,
    description: String,
    account_id: i64,
    net: i64,
    tax: i64,
    exchange_rate: Option<String>,
}

/// Receipt lines an asset may come from: `line` alone if given, and never one taken by another
/// asset than `except`.
async fn candidates(
    db: &mut SqliteConnection,
    business: &Business,
    line: Option<i64>,
    except: Option<i64>,
) -> Result<Vec<AssetCandidate>, ApiError> {
    let rows: Vec<CandidateRow> = sqlx::query_as(
        "SELECT l.id AS receipt_line_id, r.id AS receipt_id, s.name AS supplier_name, r.date,
         l.description, l.account_id, l.net, l.tax, r.exchange_rate
         FROM receipt_line l JOIN receipt r ON r.id = l.receipt_id
         JOIN supplier s ON s.id = r.supplier_id JOIN ledger_account a ON a.id = l.account_id
         WHERE r.status = 'booked'
           AND NOT EXISTS (SELECT 1 FROM receipt_reversal v WHERE v.receipt_id = r.id)
           AND a.type = 'asset' AND a.role IS NULL AND a.number LIKE '0%'
           AND NOT EXISTS (SELECT 1 FROM asset x WHERE x.receipt_line_id = l.id
                           AND x.id IS NOT ?2)
           AND (?1 IS NULL OR l.id = ?1)
         ORDER BY r.date DESC, l.id",
    )
    .bind(line)
    .bind(except)
    .fetch_all(&mut *db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            // converted as the receipt was booked
            let rate: Decimal = row.exchange_rate.as_deref().map_or(Decimal::ONE, |r| {
                r.parse().expect("rates are stored as decimals")
            });
            let net = round(Decimal::from(row.net) * rate);
            let tax = round(Decimal::from(row.tax) * rate);
            AssetCandidate {
                receipt_line_id: row.receipt_line_id,
                receipt_id: row.receipt_id,
                supplier_name: row.supplier_name,
                date: row.date,
                description: row.description,
                account_id: row.account_id,
                cost: if business.small_business() {
                    net + tax
                } else {
                    net
                },
                net,
            }
        })
        .collect())
}

async fn due_years(
    db: &mut SqliteConnection,
    business: &Business,
    all: &[Asset],
) -> Result<Vec<i16>, ApiError> {
    let Some(from) = assets::first_open_year(db).await? else {
        return Ok(Vec::new());
    };
    let through = today(business)?.year() - 1;
    let mut years = BTreeSet::new();
    for asset in all {
        years.extend(assets::due(db, asset, from, through).await?);
    }
    Ok(years.into_iter().collect())
}

async fn listing(db: &mut SqliteConnection, business: &Business) -> Result<Assets, ApiError> {
    let all = assets::all(db).await?;
    let due = due_years(db, business, &all).await?;
    let mut views = Vec::new();
    for asset in all {
        views.push(view(db, asset).await?);
    }
    Ok(Assets {
        assets: views,
        candidates: candidates(db, business, None, None).await?,
        due,
    })
}

/// The fixed assets with their depreciation schedules.
#[utoipa::path(get, path = "/assets", tag = "books",
    responses((status = OK, body = Assets), (status = "default", body = ApiError)))]
pub async fn list_assets(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
) -> Result<Json<Assets>, ApiError> {
    let business = business(&app).await?;
    let mut db = app.db.acquire().await?;
    started(&mut db).await?;
    Ok(Json(listing(&mut db, &business).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetInput {
    name: Option<String>,
    kind: AssetKind,
    method: Method,
    /// Whole years, for straight-line depreciation.
    life_years: Option<i16>,
    /// A booked receipt's line on a fixed-asset account; its account and amount are taken.
    receipt_line_id: Option<i64>,
    /// The fixed-asset account, without a receipt line.
    account_id: Option<i64>,
    /// ISO date; the receipt's date when unset.
    acquired: Option<String>,
    /// EUR cents, without a receipt line; the low-value limit is tested on it too.
    cost: Option<i64>,
}

/// An asset's fields as the input sets them.
#[derive(PartialEq)]
struct Fields {
    name: String,
    kind: AssetKind,
    method: Method,
    life_years: Option<i16>,
    receipt_line_id: Option<i64>,
    account_id: i64,
    acquired: String,
    cost: i64,
    net: i64,
}

async fn fields(
    db: &mut SqliteConnection,
    business: &Business,
    input: AssetInput,
    except: Option<i64>,
) -> Result<Fields, ApiError> {
    let mut check = Check::default();
    let name = check.required("name", input.name, 200);
    let (account_id, date, cost, net) = match input.receipt_line_id {
        Some(line) => match candidates(db, business, Some(line), except).await?.pop() {
            Some(c) => (c.account_id, Some(c.date), c.cost, c.net),
            None => {
                check.fail("receipt_line_id", FieldError::InvalidFormat);
                (0, None, 0, 0)
            }
        },
        None => {
            let fixed: bool = sqlx::query_scalar(
                "SELECT count(*) > 0 FROM ledger_account WHERE id = ? AND type = 'asset'
                 AND role IS NULL AND number LIKE '0%' AND NOT archived",
            )
            .bind(input.account_id)
            .fetch_one(&mut *db)
            .await?;
            if !fixed {
                check.fail("account_id", FieldError::Required);
            }
            let cost = input.cost.unwrap_or(0);
            if !(1..=100_000_000_000).contains(&cost) {
                check.fail("cost", FieldError::InvalidFormat);
            }
            (input.account_id.unwrap_or(0), None, cost, cost)
        }
    };
    // stored as plain ISO, which compares correctly as text
    let acquired = match input.acquired.or(date).unwrap_or_default().parse() {
        Ok::<jiff::civil::Date, _>(d) => d.to_string(),
        Err(_) => {
            check.fail("acquired", FieldError::InvalidFormat);
            String::new()
        }
    };
    let life_years = match input.method {
        Method::StraightLine => {
            if !input.life_years.is_some_and(|y| (1..=50).contains(&y)) {
                check.fail("life_years", FieldError::InvalidFormat);
            }
            input.life_years
        }
        Method::LowValue => {
            if net > LOW_VALUE_LIMIT {
                check.fail("method", FieldError::InvalidFormat);
            }
            None
        }
    };
    check.done()?;
    Ok(Fields {
        name,
        kind: input.kind,
        method: input.method,
        life_years,
        receipt_line_id: input.receipt_line_id,
        account_id,
        acquired,
        cost,
        net,
    })
}

/// Registers a fixed asset, from a receipt line or by hand.
#[utoipa::path(post, path = "/assets", tag = "books", request_body = AssetInput,
    responses((status = OK, body = AssetView), (status = "default", body = ApiError)))]
pub async fn create_asset(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<AssetInput>,
) -> Result<Json<AssetView>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    started(&mut tx).await?;
    let f = fields(&mut tx, &business, input, None).await?;
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO asset (name, kind, account_id, receipt_line_id, acquired, cost, net, method,
         life_years, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&f.name)
    .bind(f.kind)
    .bind(f.account_id)
    .bind(f.receipt_line_id)
    .bind(&f.acquired)
    .bind(f.cost)
    .bind(f.net)
    .bind(f.method)
    .bind(f.life_years)
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    let asset = find(&mut tx, id).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "asset.create",
        &format!("asset:{id}"),
        None,
        Some(serde_json::to_value(&asset).map_err(ApiError::internal)?),
    )
    .await?;
    let view = view(&mut tx, asset).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Changes an asset; once anything of it is booked, only its name.
#[utoipa::path(put, path = "/assets/{id}", tag = "books", request_body = AssetInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = AssetView), (status = "default", body = ApiError)))]
pub async fn put_asset(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<AssetInput>,
) -> Result<Json<AssetView>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    started(&mut tx).await?;
    let before = find(&mut tx, id).await?;
    let f = fields(&mut tx, &business, input, Some(id)).await?;
    if postings(&mut tx, id).await? > 0 {
        let kept = Fields {
            name: f.name.clone(),
            kind: before.kind,
            method: before.method,
            life_years: before.life_years,
            receipt_line_id: before.receipt_line_id,
            account_id: before.account_id,
            acquired: before.acquired.clone(),
            cost: before.cost,
            net: before.net,
        };
        if f != kept {
            return Err(ApiError::new(ErrorCode::AssetBooked));
        }
    }
    sqlx::query(
        "UPDATE asset SET name = ?, kind = ?, account_id = ?, receipt_line_id = ?, acquired = ?,
         cost = ?, net = ?, method = ?, life_years = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&f.name)
    .bind(f.kind)
    .bind(f.account_id)
    .bind(f.receipt_line_id)
    .bind(&f.acquired)
    .bind(f.cost)
    .bind(f.net)
    .bind(f.method)
    .bind(f.life_years)
    .bind(now())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let after = find(&mut tx, id).await?;
    if let Some((old, new)) = audit::changes(&before, &after) {
        audit::record(
            &mut tx,
            auth.user.id,
            "asset.update",
            &format!("asset:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    let view = view(&mut tx, after).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn postings(db: &mut SqliteConnection, id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM posting WHERE asset_id = ?")
        .bind(id)
        .fetch_one(db)
        .await
}

/// Deletes an asset registered by mistake, while nothing of it is booked.
#[utoipa::path(delete, path = "/assets/{id}", tag = "books", params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn delete_asset(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let asset = find(&mut tx, id).await?;
    if postings(&mut tx, id).await? > 0 {
        return Err(ApiError::new(ErrorCode::AssetBooked));
    }
    sqlx::query("DELETE FROM asset WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "asset.delete",
        &format!("asset:{id}"),
        Some(serde_json::to_value(&asset).map_err(ApiError::internal)?),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DepreciationInput {
    year: i16,
}

/// Books the depreciation of every asset through a finished year, each year on 31.12.
#[utoipa::path(post, path = "/assets/depreciation", tag = "books", request_body = DepreciationInput,
    responses((status = OK, body = Assets), (status = "default", body = ApiError)))]
pub async fn book_depreciation(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<DepreciationInput>,
) -> Result<Json<Assets>, ApiError> {
    let business = business(&app).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    started(&mut tx).await?;
    let from = assets::first_open_year(&mut tx)
        .await?
        .expect("the books are started");
    if input.year < from || input.year >= today(&business)?.year() {
        return Err(field_error("year", FieldError::InvalidFormat));
    }
    for asset in assets::all(&mut tx).await? {
        for year in assets::due(&mut tx, &asset, from, input.year).await? {
            assets::book(&mut tx, auth.user.id, &asset, year)
                .await
                .map_err(rule_error)?;
        }
    }
    let listing = listing(&mut tx, &business).await?;
    tx.commit().await?;
    Ok(Json(listing))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DisposalInput {
    /// ISO date it left the business.
    date: String,
}

/// Records that an asset left the business: its depreciation through that month and the rest of
/// its book value are booked on that day, replacing anything booked for that year or later.
#[utoipa::path(post, path = "/assets/{id}/dispose", tag = "books", request_body = DisposalInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = AssetView), (status = "default", body = ApiError)))]
pub async fn dispose_asset(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<DisposalInput>,
) -> Result<Json<AssetView>, ApiError> {
    let business = business(&app).await?;
    let today = today(&business)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let books = started(&mut tx).await?;
    let asset = find(&mut tx, id).await?;
    let mut check = Check::default();
    check_date(&mut check, "date", &input.date, &books);
    check.done()?;
    let date: jiff::civil::Date = input.date.parse().map_err(ApiError::internal)?;
    let iso = date.to_string();
    let from = assets::first_open_year(&mut tx)
        .await?
        .expect("the books are started");
    if asset.disposed.is_some() {
        return Err(field_error("date", FieldError::Locked));
    }
    if iso < asset.acquired || date > today || date.year() < from {
        return Err(field_error("date", FieldError::InvalidFormat));
    }
    let replaced: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, date FROM posting p WHERE asset_id = ? AND date >= ? AND reverses_id IS NULL
         AND NOT EXISTS (SELECT 1 FROM posting r WHERE r.reverses_id = p.id)",
    )
    .bind(id)
    .bind(format!("{}-01-01", date.year()))
    .fetch_all(&mut *tx)
    .await?;
    for (posting, on) in replaced {
        books::reverse(&mut tx, auth.user.id, posting, &on).await?;
    }
    sqlx::query("UPDATE asset SET disposed = ?, updated_at = ? WHERE id = ?")
        .bind(&iso)
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let asset = find(&mut tx, id).await?;
    for year in assets::due(&mut tx, &asset, from, date.year()).await? {
        assets::book(&mut tx, auth.user.id, &asset, year)
            .await
            .map_err(rule_error)?;
    }
    audit::record(
        &mut tx,
        auth.user.id,
        "asset.dispose",
        &format!("asset:{id}"),
        None,
        Some(serde_json::json!({ "disposed": iso })),
    )
    .await?;
    let view = view(&mut tx, asset).await?;
    tx.commit().await?;
    Ok(Json(view))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RegisterYear {
    year: i16,
}

/// The Anlage AVEÜR of a year.
#[utoipa::path(get, path = "/assets/register", tag = "books", params(RegisterYear),
    responses((status = OK, body = Register), (status = "default", body = ApiError)))]
pub async fn get_register(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(RegisterYear { year }): Query<RegisterYear>,
) -> Result<Json<Register>, ApiError> {
    if !(2000..=2999).contains(&year) {
        return Err(field_error("year", FieldError::InvalidFormat));
    }
    let mut db = app.db.acquire().await?;
    started(&mut db).await?;
    Ok(Json(assets::register(&assets::all(&mut db).await?, year)))
}
