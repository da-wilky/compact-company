use axum::{
    Json, Router,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::Embed;
use serde_json::json;

#[derive(Embed)]
#[folder = "../../web/build"]
struct Client;

const CLIENT_FALLBACK: &str = "200.html";

pub fn router() -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .fallback(api_not_found);
    Router::new().nest("/api", api).fallback(client)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn api_not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response()
}

async fn client(uri: Uri) -> Response {
    let file = Client::get(uri.path().trim_start_matches('/'))
        .or_else(|| Client::get(CLIENT_FALLBACK))
        .expect("the client build contains its fallback page");
    (
        [(header::CONTENT_TYPE, file.metadata.mimetype())],
        file.data,
    )
        .into_response()
}
