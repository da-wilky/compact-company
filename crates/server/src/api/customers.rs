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

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::De => "de",
            Self::En => "en",
        }
    }

    pub fn pick<'a>(self, de: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::De => de,
            Self::En => en,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomerInput {
    name: Option<String>,
    /// All of the customer's billing addresses: one without an id is added, one left out is
    /// removed. The first is the default unless one says otherwise.
    #[serde(default)]
    addresses: Vec<AddressInput>,
    email: Option<String>,
    language: Language,
    /// The business's currency when left out.
    currency: Option<String>,
    /// In the minor unit of the currency.
    hourly_rate: Option<i64>,
    note: Option<String>,
    vat_id: Option<String>,
    /// The buyer reference public customers give for XRechnung.
    leitweg_id: Option<String>,
    /// Whether invoices also go out as XRechnung.
    #[serde(default)]
    xrechnung: bool,
    #[serde(default)]
    archived: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddressInput {
    /// An existing address of this customer; a new one when left out.
    id: Option<i64>,
    /// A name to pick it by, e.g. Buchhaltung.
    label: Option<String>,
    /// One line each for c/o, department and street; at most three.
    street: Option<String>,
    postcode: Option<String>,
    city: Option<String>,
    /// ISO 3166-1 alpha-2; DE when left out.
    country: Option<String>,
    /// Where documents to this address are emailed; the customer's email when left out.
    email: Option<String>,
    #[serde(default)]
    is_default: bool,
}

/// A billing address; the customer's name heads it on every document.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
#[schema(as = CustomerAddress)]
pub struct Address {
    id: i64,
    label: Option<String>,
    street: Option<String>,
    postcode: Option<String>,
    city: Option<String>,
    country: String,
    email: Option<String>,
    is_default: bool,
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
    /// The default first, then in the order they were added.
    #[sqlx(skip)]
    addresses: Vec<Address>,
    email: Option<String>,
    language: Language,
    currency: String,
    /// In the minor unit of the currency.
    hourly_rate: Option<i64>,
    note: Option<String>,
    vat_id: Option<String>,
    leitweg_id: Option<String>,
    xrechnung: bool,
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
    let mut db = app.db.acquire().await?;
    let mut customers: Vec<Customer> =
        sqlx::query_as("SELECT * FROM customer WHERE archived = ? ORDER BY name COLLATE NOCASE")
            .bind(filter.archived)
            .fetch_all(&mut *db)
            .await?;
    for customer in &mut customers {
        customer.fields.addresses = addresses(&mut db, customer.id).await?;
    }
    Ok(Json(customers))
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
    let (fields, addresses) = validate(&app, input).await?;
    let mut check = Check::default();
    for (i, address) in addresses.iter().enumerate() {
        if address.id.is_some() {
            check.fail(&format!("addresses.{i}.id"), FieldError::InvalidFormat);
        }
    }
    check.done()?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let next: i64 = sqlx::query_scalar(
        "UPDATE sequence SET last = last + 1 WHERE name = 'customer' RETURNING last",
    )
    .fetch_one(&mut *tx)
    .await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO customer
         (number, name, email, language, currency, hourly_rate, note,
          vat_id, leitweg_id, xrechnung, archived, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(format!("K-{next:03}"))
    .bind(&fields.name)
    .bind(&fields.email)
    .bind(fields.language)
    .bind(&fields.currency)
    .bind(fields.hourly_rate)
    .bind(&fields.note)
    .bind(&fields.vat_id)
    .bind(&fields.leitweg_id)
    .bind(fields.xrechnung)
    .bind(fields.archived)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    save_addresses(&mut tx, id, addresses).await?;
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
    let (fields, addresses) = validate(&app, input).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    // project rates are in the customer's currency and would silently change meaning
    let rated: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM project WHERE customer_id = ? AND hourly_rate IS NOT NULL)",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let mut check = Check::default();
    if rated && fields.currency != before.fields.currency {
        check.fail("currency", FieldError::Locked);
    }
    for (i, address) in addresses.iter().enumerate() {
        if address
            .id
            .is_some_and(|a| !before.fields.addresses.iter().any(|b| b.id == a))
        {
            check.fail(&format!("addresses.{i}.id"), FieldError::InvalidFormat);
        }
    }
    check.done()?;
    sqlx::query(
        "UPDATE customer SET name = ?, email = ?, language = ?, currency = ?,
         hourly_rate = ?, note = ?, vat_id = ?, leitweg_id = ?, xrechnung = ?, archived = ? WHERE id = ?",
    )
    .bind(&fields.name)
    .bind(&fields.email)
    .bind(fields.language)
    .bind(&fields.currency)
    .bind(fields.hourly_rate)
    .bind(&fields.note)
    .bind(&fields.vat_id)
    .bind(&fields.leitweg_id)
    .bind(fields.xrechnung)
    .bind(fields.archived)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    for gone in before
        .fields
        .addresses
        .iter()
        .filter(|b| !addresses.iter().any(|a| a.id == Some(b.id)))
    {
        remove_address(&mut tx, gone.id).await?;
    }
    save_addresses(&mut tx, id, addresses).await?;
    let customer = find(&mut tx, id).await?;
    if let Some((old, new)) = audit::changes(&before.fields, &customer.fields) {
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
    tx.commit().await?;
    Ok(Json(customer))
}

pub async fn find(db: &mut SqliteConnection, id: i64) -> Result<Customer, ApiError> {
    let mut customer: Customer = sqlx::query_as("SELECT * FROM customer WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    customer.fields.addresses = addresses(db, id).await?;
    Ok(customer)
}

async fn addresses(db: &mut SqliteConnection, customer: i64) -> Result<Vec<Address>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, label, street, postcode, city, country, email, is_default FROM customer_address
         WHERE customer_id = ? AND NOT archived ORDER BY is_default DESC, id",
    )
    .bind(customer)
    .fetch_all(db)
    .await
}

/// Drafts and recurring invoices that named it fall back to the default; an issued document
/// keeps naming it.
async fn remove_address(tx: &mut SqliteConnection, id: i64) -> Result<(), sqlx::Error> {
    for sql in [
        "UPDATE document SET address_id = NULL WHERE address_id = ? AND status = 'draft'",
        "UPDATE recurring_invoice SET address_id = NULL WHERE address_id = ?",
    ] {
        sqlx::query(sql).bind(id).execute(&mut *tx).await?;
    }
    let issued: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM document WHERE address_id = ?)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let sql = if issued {
        "UPDATE customer_address SET archived = 1, is_default = 0 WHERE id = ?"
    } else {
        "DELETE FROM customer_address WHERE id = ?"
    };
    sqlx::query(sql).bind(id).execute(&mut *tx).await?;
    Ok(())
}

