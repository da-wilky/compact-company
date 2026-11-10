use std::collections::BTreeMap;

use axum::{
    extract::{
        FromRequest, FromRequestParts,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

/// The one error body every endpoint returns; `message` and each field error are catalog keys.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    #[serde(skip)]
    status: StatusCode,
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, FieldError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    MalformedRequest,
    InvalidInput,
    Unauthenticated,
    Forbidden,
    CsrfToken,
    SetupDone,
    InvalidSetupToken,
    CeremonyExpired,
    PasskeyRejected,
    LastPasskey,
    LastOwner,
    InvitationInvalid,
    OidcFailed,
    OidcUnknownIdentity,
    OidcIdentityTaken,
    Archived,
    StatusInUse,
    HasTime,
    RateLimited,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldError {
    Required,
    TooLong,
    InvalidFormat,
    Pinned,
    Locked,
}

impl ErrorCode {
    pub const ALL: &[Self] = &[
        Self::NotFound,
        Self::MalformedRequest,
        Self::InvalidInput,
        Self::Unauthenticated,
        Self::Forbidden,
        Self::CsrfToken,
        Self::SetupDone,
        Self::InvalidSetupToken,
        Self::CeremonyExpired,
        Self::PasskeyRejected,
        Self::LastPasskey,
        Self::LastOwner,
        Self::InvitationInvalid,
        Self::OidcFailed,
        Self::OidcUnknownIdentity,
        Self::OidcIdentityTaken,
        Self::Archived,
        Self::StatusInUse,
        Self::HasTime,
        Self::RateLimited,
        Self::Internal,
    ];

    fn status(self) -> StatusCode {
        match self {
            Self::NotFound | Self::InvitationInvalid => StatusCode::NOT_FOUND,
            Self::MalformedRequest
            | Self::CeremonyExpired
            | Self::PasskeyRejected
            | Self::OidcFailed => StatusCode::BAD_REQUEST,
            Self::InvalidInput => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden
            | Self::CsrfToken
            | Self::InvalidSetupToken
            | Self::OidcUnknownIdentity => StatusCode::FORBIDDEN,
            Self::SetupDone
            | Self::LastPasskey
            | Self::LastOwner
            | Self::OidcIdentityTaken
            | Self::Archived
            | Self::StatusInUse
            | Self::HasTime => StatusCode::CONFLICT,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn key(self) -> String {
        format!("error.{}", snake_case(self))
    }
}

impl FieldError {
    pub const ALL: &[Self] = &[
        Self::Required,
        Self::TooLong,
        Self::InvalidFormat,
        Self::Pinned,
        Self::Locked,
    ];

    pub fn key(self) -> String {
        format!("field.{}", snake_case(self))
    }
}

fn snake_case(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("unit variants serialize to strings")
}

impl ApiError {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            status: code.status(),
            code,
            message: code.key(),
            fields: BTreeMap::new(),
        }
    }

    pub fn fields(fields: BTreeMap<String, FieldError>) -> Self {
        Self {
            fields,
            ..Self::new(ErrorCode::InvalidInput)
        }
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        eprintln!("internal error: {error}");
        Self::new(ErrorCode::Internal)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, axum::Json(&self)).into_response()
    }
}

impl From<JsonRejection> for ApiError {
    fn from(_: JsonRejection) -> Self {
        Self::new(ErrorCode::MalformedRequest)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

/// `axum::Json` whose rejection uses the API's error format.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct Json<T>(pub T);

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

impl From<QueryRejection> for ApiError {
    fn from(_: QueryRejection) -> Self {
        Self::new(ErrorCode::MalformedRequest)
    }
}

impl From<PathRejection> for ApiError {
    fn from(_: PathRejection) -> Self {
        Self::new(ErrorCode::NotFound)
    }
}

/// `axum::extract::Query` whose rejection uses the API's error format.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct Query<T>(pub T);

/// `axum::extract::Path` whose rejection uses the API's error format.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct Path<T>(pub T);
