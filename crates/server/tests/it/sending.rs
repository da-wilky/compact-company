use crate::common;

use axum::http::StatusCode;
use common::{json, smtp, smtp_config as config, start};
use compact_company::config::Config;
use serde_json::json;

#[tokio::test]
async fn an_issued_invoice_is_sent_to_the_customer_and_logged() {
    let (port, received) = smtp(false).await;
    let app = start(config(port)).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Beratung", "quantity": "1", "unit": "hour", "unit_price": 9000 }
            ] }),
        )
        .await;
    let path = format!("/api/documents/{}", draft["id"]);
    let early = app
        .as_browser(&mut owner, "POST", &format!("{path}/send"), json!({}))
        .await;
    assert_eq!(early.status(), StatusCode::CONFLICT, "drafts are not sent");
    let issued = app
        .ok(&mut owner, "POST", &format!("{path}/issue"), json!({}))
        .await;
    let number = issued["number"].as_str().unwrap();

    let sent = app
        .ok(&mut owner, "POST", &format!("{path}/send"), json!({}))
        .await;
    assert_eq!(sent["sent"][0]["recipient"], "info@krume.example");
    assert_eq!(
        sent["sent"][0]["subject"],
        format!("Rechnung {number}").as_str()
    );
    assert_eq!(sent["sent"][0]["error"], serde_json::Value::Null);
    let messages = received.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("To: info@krume.example"));
    assert!(messages[0].contains(&format!("filename=\"{number}.pdf\"")));
}

#[tokio::test]
async fn a_refused_mail_is_logged_with_the_servers_answer() {
    let (port, _) = smtp(true).await;
    let app = start(config(port)).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "quote", "customer_id": customer, "lines": [
                { "description": "Beratung", "quantity": "1", "unit": "hour", "unit_price": 9000 }
            ] }),
        )
        .await;
    let path = format!("/api/documents/{}", draft["id"]);
    app.ok(&mut owner, "POST", &format!("{path}/issue"), json!({}))
        .await;
    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("{path}/send"),
            json!({ "to": "wer@example.org", "subject": "Ihr Angebot" }),
        )
        .await;
    assert_eq!(refused.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(json(refused).await["code"], "mail_failed");
    let document = app.ok(&mut owner, "GET", &path, json!({})).await;
    assert_eq!(document["sent"][0]["recipient"], "wer@example.org");
    assert!(
        document["sent"][0]["error"]
            .as_str()
            .unwrap()
            .contains("no such mailbox")
    );
}

#[tokio::test]
async fn without_smtp_nothing_is_sent() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let response = app
        .as_browser(&mut owner, "POST", "/api/documents/1/send", json!({}))
        .await;
    assert_eq!(json(response).await["code"], "mail_not_configured");
}

#[tokio::test]
async fn a_document_to_a_billing_address_with_its_own_email_goes_there_as_issued() {
    let (port, received) = smtp(false).await;
    let app = start(config(port)).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let customer_path = format!("/api/customers/{customer}");
    let main = app.ok(&mut owner, "GET", &customer_path, json!({})).await["addresses"][0].clone();
    let with_office = |office_email: &str| {
        json!({ "name": "Bäckerei Krume", "language": "de", "email": "info@krume.example",
                "addresses": [
                    { "id": main["id"], "street": "Brotgasse 2", "postcode": "04109",
                      "city": "Leipzig" },
                    { "street": "c/o Steuerbüro Lind\nAm Ring 4", "postcode": "04103",
                      "city": "Leipzig", "email": office_email }
                ] })
    };
    let office = app
        .ok(
            &mut owner,
            "PUT",
            &customer_path,
            with_office("krume@lind.example"),
        )
        .await["addresses"][1]
        .clone();
    assert_eq!(office["email"], "krume@lind.example");
    let draft = |address: &serde_json::Value| {
        json!({ "kind": "invoice", "customer_id": customer, "address_id": address,
                "lines": [{ "description": "Beratung", "quantity": "1", "unit": "hour",
                            "unit_price": 9000 }] })
    };
    let plain = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            draft(&serde_json::Value::Null),
        )
        .await;
    assert_eq!(plain["data"]["customer"]["email"], "info@krume.example");
    let picked = app
        .ok(&mut owner, "POST", "/api/documents", draft(&office["id"]))
        .await;
    assert_eq!(picked["data"]["customer"]["email"], "krume@lind.example");
    let path = format!("/api/documents/{}", picked["id"]);
    app.ok(&mut owner, "POST", &format!("{path}/issue"), json!({}))
        .await;

    // the issued invoice keeps the email it was issued with
    let mut changed = with_office("neu@lind.example");
    changed["addresses"][1]["id"] = office["id"].clone();
    app.ok(&mut owner, "PUT", &customer_path, changed).await;
    let sent = app
        .ok(&mut owner, "POST", &format!("{path}/send"), json!({}))
        .await;
    assert_eq!(sent["sent"][0]["recipient"], "krume@lind.example");
    assert!(received.lock().unwrap()[0].contains("To: krume@lind.example"));

    let refused = app
        .as_browser(
            &mut owner,
            "PUT",
            &customer_path,
            with_office("kein-at-zeichen"),
        )
        .await;
    assert_eq!(
        json(refused).await["fields"]["addresses.1.email"],
        "invalid_format"
    );
}
