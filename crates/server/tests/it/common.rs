use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use compact_company::{App, config::Config};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
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
                json!({ "token": self.app.setup_token, "name": "Erika" }),
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

impl TestApp {
    /// Sends a request that must succeed and returns its JSON body.
    pub async fn ok(&self, browser: &mut Browser, method: &str, path: &str, body: Value) -> Value {
        let response = self.as_browser(browser, method, path, body).await;
        let status = response.status();
        let body = text(response).await;
        assert_eq!(status, StatusCode::OK, "{method} {path}: {body}");
        serde_json::from_str(&body).unwrap()
    }

    /// Sends `parts` (name, file name, content) as multipart/form-data.
    pub async fn multipart(
        &self,
        browser: &mut Browser,
        method: &str,
        path: &str,
        parts: &[(&str, Option<&str>, &[u8])],
    ) -> Response {
        let boundary = "compact-company-test-boundary";
        let mut body = Vec::new();
        for (name, file_name, content) in parts {
            body.extend(
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"").bytes(),
            );
            if let Some(file_name) = file_name {
                body.extend(format!("; filename=\"{file_name}\"").bytes());
            }
            body.extend(b"\r\n\r\n");
            body.extend(*content);
            body.extend(b"\r\n");
        }
        body.extend(format!("--{boundary}--\r\n").bytes());
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header(
                "x-csrf-token",
                browser.csrf_token.clone().unwrap_or_default(),
            );
        let cookies: Vec<_> = browser
            .cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        request = request.header(header::COOKIE, cookies.join("; "));
        self.request(request.body(Body::from(body)).unwrap()).await
    }

    /// A business ready to issue documents, and a customer with an address.
    pub async fn ready_to_issue(&self, owner: &mut Browser) -> i64 {
        self.ok(
            owner,
            "PUT",
            "/api/business",
            json!({
                "name": "Beispiel UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
                "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
                "taxation": "small_business", "iban": "DE89370400440532013000"
            }),
        )
        .await;
        let customer = self
            .ok(
                owner,
                "POST",
                "/api/customers",
                json!({
                    "name": "Bäckerei Krume", "language": "de", "hourly_rate": 8000,
                    "street": "Brotgasse 2", "postcode": "04109", "city": "Leipzig",
                    "email": "info@krume.example"
                }),
            )
            .await;
        customer["id"].as_i64().unwrap()
    }
}

pub async fn text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub async fn json(response: Response) -> Value {
    serde_json::from_str(&text(response).await).unwrap()
}

/// A mail server that accepts every message, or refuses every recipient.
pub async fn smtp(refuse: bool) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::new()));
    let store = received.clone();
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let store = store.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                write.write_all(b"220 test ESMTP\r\n").await.unwrap();
                let mut message = String::new();
                let mut in_data = false;
                while let Ok(Some(line)) = lines.next_line().await {
                    if in_data {
                        if line == "." {
                            in_data = false;
                            store.lock().unwrap().push(std::mem::take(&mut message));
                            write.write_all(b"250 queued\r\n").await.unwrap();
                        } else {
                            message.push_str(&line);
                            message.push('\n');
                        }
                        continue;
                    }
                    let reply: &[u8] = match line.get(..4).unwrap_or_default() {
                        "EHLO" => b"250-test\r\n250 8BITMIME\r\n",
                        "RCPT" if refuse => b"550 no such mailbox\r\n",
                        "DATA" => {
                            in_data = true;
                            b"354 go ahead\r\n"
                        }
                        "QUIT" => b"221 bye\r\n",
                        _ => b"250 ok\r\n",
                    };
                    write.write_all(reply).await.unwrap();
                }
            });
        }
    });
    (port, received)
}

pub fn smtp_config(port: u16) -> Config {
    toml::from_str(&format!(
        "[smtp]\nhost = \"127.0.0.1\"\nport = {port}\nsecurity = \"none\"\nfrom = \"Beispiel UG <rechnung@beispiel.example>\""
    ))
    .unwrap()
}

