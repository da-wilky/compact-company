use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::SqlitePool;

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Business {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_business: Option<bool>,
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
    #[error("{0} is pinned by the config")]
    Pinned(String),
    #[error("invalid setting: {0}")]
    Invalid(serde_json::Error),
}

impl Settings {
    pub fn new(db: SqlitePool, pinned: &Business) -> Self {
        let Ok(Value::Object(pinned)) = serde_json::to_value(pinned) else {
            unreachable!("Business serializes to an object")
        };
        Self { db, pinned }
    }

    pub fn is_pinned(&self, key: &str) -> bool {
        self.pinned.contains_key(key)
    }

    pub async fn business(&self) -> Result<Business, Error> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM setting")
            .fetch_all(&self.db)
            .await?;
        let mut values = Map::new();
        for (key, value) in rows {
            values.insert(key, serde_json::from_str(&value).map_err(Error::Invalid)?);
        }
        values.extend(self.pinned.clone());
        serde_json::from_value(Value::Object(values)).map_err(Error::Invalid)
    }

    pub async fn set(&self, key: &str, value: Value) -> Result<(), Error> {
        if self.is_pinned(key) {
            return Err(Error::Pinned(key.to_owned()));
        }
        let single = Value::Object(Map::from_iter([(key.to_owned(), value.clone())]));
        serde_json::from_value::<Business>(single).map_err(Error::Invalid)?;
        sqlx::query(
            "INSERT INTO setting (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value.to_string())
        .execute(&self.db)
        .await?;
        Ok(())
    }
}
