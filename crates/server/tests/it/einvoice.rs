use crate::common;

use common::{json, start, text};
use compact_company::config::Config;
use http_body_util::BodyExt;
use serde_json::json;

#[tokio::test]
async fn an_issued_invoice_carries_zugferd_and_an_xrechnung_customer_gets_its_xml() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    app.ok(
        &mut owner,
        "PUT",
        &format!("/api/customers/{customer}"),
        json!({ "name": "Stadt Leipzig", "language": "de", "street": "Markt 1", "postcode": "04109",
                "city": "Leipzig", "email": "rechnung@leipzig.example", "leitweg_id": "991-12345-67",
                "xrechnung": true }),
    )
    .await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Beratung", "quantity": "2", "unit": "hour", "unit_price": 9000 }
            ] }),
        )
        .await;
    assert_eq!(draft["missing"]["business.email"], "required");
    assert_eq!(draft["missing"]["business.phone"], "required");
    app.ok(
        &mut owner,
        "PUT",
        "/api/business",
        json!({ "name": "swilk UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
                "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
                "taxation": "small_business", "iban": "DE89370400440532013000",
                "email": "post@swilk.example", "phone": "+49 351 123456" }),
    )
    .await;
    let id = draft["id"].as_i64().unwrap();
    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/issue"),
            json!({}),
        )
        .await;
    let number = issued["number"].as_str().unwrap();

    let pdf = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/documents/{id}/pdf"),
            json!({}),
        )
        .await
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let pdf = lopdf::Document::load_mem(&pdf).unwrap();
    let attached = pdf.objects.values().any(|object| {
        object.as_dict().is_ok_and(|dict| {
            dict.get(b"F")
                .and_then(lopdf::Object::as_str)
                .is_ok_and(|name| name == b"factur-x.xml")
                && dict
                    .get(b"AFRelationship")
                    .and_then(lopdf::Object::as_name)
                    .is_ok_and(|r| r == b"Alternative")
        })
    });
    assert!(
        attached,
        "factur-x.xml is attached as the alternative representation"
    );
    let metadata = pdf
        .catalog()
        .unwrap()
        .get(b"Metadata")
        .unwrap()
        .as_reference()
        .unwrap();
    let xmp = pdf
        .get_object(metadata)
        .unwrap()
        .as_stream()
        .unwrap()
        .get_plain_content()
        .unwrap();
    let xmp = String::from_utf8(xmp).unwrap();
    assert!(xmp.contains("<fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>"));
    assert!(
        xmp.contains("pdfaid:part"),
        "the PDF/A identification stays"
    );

    let xml = text(
        app.as_browser(
            &mut owner,
            "GET",
            &format!("/api/documents/{id}/xml"),
            json!({}),
        )
        .await,
    )
    .await;
    assert!(xml.contains("urn:xeinkauf.de:kosit:xrechnung_3.0"), "{xml}");
    assert!(xml.contains("<ram:BuyerReference>991-12345-67</ram:BuyerReference>"));
    assert!(xml.contains(&format!("<ram:ID>{number}</ram:ID>")));
    assert!(xml.contains("<ram:CategoryCode>E</ram:CategoryCode>"));

    let quote = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "quote", "customer_id": customer }),
        )
        .await;
    let none = app
        .as_browser(
            &mut owner,
            "GET",
            &format!("/api/documents/{}/xml", quote["id"]),
            json!({}),
        )
        .await;
    assert_eq!(json(none).await["code"], "not_found");
}
