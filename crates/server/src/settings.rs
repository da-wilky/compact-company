use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{SqliteConnection, SqliteExecutor, SqlitePool};
use utoipa::ToSchema;

use crate::audit;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Business {
    pub name: Option<String>,
    pub street: Option<String>,
    pub postcode: Option<String>,
    pub city: Option<String>,
    /// ISO 3166-1 alpha-2; DE when unset.
    pub country: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub tax_number: Option<String>,
    pub vat_id: Option<String>,
    /// How the business owes VAT; accrual when unset.
    pub taxation: Option<Taxation>,
    /// The legal form of a UG or GmbH, such as `UG (haftungsbeschränkt)`; with it, documents
    /// must also carry the register and managing directors (§ 35a GmbHG).
    pub legal_form: Option<String>,
    pub register_court: Option<String>,
    pub register_number: Option<String>,
    pub managing_directors: Option<String>,
    pub bank_name: Option<String>,
    pub iban: Option<String>,
    pub bic: Option<String>,
    /// ISO 4217 code new customers are billed in; EUR when unset.
    pub currency: Option<String>,
    /// IANA name such as `Europe/Berlin`, which is the default; dates on documents are in it.
    pub timezone: Option<String>,
    /// Percent charged on new lines unless the business is a small business; 19 when unset.
    pub vat_rate: Option<rust_decimal::Decimal>,
    /// Days a quote stands after it is issued; 30 when unset.
    pub quote_valid_days: Option<u32>,
    /// Days from issue until an invoice is due; 14 when unset.
    pub payment_days: Option<u32>,
    pub quote_number_format: Option<String>,
    pub order_confirmation_number_format: Option<String>,
    /// Also numbers cancellation invoices, which are invoices.
    pub invoice_number_format: Option<String>,
    pub delivery_note_number_format: Option<String>,
    pub acceptance_report_number_format: Option<String>,
    pub dunning_number_format: Option<String>,
    /// How often VAT advance returns are due; quarterly when unset.
    pub vat_period: Option<crate::deadlines::VatPeriod>,
    /// Permanent extension (Dauerfristverlängerung): advance returns are due a month later.
    pub vat_extension: Option<bool>,
    /// Beraternummer in the DATEV export, 1001 to 9999999; 1001 when unset.
    pub datev_consultant_number: Option<u32>,
    /// Mandantennummer in the DATEV export, 1 to 99999; 1 when unset.
    pub datev_client_number: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Taxation {
    /// Exempt from charging VAT under § 19 UStG.
    SmallBusiness,
    /// VAT on an invoice is owed when its payment arrives (Istversteuerung).
    Cash,
    /// VAT on an invoice is owed for the period it is issued in (Sollversteuerung).
    Accrual,
}

impl Business {
    pub fn taxation(&self) -> Taxation {
        self.taxation.unwrap_or(Taxation::Accrual)
    }

    pub fn small_business(&self) -> bool {
        self.taxation() == Taxation::SmallBusiness
    }
}

/// Business data and defaults: stored in the database unless the config pins them (ADR 0017).
pub struct Settings {
    db: SqlitePool,
    pinned: Map<String, Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("pinned by the config: {}", .0.join(", "))]
    Pinned(Vec<String>),
    #[error("stored setting is invalid: {0}")]
    Corrupt(serde_json::Error),
}

fn to_map(business: &Business) -> Map<String, Value> {
    let Ok(Value::Object(map)) = serde_json::to_value(business) else {
        unreachable!("Business serializes to an object")
    };
    map
}

impl Settings {
    pub fn new(db: SqlitePool, pinned: &Business) -> Self {
        let pinned = to_map(pinned)
            .into_iter()
            .filter(|(_, value)| !value.is_null())
            .collect();
        Self { db, pinned }
    }

    pub fn pinned(&self) -> impl Iterator<Item = &str> {
        self.pinned.keys().map(String::as_str)
    }

    pub async fn business(&self) -> Result<Business, Error> {
        self.read(&self.db).await
    }

    async fn read(&self, db: impl SqliteExecutor<'_>) -> Result<Business, Error> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM setting")
            .fetch_all(db)
            .await?;
        let mut values = Map::new();
        for (key, value) in rows {
            values.insert(key, serde_json::from_str(&value).map_err(Error::Corrupt)?);
        }
        values.extend(self.pinned.clone());
        serde_json::from_value(Value::Object(values)).map_err(Error::Corrupt)
    }

    /// Stores every key that is not pinned and records the changed keys in the audit log; a
    /// pinned key must keep its pinned value.
    pub async fn update(
        &self,
        tx: &mut SqliteConnection,
        user_id: i64,
        business: &Business,
    ) -> Result<(), Error> {
        let values = to_map(business);
        let changed_pins: Vec<String> = self
            .pinned
            .iter()
            .filter(|(key, pinned)| {
                values
                    .get(*key)
                    .is_some_and(|v| !v.is_null() && v != *pinned)
            })
            .map(|(key, _)| key.clone())
            .collect();
        if !changed_pins.is_empty() {
            return Err(Error::Pinned(changed_pins));
        }
        let current = to_map(&self.read(&mut *tx).await?);
        let (mut before, mut after) = (Map::new(), Map::new());
        for (key, value) in values.iter().filter(|(key, value)| {
            !self.pinned.contains_key(*key) && current.get(*key) != Some(value)
        }) {
            before.insert(key.clone(), current[key].clone());
            after.insert(key.clone(), value.clone());
            if value.is_null() {
                sqlx::query("DELETE FROM setting WHERE key = ?")
                    .bind(key)
                    .execute(&mut *tx)
                    .await?;
            } else {
                sqlx::query(
                    "INSERT INTO setting (key, value) VALUES (?, ?)
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                )
                .bind(key)
                .bind(value.to_string())
                .execute(&mut *tx)
                .await?;
            }
        }
        if !after.is_empty() {
            audit::record(
                tx,
                user_id,
                "business.update",
                "business",
                Some(Value::Object(before)),
                Some(Value::Object(after)),
            )
            .await?;
        }
        Ok(())
    }
}
