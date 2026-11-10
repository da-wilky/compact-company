use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::SqliteConnection;

use crate::auth::now;

/// Appends an entry; call it in the transaction that makes the change, so both or neither land.
pub async fn record(
    tx: &mut SqliteConnection,
    user_id: i64,
    action: &str,
    subject: &str,
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit (at, user_id, action, subject, before, after)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(now())
    .bind(user_id)
    .bind(action)
    .bind(subject)
    .bind(before.map(|v| v.to_string()))
    .bind(after.map(|v| v.to_string()))
    .execute(tx)
    .await?;
    Ok(())
}

/// The fields that differ between two snapshots of the same thing, as before and after maps;
/// `None` when nothing changed.
pub fn changes(before: &impl Serialize, after: &impl Serialize) -> Option<(Value, Value)> {
    let (Ok(Value::Object(before)), Ok(Value::Object(after))) =
        (serde_json::to_value(before), serde_json::to_value(after))
    else {
        unreachable!("snapshots serialize to objects")
    };
    let changed: Map<String, Value> = after
        .into_iter()
        .filter(|(key, value)| before.get(key) != Some(value))
        .collect();
    if changed.is_empty() {
        return None;
    }
    let old = changed
        .keys()
        .map(|key| (key.clone(), before.get(key).cloned().unwrap_or(Value::Null)))
        .collect();
    Some((Value::Object(old), Value::Object(changed)))
}
