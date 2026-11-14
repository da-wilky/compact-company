use crate::common;

use std::net::{IpAddr, SocketAddr};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Request, StatusCode},
};
use common::{TestApp, json, start};
use compact_company::{config::Config, limit::Limiter};

async fn sign_in_start(app: &TestApp, peer: &str, forwarded: Option<&str>) -> StatusCode {
    let mut request =
        Request::post("/api/sign-in/start").header("content-type", "application/json");
    if let Some(forwarded) = forwarded {
        request = request.header("x-forwarded-for", forwarded);
    }
    let mut request = request.body(Body::from("{}")).unwrap();
    let peer: SocketAddr = format!("{peer}:50000").parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    app.request(request).await.status()
}

#[tokio::test]
async fn starts_without_a_session_are_limited_per_client() {
    let app = start(Config::default()).await;
    for _ in 0..20 {
        assert_eq!(
            sign_in_start(&app, "198.51.100.7", None).await,
            StatusCode::OK
        );
    }
    assert_eq!(
        sign_in_start(&app, "198.51.100.7", None).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // another client is not affected
    assert_eq!(
        sign_in_start(&app, "198.51.100.8", None).await,
        StatusCode::OK
    );

    let response = app
        .send_json(
            "POST",
            "/api/invitation",
            serde_json::json!({ "token": "x" }),
        )
        .await;
    assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    // an untrusted peer cannot claim to be someone else
    assert_eq!(
        sign_in_start(&app, "198.51.100.7", Some("203.0.113.9")).await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn behind_a_trusted_proxy_the_forwarded_client_counts() {
    let app = start(Config {
        trusted_proxies: vec!["10.0.0.1".parse().unwrap()],
        ..Config::default()
    })
    .await;
    for _ in 0..20 {
        assert_eq!(
            sign_in_start(&app, "10.0.0.1", Some("203.0.113.5")).await,
            StatusCode::OK
        );
    }
    let refused = Request::post("/api/sign-in/start")
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.5")
        .body(Body::from("{}"))
        .unwrap();
    let mut refused = refused;
    refused
        .extensions_mut()
        .insert(ConnectInfo("10.0.0.1:1".parse::<SocketAddr>().unwrap()));
    let response = app.request(refused).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(json(response).await["code"], "rate_limited");
    assert_eq!(
        sign_in_start(&app, "10.0.0.1", Some("203.0.113.6")).await,
        StatusCode::OK
    );
}

#[test]
fn the_client_is_the_last_hop_no_trusted_proxy_added() {
    let limiter = Limiter::new(vec![
        "10.0.0.1".parse().unwrap(),
        "10.0.0.2".parse().unwrap(),
    ]);
    let headers = |value: &str| {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", value.parse().unwrap());
        headers
    };
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    // the client may write anything on the left; only what the proxies saw counts
    assert_eq!(
        limiter.client(
            Some(ip("10.0.0.1")),
            &headers("1.1.1.1, 203.0.113.5, 10.0.0.2")
        ),
        ip("203.0.113.5")
    );
    assert_eq!(
        limiter.client(Some(ip("192.0.2.1")), &headers("203.0.113.5")),
        ip("192.0.2.1")
    );
    assert_eq!(
        limiter.client(Some(ip("10.0.0.1")), &headers("garbage")),
        ip("10.0.0.1")
    );
    assert_eq!(
        limiter.client(Some(ip("::ffff:10.0.0.1")), &headers("203.0.113.5")),
        ip("203.0.113.5")
    );
    assert_eq!(
        limiter.client(Some(ip("2001:db8:1:2:3:4:5:6")), &HeaderMap::new()),
        ip("2001:db8:1:2::")
    );
}
