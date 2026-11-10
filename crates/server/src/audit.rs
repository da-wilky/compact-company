use serde_json::Value;
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
