#![allow(dead_code)]

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Request, header},
    response::Response,
};
use compact_company::{App, config::Config};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

pub struct TestApp {
    pub dir: tempfile::TempDir,
    pub app: Arc<App>,
    pub router: Router,
}

pub async fn start(config: Config) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        data_dir: dir.path().join("data"),
        ..config
    };
    let app = Arc::new(App::open(&config).await.unwrap());
    let router = compact_company::router(app.clone());
    TestApp { dir, app, router }
}

impl TestApp {
    pub async fn request(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    pub async fn get(&self, path: &str) -> Response {
        self.request(Request::get(path).body(Body::empty()).unwrap())
            .await
    }

    pub async fn send_json(&self, method: &str, path: &str, body: Value) -> Response {
        self.request(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }
}

pub async fn text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub async fn json(response: Response) -> Value {
    serde_json::from_str(&text(response).await).unwrap()
}
