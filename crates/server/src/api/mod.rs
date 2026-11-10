use std::sync::Arc;

use serde::Serialize;
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::App;

pub mod business;
pub mod error;
pub mod passkeys;
pub mod session;

use error::{ApiError, ErrorCode, Json};

#[derive(OpenApi)]
#[openapi(info(title = "compact-company"))]
struct Doc;

pub fn routes() -> OpenApiRouter<Arc<App>> {
    let api = OpenApiRouter::new()
        .routes(routes!(health))
        .routes(routes!(business::get_business, business::put_business))
        .routes(routes!(session::get_session))
        .routes(routes!(session::setup_start))
        .routes(routes!(session::setup_finish))
        .routes(routes!(session::sign_in_start))
        .routes(routes!(session::sign_in_finish))
        .routes(routes!(session::sign_out))
        .routes(routes!(passkeys::list_passkeys))
        .routes(routes!(passkeys::add_passkey_start))
        .routes(routes!(passkeys::add_passkey_finish))
        .routes(routes!(passkeys::remove_passkey))
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