async fn save_addresses(
    tx: &mut SqliteConnection,
    customer: i64,
    addresses: Vec<AddressFields>,
) -> Result<(), sqlx::Error> {
    // the unique default index would refuse moving the default in one step
    sqlx::query("UPDATE customer_address SET is_default = 0 WHERE customer_id = ?")
        .bind(customer)
        .execute(&mut *tx)
        .await?;
    for a in addresses {
        let query = match a.id {
            Some(id) => sqlx::query(
                "UPDATE customer_address SET label = ?, street = ?, postcode = ?, city = ?,
                 country = ?, email = ?, is_default = ? WHERE id = ? AND customer_id = ?",
            )
            .bind(a.label)
            .bind(a.street)
            .bind(a.postcode)
            .bind(a.city)
            .bind(a.country)
            .bind(a.email)
            .bind(a.is_default)
            .bind(id),
            None => sqlx::query(
                "INSERT INTO customer_address
                 (label, street, postcode, city, country, email, is_default, customer_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(a.label)
            .bind(a.street)
            .bind(a.postcode)
            .bind(a.city)
            .bind(a.country)
            .bind(a.email)
            .bind(a.is_default),
        };
        query.bind(customer).execute(&mut *tx).await?;
    }
    Ok(())
}

struct AddressFields {
    id: Option<i64>,
    label: Option<String>,
    street: Option<String>,
    postcode: Option<String>,
    city: Option<String>,
    country: String,
    email: Option<String>,
    is_default: bool,
}

fn checked_email(check: &mut Check, key: &str, value: Option<String>) -> Option<String> {
    let email = check.text(key, value, 254);
    if email
        .as_deref()
        .is_some_and(|e| !e.contains('@') || e.contains(char::is_whitespace))
    {
        check.fail(key, FieldError::InvalidFormat);
    }
    email
}

async fn validate(
    app: &App,
    input: CustomerInput,
) -> Result<(CustomerFields, Vec<AddressFields>), ApiError> {
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
    let email = checked_email(&mut check, "email", input.email);
    if input.hourly_rate.is_some_and(|rate| !money::is_rate(rate)) {
        check.fail("hourly_rate", FieldError::InvalidFormat);
    }
    let defaults = input.addresses.iter().filter(|a| a.is_default).count();
    if defaults > 1 || input.addresses.len() > 50 {
        check.fail("addresses", FieldError::InvalidFormat);
    }
    let mut ids = std::collections::BTreeSet::new();
    let addresses = input
        .addresses
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            if a.id.is_some_and(|id| !ids.insert(id)) {
                check.fail(&format!("addresses.{i}.id"), FieldError::InvalidFormat);
            }
            let street = check.text(&format!("addresses.{i}.street"), a.street, 500);
            // an e-invoice carries three address lines
            if street.as_deref().is_some_and(|s| s.lines().count() > 3) {
                check.fail(&format!("addresses.{i}.street"), FieldError::InvalidFormat);
            }
            AddressFields {
                id: a.id,
                label: check.text(&format!("addresses.{i}.label"), a.label, 100),
                street,
                postcode: check.text(&format!("addresses.{i}.postcode"), a.postcode, 20),
                city: check.text(&format!("addresses.{i}.city"), a.city, 100),
                country: check.country(&format!("addresses.{i}.country"), a.country),
                email: checked_email(&mut check, &format!("addresses.{i}.email"), a.email),
                is_default: a.is_default || (defaults == 0 && i == 0),
            }
        })
        .collect();
    let fields = CustomerFields {
        name: check.required("name", input.name, 200),
        addresses: Vec::new(),
        email,
        language: input.language,
        currency,
        hourly_rate: input.hourly_rate,
        note: check.text("note", input.note, 2000),
        vat_id: check
            .text("vat_id", input.vat_id, 20)
            .map(|v| v.replace(' ', "").to_ascii_uppercase()),
        leitweg_id: check.text("leitweg_id", input.leitweg_id, 50),
        xrechnung: input.xrechnung,
        archived: input.archived,
    };
    check.done()?;
    Ok((fields, addresses))
}
