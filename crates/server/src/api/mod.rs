use std::sync::Arc;

use serde::Serialize;
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::App;

pub mod audit;
pub mod business;
pub mod customers;
pub mod error;
pub mod input;
pub mod invitations;
pub mod oidc;
pub mod passkeys;
pub mod session;
pub mod users;

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
        .routes(routes!(users::list_users))
        .routes(routes!(users::change_user))
        .routes(routes!(invitations::list_invitations, invitations::invite))
        .routes(routes!(invitations::revoke_invitation))
        .routes(routes!(invitations::view_invitation))
        .routes(routes!(invitations::accept_start))
        .routes(routes!(invitations::accept_finish))
        .routes(routes!(oidc::oidc_start))
        .routes(routes!(oidc::oidc_callback))
        .routes(routes!(audit::list_audit))
        .routes(routes!(currencies))
        .routes(routes!(customers::list_customers, customers::create_customer))
        .routes(routes!(customers::get_customer, customers::put_customer))
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

/// The ISO 4217 codes customers may be billed in.
#[utoipa::path(get, path = "/currencies", tag = "customers",
    responses((status = OK, body = Vec<String>), (status = "default", body = ApiError)))]
async fn currencies(_: crate::auth::Auth) -> Json<Vec<&'static str>> {
    Json(crate::money::CURRENCIES.iter().map(|(code, _)| *code).collect())
}
