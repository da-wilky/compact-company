use std::sync::Arc;

use sqlx::SqliteConnection;

use crate::{api::error::ApiError, documents::Kind, render::Layout};

/// The layout a document of `kind` for `customer_id` is rendered with.
pub async fn for_document(
    _db: &mut SqliteConnection,
    _kind: Kind,
    _customer_id: i64,
) -> Result<Arc<Layout>, ApiError> {
    Ok(Arc::new(Layout::built_in()))
}
