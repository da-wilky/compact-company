use std::{collections::BTreeMap, sync::Arc};

use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use super::error::{ApiError, FieldError, Json};
use crate::{
    App,
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
pub async fn get_business(State(app): State<Arc<App>>) -> Result<Json<BusinessView>, ApiError> {
    view(&app).await.map(Json)
}

#[utoipa::path(put, path = "/business", tag = "business", request_body = Business,
    responses((status = OK, body = BusinessView), (status = "default", body = ApiError)))]
pub async fn put_business(
    State(app): State<Arc<App>>,
    Json(business): Json<Business>,
) -> Result<Json<BusinessView>, ApiError> {
    let business = validate(business)?;
    match app.settings.update(&business).await {
        Err(settings::Error::Pinned(keys)) => Err(ApiError::fields(
            keys.into_iter()
                .map(|key| (key, FieldError::Pinned))
                .collect(),
        )),
        result => result.map_err(ApiError::internal),
    }?;
    view(&app).await.map(Json)
}

async fn view(app: &App) -> Result<BusinessView, ApiError> {
    Ok(BusinessView {
        values: app.settings.business().await.map_err(ApiError::internal)?,
        pinned: app.settings.pinned().map(str::to_owned).collect(),
    })
}

fn validate(business: Business) -> Result<Business, ApiError> {
    let mut fields = BTreeMap::new();
    let mut text = |key: &str, value: Option<String>, max: usize| {
        let value = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        if value.as_ref().is_some_and(|v| v.chars().count() > max) {
            fields.insert(key.to_owned(), FieldError::TooLong);
        }
        value
    };
    let business = Business {
        name: text("name", business.name, 200),
        address: text("address", business.address, 500),
        tax_number: text("tax_number", business.tax_number, 50),
        vat_id: text("vat_id", business.vat_id, 20)
            .map(|v| v.replace(' ', "").to_ascii_uppercase()),
        small_business: business.small_business,
    };
    if business.vat_id.as_deref().is_some_and(|v| !is_vat_id(v)) {
        fields.insert("vat_id".to_owned(), FieldError::InvalidFormat);
    }
    if fields.is_empty() {
        Ok(business)
    } else {
        Err(ApiError::fields(fields))
    }
}

/// Two-letter country prefix and 2 to 13 letters or digits, the shape every EU VAT ID shares.
fn is_vat_id(value: &str) -> bool {
    let (country, rest) = value.split_at_checked(2).unwrap_or((value, ""));
    country.chars().all(|c| c.is_ascii_uppercase())
        && (2..=13).contains(&rest.len())
        && rest.chars().all(|c| c.is_ascii_alphanumeric())
}
