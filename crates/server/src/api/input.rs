use std::collections::BTreeMap;

use super::error::{ApiError, FieldError};

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