/// A WebDAV server's files and folders by decoded path, as a mirror sees them.
pub struct Dav {
    pub files: Mutex<BTreeMap<String, Vec<u8>>>,
    pub folders: Mutex<std::collections::BTreeSet<String>>,
    pub down: std::sync::atomic::AtomicBool,
}

/// Serves WebDAV's PUT, MKCOL, DELETE and PROPFIND like Nextcloud: a file's folder must exist
/// first, deleting a folder deletes what is in it, and a listing names the folder itself too.
pub async fn webdav() -> (String, Arc<Dav>) {
    use axum::{extract::Request, response::IntoResponse};
    use std::sync::atomic::Ordering;

    const ROOT: &str = "/remote.php/dav/files/erika/Firma";
    let dav = Arc::new(Dav {
        files: Mutex::default(),
        folders: Mutex::new([String::new()].into()),
        down: Default::default(),
    });
    let state = dav.clone();
    let handler = move |request: Request| {
        let dav = state.clone();
        async move {
            if dav.down.load(Ordering::SeqCst) {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            let raw = request.uri().path().to_owned();
            let Some(path) = raw.strip_prefix(ROOT) else {
                return StatusCode::NOT_FOUND.into_response();
            };
            let path = decode(path.trim_matches('/'));
            let parent = path.rsplit_once('/').map_or("", |(p, _)| p).to_owned();
            let method = request.method().as_str().to_owned();
            let body = request.into_body().collect().await.unwrap().to_bytes();
            let mut dirs = dav.folders.lock().unwrap();
            let mut files = dav.files.lock().unwrap();
            match method.as_str() {
                "PUT" if !dirs.contains(&parent) => StatusCode::CONFLICT.into_response(),
                "PUT" => {
                    files.insert(path, body.to_vec());
                    StatusCode::CREATED.into_response()
                }
                "MKCOL" if !dirs.contains(&parent) => StatusCode::CONFLICT.into_response(),
                "MKCOL" if dirs.contains(&path) => StatusCode::METHOD_NOT_ALLOWED.into_response(),
                "MKCOL" => {
                    dirs.insert(path);
                    StatusCode::CREATED.into_response()
                }
                "DELETE" if dirs.remove(&path) => {
                    let inside = |p: &String| p.starts_with(&format!("{path}/"));
                    dirs.retain(|d| !inside(d));
                    files.retain(|f, _| !inside(f));
                    StatusCode::NO_CONTENT.into_response()
                }
                "DELETE" => match files.remove(&path) {
                    Some(_) => StatusCode::NO_CONTENT.into_response(),
                    None => StatusCode::NOT_FOUND.into_response(),
                },
                "PROPFIND" if !dirs.contains(&path) => StatusCode::NOT_FOUND.into_response(),
                "PROPFIND" => {
                    let child = |p: &String| {
                        let parent = p.rsplit_once('/').map_or("", |(parent, _)| parent);
                        !p.is_empty() && parent == path
                    };
                    let entry = |p: &str, size: Option<usize>| {
                        let href = format!("{ROOT}/{p}");
                        let (href, kind, size) = match size {
                            Some(size) => (
                                href,
                                String::new(),
                                format!("<d:getcontentlength>{size}</d:getcontentlength>"),
                            ),
                            None if p.is_empty() => {
                                (href, "<d:collection/>".to_owned(), String::new())
                            }
                            None => (
                                format!("{href}/"),
                                "<d:collection/>".to_owned(),
                                String::new(),
                            ),
                        };
                        format!(
                            "<d:response><d:href>{href}</d:href><d:propstat><d:prop>\
                             <d:getlastmodified>Sat, 14 Nov 2026 12:00:00 GMT</d:getlastmodified>\
                             {size}<d:resourcetype>{kind}</d:resourcetype></d:prop>\
                             <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
                        )
                    };
                    let mut xml =
                        String::from(r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">"#);
                    xml.push_str(&entry(&path, None));
                    for dir in dirs.iter().filter(|d| child(d)) {
                        xml.push_str(&entry(dir, None));
                    }
                    for (file, bytes) in files.iter().filter(|(f, _)| child(f)) {
                        xml.push_str(&entry(file, Some(bytes.len())));
                    }
                    xml.push_str("</d:multistatus>");
                    (StatusCode::MULTI_STATUS, xml).into_response()
                }
                _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            }
        }
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}{ROOT}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, Router::new().fallback(handler))
            .await
            .unwrap()
    });
    (url, dav)
}

