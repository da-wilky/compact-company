#![allow(dead_code)]

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use compact_company::{App, config::Config};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use webauthn_authenticator_rs::{
    prelude::{CreationChallengeResponse, RequestChallengeResponse, Url, WebauthnAuthenticator},
    softpasskey::SoftPasskey,
};

pub struct TestApp {
    pub dir: tempfile::TempDir,
    pub app: Arc<App>,
    pub router: Router,
}

/// A browser: its session cookie and CSRF token, and an authenticator holding its passkeys.
pub struct Browser {
    pub cookies: BTreeMap<String, String>,
    pub csrf_token: Option<String>,
    pub authenticator: SoftPasskey,
    pub credential_ids: Vec<String>,
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

impl Browser {
    pub fn new() -> Self {
        Self {
            cookies: BTreeMap::new(),
            csrf_token: None,
            // the soft authenticator cannot verify a user, so it only claims to
            authenticator: SoftPasskey::new(true),
            credential_ids: Vec::new(),
        }
    }

    fn origin() -> Url {
        "http://localhost:9440".parse().unwrap()
    }

    /// Answers a registration challenge and returns the credential to send back.
    pub fn register(&mut self, options: &Value) -> Value {
        let options: CreationChallengeResponse = serde_json::from_value(options.clone()).unwrap();
        let credential = self
            .authenticator
            .do_registration(Self::origin(), options)
            .unwrap();
        self.credential_ids.push(credential.id.clone());
        serde_json::to_value(credential).unwrap()
    }

    /// Answers a sign-in challenge as a browser would after the user picked a passkey.
    pub fn authenticate(&mut self, options: &Value) -> Value {
        let mut options = options.clone();
        options["publicKey"]["allowCredentials"] = self
            .credential_ids
            .iter()
            .map(|id| json!({ "type": "public-key", "id": id }))
            .collect();
        let options: RequestChallengeResponse = serde_json::from_value(options).unwrap();
        let credential = self
            .authenticator
            .do_authentication(Self::origin(), options)
            .unwrap();
        serde_json::to_value(credential).unwrap()
    }
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
        self.send(None, method, path, body).await
    }

    /// Sends a request as `browser`, keeping any session cookie the response sets.
    pub async fn send(
        &self,
        browser: Option<&mut Browser>,
        method: &str,
        path: &str,
        body: Value,
    ) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(browser) = &browser {
            if !browser.cookies.is_empty() {
                let cookies: Vec<_> = browser
                    .cookies
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect();
                request = request.header(header::COOKIE, cookies.join("; "));
            }
            if let Some(csrf) = &browser.csrf_token {
                request = request.header("x-csrf-token", csrf);
            }
        }
        let response = self
            .request(request.body(Body::from(body.to_string())).unwrap())
            .await;
        if let Some(browser) = browser {
            for set_cookie in response.headers().get_all(header::SET_COOKIE) {
                let pair = set_cookie.to_str().unwrap().split(';').next().unwrap();
                let (name, value) = pair.split_once('=').unwrap();
                if value.is_empty() {
                    browser.cookies.remove(name);
                } else {
                    browser.cookies.insert(name.into(), value.into());
                }
            }
        }
        response
    }

    pub async fn as_browser(
        &self,
        browser: &mut Browser,
        method: &str,
        path: &str,
        body: Value,
    ) -> Response {
        self.send(Some(browser), method, path, body).await
    }

    /// Runs first-time setup and returns the owner's signed-in browser.
    pub async fn setup_owner(&self) -> Browser {
        let mut browser = Browser::new();
        let start = self
            .send_json(
                "POST",
                "/api/setup/start",
                json!({ "token": self.app.setup_token, "name": "Samuel" }),
            )
            .await;
        assert_eq!(start.status(), StatusCode::OK);
        let start = json(start).await;
        let credential = browser.register(&start["options"]);
        let finish = self
            .as_browser(
                &mut browser,
                "POST",
                "/api/setup/finish",
                json!({ "ceremony": start["ceremony"], "passkey_name": "Laptop", "credential": credential }),
            )
            .await;
        assert_eq!(finish.status(), StatusCode::OK);
        browser.csrf_token = json(finish).await["csrf_token"].as_str().map(str::to_owned);
        browser
    }

    pub async fn sign_in(&self, browser: &mut Browser) -> Response {
        let start = json(
            self.send_json("POST", "/api/sign-in/start", json!({}))
                .await,
        )
        .await;
        let credential = browser.authenticate(&start["options"]);
        let response = self
            .as_browser(
                browser,
                "POST",
                "/api/sign-in/finish",
                json!({ "ceremony": start["ceremony"], "credential": credential }),
            )
            .await;
        if response.status() == StatusCode::OK {
            let body = text(response).await;
            let view: Value = serde_json::from_str(&body).unwrap();
            browser.csrf_token = view["csrf_token"].as_str().map(str::to_owned);
            return Response::new(Body::from(body));
        }
        response
    }
}

impl TestApp {
    /// Has the owner invite someone and returns the token from the invitation link.
    pub async fn invite(&self, owner: &mut Browser, name: &str, role: &str) -> String {
        let response = self
            .as_browser(
                owner,
                "POST",
                "/api/invitations",
                json!({ "name": name, "role": role }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let link = json(response).await["link"].as_str().unwrap().to_owned();
        link.split_once("/invitation#").unwrap().1.to_owned()
    }

    pub async fn accept_with_passkey(&self, token: &str) -> Result<Browser, Box<Response>> {
        let mut browser = Browser::new();
        let start = self
            .send_json(
                "POST",
                "/api/invitation/passkey/start",
                json!({ "token": token }),
            )
            .await;
        if start.status() != StatusCode::OK {
            return Err(Box::new(start));
        }
        let start = json(start).await;
        let credential = browser.register(&start["options"]);
        let finish = self
            .as_browser(
                &mut browser,
                "POST",
                "/api/invitation/passkey/finish",
                json!({ "ceremony": start["ceremony"], "passkey_name": "Laptop", "credential": credential }),
            )
            .await;
        if finish.status() != StatusCode::OK {
            return Err(Box::new(finish));
        }
        browser.csrf_token = json(finish).await["csrf_token"].as_str().map(str::to_owned);
        Ok(browser)
    }

    /// Signs in a user with `role` through an invitation from a fresh owner.
    pub async fn with_role(&self, owner: &mut Browser, role: &str) -> Browser {
        let token = self.invite(owner, role, role).await;
        match self.accept_with_passkey(&token).await {
            Ok(browser) => browser,
            Err(response) => panic!("{}", text(*response).await),
        }
    }
}

pub async fn text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub async fn json(response: Response) -> Value {
    serde_json::from_str(&text(response).await).unwrap()
}
