use std::sync::Arc;

use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use super::{
    error::{ApiError, FieldError, Json},
    input::Check,
};
use crate::{
    App,
    auth::{Auth, OwnerOnly, OwnerOrAccountant},
    documents::NumberFormat,
    money,
    settings::{self, Business},
};

#[derive(Serialize, ToSchema)]
pub struct BusinessView {
    values: Business,
    /// Keys fixed by the config; the app shows them read-only.
    pinned: Vec<String>,
}

#[utoipa::path(get, path = "/business", tag = "business",
    responses((status = OK, body = BusinessView), (status = "default", body = ApiError)))]
pub async fn get_business(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
) -> Result<Json<BusinessView>, ApiError> {
    view(&app).await.map(Json)
}

#[utoipa::path(put, path = "/business", tag = "business", request_body = Business,
    responses((status = OK, body = BusinessView), (status = "default", body = ApiError)))]
pub async fn put_business(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(business): Json<Business>,
) -> Result<Json<BusinessView>, ApiError> {
    let business = validate(business)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    match app.settings.update(&mut tx, auth.user.id, &business).await {
        Err(settings::Error::Pinned(keys)) => Err(ApiError::fields(
            keys.into_iter()
                .map(|key| (key, FieldError::Pinned))
                .collect(),
        )),
        result => result.map_err(ApiError::internal),
    }?;
    tx.commit().await?;
    view(&app).await.map(Json)
}

async fn view(app: &App) -> Result<BusinessView, ApiError> {
    Ok(BusinessView {
        values: app.settings.business().await.map_err(ApiError::internal)?,
        pinned: app.settings.pinned().map(str::to_owned).collect(),
    })
}