fn decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&path[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// A config mirroring to `url` with the test password.
pub fn mirror_config(url: &str) -> Config {
    let mut config: Config = toml::from_str(&format!(
        "[mirror.webdav]\nurl = \"{url}\"\nusername = \"erika\"\npassword_file = \"/dev/null\""
    ))
    .unwrap();
    config.mirror.webdav.as_mut().unwrap().password = "app-password".into();
    config
}

/// An S3-compatible server with an empty bucket `bucket`, storing its objects under the
/// returned directory as `<bucket>/<key>`.
pub async fn s3(bucket: &str) -> (String, tempfile::TempDir) {
    use axum::error_handling::HandleError;
    use s3s::{auth::SimpleAuth, service::S3ServiceBuilder};

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(bucket)).unwrap();
    let mut s3 = S3ServiceBuilder::new(s3s_fs::FileSystem::new(root.path()).unwrap());
    s3.set_auth(SimpleAuth::from_single("test-key", "test-secret"));
    let s3 = HandleError::new(s3.build(), |_| async { StatusCode::INTERNAL_SERVER_ERROR });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, Router::new().fallback_service(s3))
            .await
            .unwrap()
    });
    (endpoint, root)
}

/// Starts the books on 1 January of `first` and closes every year from it to `last`.
pub async fn close_years(app: &TestApp, owner: &mut Browser, first: i16, last: i16) {
    app.ok(
        owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr04", "start_date": format!("{first}-01-01") }),
    )
    .await;
    for year in first..=last {
        app.ok(
            owner,
            "POST",
            &format!("/api/years/{year}/close"),
            json!({}),
        )
        .await;
    }
}

pub async fn account(app: &TestApp, owner: &mut Browser, number: &str) -> Value {
    app.ok(owner, "GET", "/api/accounts", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == number)
        .unwrap()["id"]
        .clone()
}

pub async fn receipt(
    app: &TestApp,
    owner: &mut Browser,
    file: &[u8],
    content: Value,
    paid_from: &Value,
) -> i64 {
    let response = app
        .multipart(
            owner,
            "POST",
            "/api/receipts",
            &[("file", Some("scan.pdf"), file)],
        )
        .await;
    let id = json(response).await["id"].as_i64().unwrap();
    let path = format!("/api/receipts/{id}");
    app.ok(owner, "PUT", &path, content).await;
    app.ok(
        owner,
        "POST",
        &format!("{path}/book"),
        json!({ "paid_from": paid_from }),
    )
    .await;
    id
}

/// Moves an issued document into an earlier year, which issuing never does.
pub async fn backdate(app: &TestApp, id: i64, date: &str) {
    let db = &mut *app.app.db.acquire().await.unwrap();
    let trigger: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'document_issued_fixed'")
            .fetch_one(&mut *db)
            .await
            .unwrap();
    sqlx::query("DROP TRIGGER document_issued_fixed")
        .execute(&mut *db)
        .await
        .unwrap();
    sqlx::query("UPDATE document SET issue_date = ?1, created_at = unixepoch(?1) WHERE id = ?2")
        .bind(date)
        .bind(id)
        .execute(&mut *db)
        .await
        .unwrap();
    // the trigger as the migration wrote it
    sqlx::query(sqlx::AssertSqlSafe(trigger))
        .execute(&mut *db)
        .await
        .unwrap();
}
