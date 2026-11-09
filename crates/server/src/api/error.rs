use std::collections::BTreeMap;

use axum::{
    extract::{FromRequest, rejection::JsonRejection},
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
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldError {
    Required,
    TooLong,
    InvalidFormat,
    Pinned,
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
            Self::SetupDone | Self::LastPasskey | Self::LastOwner | Self::OidcIdentityTaken => {
                StatusCode::CONFLICT
            }
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
