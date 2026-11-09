use std::sync::Arc;

use serde::Serialize;
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::App;

pub mod business;
pub mod error;

use error::{ApiError, ErrorCode, Json};

#[derive(OpenApi)]
#[openapi(info(title = "compact-company"))]
struct Doc;

pub fn routes() -> OpenApiRouter<Arc<App>> {
    let api = OpenApiRouter::new()
        .routes(routes!(health))
        .routes(routes!(business::get_business, business::put_business))
        .fallback(async || ApiError::new(ErrorCode::NotFound));
    OpenApiRouter::with_openapi(Doc::openapi()).nest("/api", api)
}

#[derive(Serialize, ToSchema)]
pub struct Health {
    status: &'static str,
}

#[utoipa::path(get, path = "/health", tag = "health", responses((status = OK, body = Health)))]
async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}