fn validate(business: Business) -> Result<Business, ApiError> {
    let mut check = Check::default();
    let b = business;
    let business = Business {
        name: check.text("name", b.name, 200),
        street: check.text("street", b.street, 500),
        postcode: check.text("postcode", b.postcode, 20),
        city: check.text("city", b.city, 100),
        country: b.country.map(|c| check.country("country", Some(c))),
        email: check.text("email", b.email, 254),
        phone: check.text("phone", b.phone, 50),
        website: check.text("website", b.website, 200),
        tax_number: check.text("tax_number", b.tax_number, 50),
        vat_id: check
            .text("vat_id", b.vat_id, 20)
            .map(|v| v.replace(' ', "").to_ascii_uppercase()),
        taxation: b.taxation,
        legal_form: check.text("legal_form", b.legal_form, 100),
        register_court: check.text("register_court", b.register_court, 100),
        register_number: check.text("register_number", b.register_number, 50),
        managing_directors: check.text("managing_directors", b.managing_directors, 300),
        bank_name: check.text("bank_name", b.bank_name, 100),
        iban: check
            .text("iban", b.iban, 42)
            .map(|v| v.replace(' ', "").to_ascii_uppercase()),
        bic: check
            .text("bic", b.bic, 11)
            .map(|v| v.replace(' ', "").to_ascii_uppercase()),
        currency: b.currency,
        timezone: check.text("timezone", b.timezone, 64),
        vat_rate: b.vat_rate,
        payment_days: b.payment_days,
        quote_valid_days: b.quote_valid_days,
        quote_number_format: check.text("quote_number_format", b.quote_number_format, 40),
        order_confirmation_number_format: check.text(
            "order_confirmation_number_format",
            b.order_confirmation_number_format,
            40,
        ),
        invoice_number_format: check.text("invoice_number_format", b.invoice_number_format, 40),
        delivery_note_number_format: check.text(
            "delivery_note_number_format",
            b.delivery_note_number_format,
            40,
        ),
        acceptance_report_number_format: check.text(
            "acceptance_report_number_format",
            b.acceptance_report_number_format,
            40,
        ),
        dunning_number_format: check.text("dunning_number_format", b.dunning_number_format, 40),
        datev_consultant_number: b.datev_consultant_number,
        datev_client_number: b.datev_client_number,
    };
    if business
        .datev_consultant_number
        .is_some_and(|n| !(1001..=9_999_999).contains(&n))
    {
        check.fail("datev_consultant_number", FieldError::InvalidFormat);
    }
    if business
        .datev_client_number
        .is_some_and(|n| !(1..=99_999).contains(&n))
    {
        check.fail("datev_client_number", FieldError::InvalidFormat);
    }
    if business
        .currency
        .as_deref()
        .is_some_and(|c| !money::is_currency(c))
    {
        check.fail("currency", FieldError::InvalidFormat);
    }
    if business.vat_id.as_deref().is_some_and(|v| !is_vat_id(v)) {
        check.fail("vat_id", FieldError::InvalidFormat);
    }
    if business.iban.as_deref().is_some_and(|v| !is_iban(v)) {
        check.fail("iban", FieldError::InvalidFormat);
    }
    if business.bic.as_deref().is_some_and(|v| {
        !matches!(v.len(), 8 | 11) || !v.chars().all(|c| c.is_ascii_alphanumeric())
    }) {
        check.fail("bic", FieldError::InvalidFormat);
    }
    if business
        .email
        .as_deref()
        .is_some_and(|e| !e.contains('@') || e.contains(char::is_whitespace))
    {
        check.fail("email", FieldError::InvalidFormat);
    }
    if business
        .timezone
        .as_deref()
        .is_some_and(|tz| jiff::tz::TimeZone::get(tz).is_err())
    {
        check.fail("timezone", FieldError::InvalidFormat);
    }
    if business.vat_rate.is_some_and(|v| {
        v.is_sign_negative() || v > rust_decimal::Decimal::ONE_HUNDRED || v.scale() > 2
    }) {
        check.fail("vat_rate", FieldError::InvalidFormat);
    }
    if business.quote_valid_days.is_some_and(|d| d > 365) {
        check.fail("quote_valid_days", FieldError::InvalidFormat);
    }
    if business.payment_days.is_some_and(|d| d > 365) {
        check.fail("payment_days", FieldError::InvalidFormat);
    }
    for (key, format) in [
        ("quote_number_format", &business.quote_number_format),
        (
            "order_confirmation_number_format",
            &business.order_confirmation_number_format,
        ),
        ("invoice_number_format", &business.invoice_number_format),
        (
            "delivery_note_number_format",
            &business.delivery_note_number_format,
        ),
        (
            "acceptance_report_number_format",
            &business.acceptance_report_number_format,
        ),
        ("dunning_number_format", &business.dunning_number_format),
    ] {
        if format
            .as_deref()
            .is_some_and(|f| NumberFormat::parse(f).is_none())
        {
            check.fail(key, FieldError::InvalidFormat);
        }
    }
    check.done()?;
    Ok(business)
}

/// Country code, check digits and up to 30 letters or digits, with the ISO 7064 checksum.
pub fn is_iban(value: &str) -> bool {
    let valid_shape = (15..=34).contains(&value.len())
        && value.chars().all(|c| c.is_ascii_alphanumeric())
        && value[..2].chars().all(|c| c.is_ascii_uppercase())
        && value[2..4].chars().all(|c| c.is_ascii_digit());
    valid_shape
        && value[4..]
            .chars()
            .chain(value[..4].chars())
            .fold(0u32, |rest, c| {
                let digit = c.to_digit(36).expect("checked alphanumeric");
                if digit < 10 {
                    (rest * 10 + digit) % 97
                } else {
                    (rest * 100 + digit) % 97
                }
            })
            == 1
}

/// Two-letter country prefix and 2 to 13 letters or digits, the shape every EU VAT ID shares.
pub fn is_vat_id(value: &str) -> bool {
    let (country, rest) = value.split_at_checked(2).unwrap_or((value, ""));
    country.chars().all(|c| c.is_ascii_uppercase())
        && (2..=13).contains(&rest.len())
        && rest.chars().all(|c| c.is_ascii_alphanumeric())
}
