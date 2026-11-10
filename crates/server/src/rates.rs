use sqlx::SqliteExecutor;

use crate::money::Money;

/// The hourly rate for work on a project: the activity's rate in the customer's currency if the
/// work names an activity that has one, else the project's, else the customer's default.
pub async fn resolve(
    db: impl SqliteExecutor<'_>,
    project_id: i64,
    activity_id: Option<i64>,
) -> Result<Option<Money>, sqlx::Error> {
    let row: Option<(String, Option<i64>)> = sqlx::query_as(
        "SELECT c.currency, coalesce(
             (SELECT r.hourly_rate FROM activity_rate r
              WHERE r.activity_id = ? AND r.currency = c.currency),
             p.hourly_rate, c.hourly_rate)
         FROM project p JOIN customer c ON c.id = p.customer_id WHERE p.id = ?",
    )
    .bind(activity_id)
    .bind(project_id)
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|(currency, minor)| minor.map(|minor| Money { minor, currency })))
}
