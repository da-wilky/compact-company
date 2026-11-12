use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use serde::Serialize;
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::App;

pub mod activities;
pub mod audit;
pub mod bank;
pub mod billing;
pub mod books;
pub mod business;
pub mod customers;
pub mod deadlines;
pub mod documents;
pub mod error;
pub mod exports;
pub mod input;
pub mod invitations;
pub mod layouts;
pub mod oidc;
pub mod open_items;
pub mod overview;
pub mod passkeys;
pub mod projects;
pub mod receipts;
pub mod sending;
pub mod session;
pub mod snippets;
pub mod statuses;
pub mod suppliers;
pub mod tax;
pub mod time;
pub mod todos;
pub mod tokens;
pub mod users;
pub mod years;

use error::{ApiError, ErrorCode, Json};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "compact-company",
        description = "Signed in by session cookie, or by an API token sent as `Authorization: \
            Bearer`. A token acts as its user within its scopes: `work:read`, `work:write`, \
            `documents:read`, `documents:write`, `books:read`; it never issues, sends or books."
    ),
    modifiers(&Tokens)
)]
struct Doc;

struct Tokens;

impl utoipa::Modify for Tokens {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
        openapi
            .components
            .get_or_insert_with(Default::default)
            .add_security_scheme(
                "token",
                SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
            );
    }
}

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
        .routes(routes!(
            customers::list_customers,
            customers::create_customer
        ))
        .routes(routes!(customers::get_customer, customers::put_customer))
        .routes(routes!(projects::list_projects, projects::create_project))
        .routes(routes!(projects::get_project, projects::put_project))
        .routes(routes!(
            activities::list_activities,
            activities::create_activity
        ))
        .routes(routes!(activities::put_activity))
        .routes(routes!(statuses::list_statuses, statuses::put_statuses))
        .routes(routes!(todos::list_assignees))
        .routes(routes!(todos::list_todos, todos::create_todo))
        .routes(routes!(
            todos::get_todo,
            todos::put_todo,
            todos::delete_todo
        ))
        .routes(routes!(todos::move_todo))
        .routes(routes!(todos::add_comment))
        .routes(routes!(
            documents::list_documents,
            documents::create_document
        ))
        .routes(routes!(
            documents::get_document,
            documents::put_document,
            documents::delete_document
        ))
        .routes(routes!(documents::issue_document))
        .routes(routes!(documents::document_pdf))
        .routes(routes!(documents::document_xml))
        .routes(routes!(sending::send_document))
        .routes(routes!(overview::add_payment))
        .routes(routes!(overview::reverse_payment))
        .routes(routes!(overview::get_overview))
        .routes(routes!(documents::duplicate_document))
        .routes(routes!(documents::follow_up))
        .routes(routes!(documents::set_outcome))
        .routes(routes!(billing::add_time))
        .routes(routes!(billing::list_unbilled))
        .routes(routes!(books::get_books, books::start_books))
        .routes(routes!(books::list_accounts, books::create_account))
        .routes(routes!(books::put_account))
        .routes(routes!(books::list_postings, books::create_posting))
        .routes(routes!(books::reverse_posting))
        .routes(routes!(
            suppliers::list_suppliers,
            suppliers::create_supplier
        ))
        .routes(routes!(suppliers::put_supplier))
        .merge(
            OpenApiRouter::new()
                .routes(routes!(receipts::list_receipts, receipts::create_receipt))
                .layer(DefaultBodyLimit::max(25 << 20)),
        )
        .routes(routes!(
            receipts::get_receipt,
            receipts::put_receipt,
            receipts::delete_receipt
        ))
        .routes(routes!(receipts::book_receipt))
        .routes(routes!(receipts::reverse_receipt))
        .routes(routes!(receipts::receipt_file))
        .routes(routes!(receipts::add_receipt_payment))
        .routes(routes!(receipts::reverse_receipt_payment))
        .routes(routes!(bank::list_bank_accounts, bank::create_bank_account))
        .routes(routes!(bank::put_bank_account))
        .merge(
            OpenApiRouter::new()
                .routes(routes!(bank::import_statement))
                .layer(DefaultBodyLimit::max(25 << 20)),
        )
        .routes(routes!(bank::list_transactions))
        .routes(routes!(bank::get_transaction))
        .routes(routes!(bank::match_transaction))
        .routes(routes!(bank::unmatch_transaction))
        .routes(routes!(open_items::get_open_items))
        .routes(routes!(tax::get_vat_return))
        .routes(routes!(tax::get_euer))
        .routes(routes!(exports::datev_export))
        .routes(routes!(deadlines::list_deadlines))
        .routes(routes!(
            deadlines::get_feed,
            deadlines::renew_feed,
            deadlines::delete_feed
        ))
        .routes(routes!(deadlines::calendar))
        .routes(routes!(years::list_years))
        .routes(routes!(years::close_year))
        .routes(routes!(years::year_archive))
        .routes(routes!(tokens::list_tokens, tokens::create_token))
        .routes(routes!(tokens::revoke_token))
        .routes(routes!(snippets::list_snippets, snippets::create_snippet))
        .routes(routes!(snippets::put_snippet, snippets::delete_snippet))
        .merge(
            OpenApiRouter::new()
                .routes(routes!(layouts::list_layouts, layouts::create_layout))
                .routes(routes!(layouts::put_layout, layouts::delete_layout))
                // layouts come with their fonts and images
                .layer(DefaultBodyLimit::max(25 << 20)),
        )
        .routes(routes!(layouts::layout_sample))
        .routes(routes!(layouts::list_choices, layouts::put_choices))
        .routes(routes!(layouts::list_packages, layouts::approve_package))
        .routes(routes!(time::list_time, time::create_entry))
        .routes(routes!(time::put_entry, time::delete_entry))
        .routes(routes!(time::stop_timer))
        .routes(routes!(time::list_timers, time::start_timer))
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
    Json(
        crate::money::CURRENCIES
            .iter()
            .map(|(code, _)| *code)
            .collect(),
    )
}
