use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    business::{is_iban, is_vat_id},
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplierInput {
    name: Option<String>,
    street: Option<String>,
    postcode: Option<String>,
    city: Option<String>,
    /// ISO 3166-1 alpha-2; DE when empty.
    country: Option<String>,
    vat_id: Option<String>,
    iban: Option<String>,
    email: Option<String>,
    /// The ledger account its receipts' lines are booked on unless chosen otherwise.
    account_id: Option<i64>,
    #[serde(default)]
    archived: bool,
}

/// What an edit may change; also the snapshot the audit log compares.
#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct SupplierFields {
    pub name: String,
    pub street: Option<String>,
    pub postcode: Option<String>,
    pub city: Option<String>,
    pub country: String,
    pub vat_id: Option<String>,
    pub iban: Option<String>,
    pub email: Option<String>,
    pub account_id: Option<i64>,
    pub archived: bool,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Supplier {
    id: i64,
    #[serde(flatten)]
    #[sqlx(flatten)]
    fields: SupplierFields,
    created_at: i64,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SupplierFilter {
    /// Archived suppliers instead of active ones.
    #[serde(default)]
    archived: bool,
}

const SUPPLIER: &str = "SELECT id, name, street, postcode, city, country, vat_id, iban, email,
    account_id, archived, created_at FROM supplier";

#[utoipa::path(get, path = "/suppliers", tag = "books", params(SupplierFilter),
    responses((status = OK, body = Vec<Supplier>), (status = "default", body = ApiError)))]
pub async fn list_suppliers(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(filter): Query<SupplierFilter>,
) -> Result<Json<Vec<Supplier>>, ApiError> {
    let sql = format!("{SUPPLIER} WHERE archived = ? ORDER BY name COLLATE NOCASE");
    Ok(Json(
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(filter.archived)
            .fetch_all(&app.db)
            .await?,
    ))
}

#[utoipa::path(post, path = "/suppliers", tag = "books", request_body = SupplierInput,
    responses((status = OK, body = Supplier), (status = "default", body = ApiError)))]
pub async fn create_supplier(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<SupplierInput>,
) -> Result<Json<Supplier>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let fields = validate(&mut tx, input).await?;
    let id = insert(&mut tx, &fields).await?;
    let supplier = find(&mut tx, id).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "supplier.create",
        &format!("supplier:{id}"),
        None,
        Some(serde_json::to_value(&supplier.fields).map_err(ApiError::internal)?),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(supplier))
}

pub async fn insert(
    db: &mut SqliteConnection,
    fields: &SupplierFields,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO supplier (name, street, postcode, city, country, vat_id, iban, email,
         account_id, archived, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&fields.name)
    .bind(&fields.street)
    .bind(&fields.postcode)
    .bind(&fields.city)
    .bind(&fields.country)
    .bind(&fields.vat_id)
    .bind(&fields.iban)
    .bind(&fields.email)
    .bind(fields.account_id)
    .bind(fields.archived)
    .bind(now())
    .fetch_one(db)
    .await
}

#[utoipa::path(put, path = "/suppliers/{id}", tag = "books", request_body = SupplierInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Supplier), (status = "default", body = ApiError)))]
pub async fn put_supplier(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<SupplierInput>,
) -> Result<Json<Supplier>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    let fields = validate(&mut tx, input).await?;
    sqlx::query(
        "UPDATE supplier SET name = ?, street = ?, postcode = ?, city = ?, country = ?, vat_id = ?,
         iban = ?, email = ?, account_id = ?, archived = ? WHERE id = ?",
    )
    .bind(&fields.name)
    .bind(&fields.street)
    .bind(&fields.postcode)
    .bind(&fields.city)
    .bind(&fields.country)
    .bind(&fields.vat_id)
    .bind(&fields.iban)
    .bind(&fields.email)
    .bind(fields.account_id)
    .bind(fields.archived)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if let Some((old, new)) = audit::changes(&before.fields, &fields) {
        audit::record(
            &mut tx,
            auth.user.id,
            "supplier.update",
            &format!("supplier:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    let supplier = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(supplier))
}

pub async fn find(db: &mut SqliteConnection, id: i64) -> Result<Supplier, ApiError> {
    let sql = format!("{SUPPLIER} WHERE id = ?");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}

async fn validate(
    db: &mut SqliteConnection,
    input: SupplierInput,
) -> Result<SupplierFields, ApiError> {
    let mut check = Check::default();
    let fields = SupplierFields {
        name: check.required("name", input.name, 200),
        street: check.text("street", input.street, 500),
        postcode: check.text("postcode", input.postcode, 20),
        city: check.text("city", input.city, 100),
        country: check.country("country", input.country),
        vat_id: check
            .text("vat_id", input.vat_id, 20)
            .map(|v| v.replace(' ', "").to_ascii_uppercase()),
        iban: check
            .text("iban", input.iban, 42)
            .map(|v| v.replace(' ', "").to_ascii_uppercase()),
        email: check.text("email", input.email, 254),
        account_id: input.account_id,
        archived: input.archived,
    };
    if fields.vat_id.as_deref().is_some_and(|v| !is_vat_id(v)) {
        check.fail("vat_id", FieldError::InvalidFormat);
    }
    if fields.iban.as_deref().is_some_and(|v| !is_iban(v)) {
        check.fail("iban", FieldError::InvalidFormat);
    }
    if let Some(account) = fields.account_id {
        let open: bool = sqlx::query_scalar(
            "SELECT count(*) > 0 FROM ledger_account WHERE id = ? AND NOT archived",
        )
        .bind(account)
        .fetch_one(db)
        .await?;
        if !open {
            check.fail("account_id", FieldError::InvalidFormat);
        }
    }
    check.done()?;
    Ok(fields)
}
