use crate::common;

use axum::{Router, http::StatusCode, routing::get};
use common::{json, start};
use compact_company::config::Config;
use serde_json::{Value, json};
use typst::{foundations::Bytes, text::Font};

const LAYOUT: &str = r#"
#import "/cc.typ"
#import "@preview/stamp:0.1.0": stamp
#let schema = 1
#let render(data) = {
  set text(font: "DejaVu Sans Mono", size: 8pt)
  stamp[CUSTOM]
  for line in cc.address(data.business) [#line \ ]
  for line in cc.address(data.customer) [#line \ ]
  [#data.number #cc.date(data, data.issue_date) \ ]
  for line in cc.legal(data) [#line \ ]
  if data.service_from != none [#cc.date(data, data.service_from) – #cc.date(data, data.service_to) \ ]
  for line in data.lines [#line.description #cc.number(data, line.quantity) \ ]
  if data.totals != none [#cc.amount(data, data.totals.net) #cc.amount(data, data.totals.gross) \ ]
  for group in data.vat [#cc.percent(data, group.rate) #cc.amount(data, group.tax) \ ]
  for d in data.deductions {
    [#d.number \ ]
    for group in d.vat [#cc.amount(data, group.net) #cc.amount(data, group.tax) \ ]
  }
  if data.totals != none and data.totals.due != none [#cc.amount(data, data.totals.due) \ ]
  cc.vat-note(data)
}
"#;

/// A package archive as the Typst registry serves it.
fn package() -> Vec<u8> {
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    for (path, content) in [
        (
            "typst.toml",
            "[package]\nname = \"stamp\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\"\n",
        ),
        (
            "lib.typ",
            "#let stamp(body) = box(stroke: 1pt, inset: 2pt, body)\n",
        ),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, content.as_bytes())
            .unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

/// A font of its own for the layout to upload.
fn font() -> Vec<u8> {
    typst_assets::fonts()
        .find(|data| {
            Font::new(Bytes::new(*data), 0).is_some_and(|f| f.info().family == "DejaVu Sans Mono")
        })
        .unwrap()
        .to_vec()
}

async fn registry() -> url::Url {
    let archive = package();
    let router = Router::new().route(
        "/preview/stamp-0.1.0.tar.gz",
        get(move || {
            let archive = archive.clone();
            async move { archive }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{address}/").parse().unwrap()
}

#[tokio::test]
async fn an_uploaded_layout_renders_one_customers_invoices_and_the_others_keep_the_default() {
    let app = start(Config {
        typst_packages: registry().await,
        ..Config::default()
    })
    .await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let other = app
        .ok(
            &mut owner,
            "POST",
            "/api/customers",
            json!({ "name": "Andere AG", "language": "en", "street": "Weg 1", "postcode": "10115",
                    "city": "Berlin" }),
        )
        .await["id"]
        .as_i64()
        .unwrap();
    let font = font();
    let upload = |source: &'static str| {
        vec![
            ("name", None, b"Hausdesign".as_slice()),
            ("kinds", None, b"invoice".as_slice()),
            ("source", Some("layout.typ"), source.as_bytes()),
        ]
    };
    let mut with_font = upload(LAYOUT);
    with_font.push(("files", Some("Mono.ttf"), &font));

    let unapproved = app
        .multipart(&mut owner, "POST", "/api/layouts", &with_font)
        .await;
    assert_eq!(unapproved.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let unapproved = json(unapproved).await;
    assert_eq!(unapproved["code"], "package_not_approved");
    assert_eq!(unapproved["detail"], "@preview/stamp:0.1.0");

    let package = app
        .ok(
            &mut owner,
            "POST",
            "/api/packages",
            json!({ "spec": "@preview/stamp:0.1.0" }),
        )
        .await;
    assert_eq!(package["approved_by"], "Samuel");

    let incomplete = app
        .multipart(
            &mut owner,
            "POST",
            "/api/layouts",
            &upload(
                "#import \"/cc.typ\"\n#let schema = 1\n#let render(data) = cc.address(data.customer).join(linebreak())",
            ),
        )
        .await;
    let incomplete = json(incomplete).await;
    assert_eq!(incomplete["code"], "layout_failed");
    let detail = incomplete["detail"].as_str().unwrap();
    assert!(
        detail.contains("invoice (de): does not show business.name"),
        "{detail}"
    );

    let layout = app
        .multipart(&mut owner, "POST", "/api/layouts", &with_font)
        .await;
    assert_eq!(layout.status(), StatusCode::OK);
    let layout = json(layout).await;
    assert_eq!(
        layout["files"],
        json!([{ "name": "Mono.ttf", "font": true }])
    );
    assert_eq!(layout["packages"], json!(["@preview/stamp:0.1.0"]));
    assert_eq!(layout["current"], true);

    let wrong_kind = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/layout-choices",
            json!([{ "kind": "quote", "layout_id": layout["id"] }]),
        )
        .await;
    assert_eq!(wrong_kind.status(), StatusCode::UNPROCESSABLE_ENTITY);
    app.ok(
        &mut owner,
        "PUT",
        "/api/layout-choices",
        json!([{ "kind": "invoice", "customer_id": customer, "layout_id": layout["id"] }]),
    )
    .await;

    let issue = async |owner: &mut common::Browser, customer: i64| -> Value {
        let draft = app
            .ok(
                owner,
                "POST",
                "/api/documents",
                json!({ "kind": "invoice", "customer_id": customer, "lines": [
                    { "description": "Beratung", "quantity": "1", "unit": "hour", "unit_price": 10000 }
                ] }),
            )
            .await;
        app.ok(
            owner,
            "POST",
            &format!("/api/documents/{}/issue", draft["id"]),
            json!({}),
        )
        .await
    };
    let custom = issue(&mut owner, customer).await;
    let default = issue(&mut owner, other).await;
    assert_eq!(custom["layout"], "Hausdesign");
    assert_eq!(default["layout"], Value::Null);

    let deleted = app
        .as_browser(
            &mut owner,
            "DELETE",
            &format!("/api/layouts/{}", layout["id"]),
            json!({}),
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let choices = app
        .ok(&mut owner, "GET", "/api/layout-choices", json!({}))
        .await;
    assert_eq!(choices, json!([]));
    let pdf = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/documents/{}/pdf", custom["id"]),
            json!({}),
        )
        .await;
    assert_eq!(
        pdf.status(),
        StatusCode::OK,
        "an issued document keeps its PDF"
    );
}
