use std::collections::BTreeMap;

use super::error::{ApiError, FieldError};

/// An error about a single field.
pub fn field_error(key: &str, error: FieldError) -> ApiError {
    ApiError::fields(BTreeMap::from([(key.to_owned(), error)]))
}

/// Collects field errors while cleaning up a request's values.
#[derive(Default)]
pub struct Check {
    fields: BTreeMap<String, FieldError>,
}

impl Check {
    /// Trimmed text, or `None` when empty; longer than `max` characters is an error.
    pub fn text(&mut self, key: &str, value: Option<String>, max: usize) -> Option<String> {
        let value = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        if value.as_ref().is_some_and(|v| v.chars().count() > max) {
            self.fail(key, FieldError::TooLong);
        }
        value
    }

    pub fn required(&mut self, key: &str, value: Option<String>, max: usize) -> String {
        let value = self.text(key, value, max);
        if value.is_none() {
            self.fail(key, FieldError::Required);
        }
        value.unwrap_or_default()
    }

    /// An ISO 3166-1 alpha-2 code in capitals; DE when empty.
    pub fn country(&mut self, key: &str, value: Option<String>) -> String {
        let value = self
            .text(key, value, 2)
            .map_or_else(|| "DE".to_owned(), |v| v.to_ascii_uppercase());
        if value.len() != 2 || !value.chars().all(|c| c.is_ascii_uppercase()) {
            self.fail(key, FieldError::InvalidFormat);
        }
        value
    }

    pub fn fail(&mut self, key: &str, error: FieldError) {
        self.fields.entry(key.to_owned()).or_insert(error);
    }

    pub fn done(self) -> Result<(), ApiError> {
        if self.fields.is_empty() {
            Ok(())
        } else {
            Err(ApiError::fields(self.fields))
        }
    }
}
