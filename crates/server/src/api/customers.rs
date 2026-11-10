use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, now},
    money::{self, DEFAULT_CURRENCY},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Language {
    De,
    En,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomerInput {
    name: Option<String>,
    address: Option<String>,
    email: Option<String>,
    language: Language,
    /// The business's currency when left out.
    currency: Option<String>,
    /// In the minor unit of the currency.
    hourly_rate: Option<i64>,
    note: Option<String>,
    #[serde(default)]
    archived: bool,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Customer {
    id: i64,
    /// Permanent and never reused, e.g. `K-001`.
    number: String,
    #[sqlx(flatten)]
    #[serde(flatten)]
    fields: CustomerFields,
    created_at: i64,
}

/// What an edit may change; also the snapshot the audit log compares.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct CustomerFields {
    name: String,
    address: Option<String>,
    email: Option<String>,
    language: Language,
    currency: String,
    /// In the minor unit of the currency.
    hourly_rate: Option<i64>,
    note: Option<String>,
    archived: bool,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CustomerFilter {
    /// Archived customers instead of active ones.
    #[serde(default)]
    archived: bool,
}

#[utoipa::path(get, path = "/customers", tag = "customers", params(CustomerFilter),
    responses((status = OK, body = Vec<Customer>), (status = "default", body = ApiError)))]
pub async fn list_customers(
    State(app): State<Arc<App>>,
    _: Auth,
    Query(filter): Query<CustomerFilter>,
) -> Result<Json<Vec<Customer>>, ApiError> {
    Ok(Json(
        sqlx::query_as("SELECT * FROM customer WHERE archived = ? ORDER BY name COLLATE NOCASE")
            .bind(filter.archived)
            .fetch_all(&app.db)
            .await?,
    ))
}

#[utoipa::path(get, path = "/customers/{id}", tag = "customers", params(("id" = i64, Path)),
    responses((status = OK, body = Customer), (status = "default", body = ApiError)))]
pub async fn get_customer(
    State(app): State<Arc<App>>,
    _: Auth,
    Path(id): Path<i64>,
) -> Result<Json<Customer>, ApiError> {
    Ok(Json(find(&mut *app.db.acquire().await?, id).await?))
}

#[utoipa::path(post, path = "/customers", tag = "customers", request_body = CustomerInput,
    responses((status = OK, body = Customer), (status = "default", body = ApiError)))]
pub async fn create_customer(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<CustomerInput>,
) -> Result<Json<Customer>, ApiError> {
    let fields = validate(&app, input).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let next: i64 = sqlx::query_scalar(
        "UPDATE sequence SET last = last + 1 WHERE name = 'customer' RETURNING last",
    )
    .fetch_one(&mut *tx)
    .await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO customer
         (number, name, address, email, language, currency, hourly_rate, note, archived, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(format!("K-{next:03}"))
    .bind(&fields.name)
    .bind(&fields.address)
    .bind(&fields.email)
    .bind(fields.language)
    .bind(&fields.currency)
    .bind(fields.hourly_rate)
    .bind(&fields.note)
    .bind(fields.archived)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    let customer = find(&mut tx, id).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "customer.create",
        &format!("customer:{id}"),
        None,
        Some(serde_json::to_value(&customer).map_err(ApiError::internal)?),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(customer))
}

#[utoipa::path(put, path = "/customers/{id}", tag = "customers", request_body = CustomerInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Customer), (status = "default", body = ApiError)))]
pub async fn put_customer(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<CustomerInput>,
) -> Result<Json<Customer>, ApiError> {
    let fields = validate(&app, input).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    sqlx::query(
        "UPDATE customer SET name = ?, address = ?, email = ?, language = ?, currency = ?,
         hourly_rate = ?, note = ?, archived = ? WHERE id = ?",
    )
    .bind(&fields.name)
    .bind(&fields.address)
    .bind(&fields.email)
    .bind(fields.language)
    .bind(&fields.currency)
    .bind(fields.hourly_rate)
    .bind(&fields.note)
    .bind(fields.archived)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if let Some((old, new)) = audit::changes(&before.fields, &fields) {
        audit::record(
            &mut tx,
            auth.user.id,
            "customer.update",
            &format!("customer:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    let customer = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(customer))
}

pub async fn find(db: &mut SqliteConnection, id: i64) -> Result<Customer, ApiError> {
    sqlx::query_as("SELECT * FROM customer WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}


async fn validate(app: &App, input: CustomerInput) -> Result<CustomerFields, ApiError> {
    let mut check = Check::default();
    let currency = match input.currency {
        Some(currency) => currency,
        None => app
            .settings
            .business()
            .await
            .map_err(ApiError::internal)?
            .currency
            .unwrap_or_else(|| DEFAULT_CURRENCY.to_owned()),
    };
    if !money::is_currency(&currency) {
        check.fail("currency", FieldError::InvalidFormat);
    }
    let email = check.text("email", input.email, 254);
    if email
        .as_deref()
        .is_some_and(|e| !e.contains('@') || e.contains(char::is_whitespace))
    {
        check.fail("email", FieldError::InvalidFormat);
    }
    if input.hourly_rate.is_some_and(|rate| rate < 0) {
        check.fail("hourly_rate", FieldError::InvalidFormat);
    }
    let fields = CustomerFields {
        name: check.required("name", input.name, 200),
        address: check.text("address", input.address, 500),
        email,
        language: input.language,
        currency,
        hourly_rate: input.hourly_rate,
        note: check.text("note", input.note, 2000),
        archived: input.archived,
    };
    check.done()?;
    Ok(fields)
}
