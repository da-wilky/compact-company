use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(path: &str) -> Response {
    compact_company::router()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn content_type(response: &Response) -> &str {
    response.headers()[header::CONTENT_TYPE].to_str().unwrap()
}

#[tokio::test]
async fn health_reports_ok() {
    let response = get("/api/health").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text(response).await, r#"{"status":"ok"}"#);
}

#[tokio::test]
async fn unknown_api_path_is_a_json_404() {
    let response = get("/api/nope").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(content_type(&response), "application/json");
}

#[tokio::test]
async fn root_serves_the_client() {
    let response = get("/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(content_type(&response).starts_with("text/html"));
    assert!(text(response).await.contains("/_app/"));
}

#[tokio::test]
async fn client_routes_fall_back_to_the_client() {
    let response = get("/customers/42").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.contains("/_app/"));
}

#[tokio::test]
async fn client_assets_are_served_with_their_type() {
    let page = text(get("/").await).await;
    let start = page.find("/_app/immutable/").unwrap();
    let end = start + page[start..].find(".js").unwrap() + 3;
    let response = get(&page[start..end]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(content_type(&response), "text/javascript");
}
