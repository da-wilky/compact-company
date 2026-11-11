use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{
    books::started,
    documents::{business, rule_error},
    error::{ApiError, FieldError, Json, Query},
    input::field_error,
};
use crate::{
    App,
    auth::{Auth, OwnerOrAccountant},
    settings::Taxation,
    tax::{self, Basis, Euer, Figure},
};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Period {
    /// ISO date, inclusive.
    from: String,
    /// ISO date, inclusive.
    to: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VatReturn {
    /// None for a small business, which files no VAT return.
    basis: Option<Basis>,
    from: String,
    to: String,
    /// Kz 81 and 86 with their tax as `81_tax` and `86_tax`, Kz 66 and Kz 83.
    figures: Vec<Figure>,
}

fn check_period(from: &str, to: &str) -> Result<(), ApiError> {
    let parse = |key: &str, value: &str| {
        value
            .parse::<jiff::civil::Date>()
            .map_err(|_| field_error(key, FieldError::InvalidFormat))
    };
    if parse("from", from)? > parse("to", to)? {
        return Err(field_error("to", FieldError::InvalidFormat));
    }
    Ok(())
}

/// The figures of a VAT advance return for a period, each with the postings behind it.
#[utoipa::path(get, path = "/tax/vat-return", tag = "tax", params(Period),
    responses((status = OK, body = VatReturn), (status = "default", body = ApiError)))]
pub async fn get_vat_return(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(period): Query<Period>,
) -> Result<Json<VatReturn>, ApiError> {
    check_period(&period.from, &period.to)?;
    let basis = match business(&app).await?.taxation() {
        Taxation::Accrual => Some(Basis::Accrual),
        Taxation::Cash => Some(Basis::Cash),
        Taxation::SmallBusiness => None,
    };
    let mut db = app.db.acquire().await?;
    started(&mut db).await?;
    let figures = match basis {
        Some(basis) => tax::vat_return(&mut db, basis, &period.from, &period.to)
            .await
            .map_err(rule_error)?,
        None => Vec::new(),
    };
    Ok(Json(VatReturn {
        basis,
        from: period.from,
        to: period.to,
        figures,
    }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct Year {
    year: i16,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EuerView {
    year: i16,
    small_business: bool,
    #[serde(flatten)]
    euer: Euer,
}

/// The Anlage EÜR of a year, each line with the postings behind it.
#[utoipa::path(get, path = "/tax/euer", tag = "tax", params(Year),
    responses((status = OK, body = EuerView), (status = "default", body = ApiError)))]
pub async fn get_euer(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(Year { year }): Query<Year>,
) -> Result<Json<EuerView>, ApiError> {
    if !(2000..=2999).contains(&year) {
        return Err(field_error("year", FieldError::InvalidFormat));
    }
    let small_business = business(&app).await?.small_business();
    let mut db = app.db.acquire().await?;
    started(&mut db).await?;
    let euer = tax::euer(
        &mut db,
        small_business,
        &format!("{year}-01-01"),
        &format!("{year}-12-31"),
    )
    .await
    .map_err(rule_error)?;
    Ok(Json(EuerView {
        year,
        small_business,
        euer,
    }))
}
