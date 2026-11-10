use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// ISO 4217 codes the app accepts with their minor-unit digits; adding a currency is one line here.
pub const CURRENCIES: &[(&str, u32)] = &[
    ("EUR", 2),
    ("CHF", 2),
    ("GBP", 2),
    ("USD", 2),
    ("DKK", 2),
    ("SEK", 2),
    ("NOK", 2),
    ("PLN", 2),
    ("CZK", 2),
];

pub const DEFAULT_CURRENCY: &str = "EUR";

pub fn is_currency(code: &str) -> bool {
    CURRENCIES.iter().any(|(c, _)| *c == code)
}

/// An amount in the minor unit of its currency (cents for EUR), never a float.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Money {
    pub minor: i64,
    pub currency: String,
}
