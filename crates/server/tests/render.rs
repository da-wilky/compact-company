mod common;

use axum::http::{StatusCode, header};
use common::start;
use compact_company::{
    api::customers::Language,
    config::Config,
    documents::{Kind, sample},
    render::{self, Layout},
};
use http_body_util::BodyExt;
use serde_json::json;

const KINDS: [Kind; 6] = [
    Kind::Quote,
    Kind::OrderConfirmation,
    Kind::Invoice,
    Kind::Cancellation,
    Kind::DeliveryNote,
    Kind::AcceptanceReport,
];

#[test]
fn the_built_in_layout_shows_every_mandatory_detail_in_both_languages() {
    for kind in KINDS {
        for (language, total) in [(Language::De, "1.462,89"), (Language::En, "1,462.89")] {
            for small_business in [false, true] {
                let data = sample(kind, language, small_business);
                let rendered = render::render(&Layout::built_in(), &data, &[])
                    .unwrap_or_else(|e| panic!("{kind:?} {language:?}: {e}"));
                assert_eq!(
                    render::missing(&data, &rendered),
                    Vec::<String>::new(),
                    "{kind:?} {language:?} small business: {small_business}"
                );
                if kind == Kind::Invoice && !small_business {
                    assert!(rendered.text().contains(total), "{language:?}");
                }
                assert!(rendered.pdf().unwrap().starts_with(b"%PDF-"));
            }
        }
    }
}

#[test]
fn a_layout_that_leaves_out_a_mandatory_detail_is_caught() {
    let layout = Layout {
        source: r#"
            #import "/cc.typ"
            #let schema = 1
            #let render(data) = {
              cc.address(data.business).join(linebreak())
              cc.address(data.customer).join(linebreak())
              [#data.number #cc.date(data, data.issue_date)]
              for line in data.lines [#line.description #cc.number(data, line.quantity)]
              cc.amount(data, data.totals.net)
              cc.amount(data, data.totals.gross)
              cc.vat-note(data)
            }
        "#
        .to_owned(),
        ..Layout::default()
    };
    let data = sample(Kind::Invoice, Language::De, true);
    let rendered = render::render(&layout, &data, &[]).unwrap();
    let missing = render::missing(&data, &rendered);
    assert!(
        missing.contains(&"business.tax_number".to_owned()),
        "{missing:?}"
    );
    assert!(
        missing.contains(&"service_period".to_owned()),
        "{missing:?}"
    );
    assert!(
        missing.contains(&"business.register_court".to_owned()),
        "{missing:?}"
    );
    assert!(!missing.contains(&"totals.gross".to_owned()), "{missing:?}");
}

#[test]
fn a_layout_for_another_schema_does_not_render() {
    let layout = Layout {
        source: "#let schema = 99\n#let render(data) = []".to_owned(),
        ..Layout::default()
    };
    let error = render::render(&layout, &sample(Kind::Quote, Language::En, false), &[])
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("schema 99"), "{error}");
}

#[tokio::test]
async fn drafts_preview_live_and_issued_documents_keep_their_pdf() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "language": "en", "lines": [
                { "description": "Consulting", "quantity": "2", "unit": "hour", "unit_price": 9000 }
            ] }),
        )
        .await;
    let pdf = format!("/api/documents/{}/pdf", draft["id"]);
    let preview = app.as_browser(&mut owner, "GET", &pdf, json!({})).await;
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(preview.headers()[header::CONTENT_TYPE], "application/pdf");

    app.ok(
        &mut owner,
        "POST",
        &format!("/api/documents/{}/issue", draft["id"]),
        json!({}),
    )
    .await;
    let first = app.as_browser(&mut owner, "GET", &pdf, json!({})).await;
    assert!(
        first.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains("RE-")
    );
    let first = text_bytes(first).await;
    let second = text_bytes(app.as_browser(&mut owner, "GET", &pdf, json!({})).await).await;
    assert!(first.starts_with(b"%PDF-"));
    assert_eq!(
        first, second,
        "an issued document keeps the PDF it was issued with"
    );
}

async fn text_bytes(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}
