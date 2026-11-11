mod common;

use axum::http::StatusCode;
use common::{json, start};
use compact_company::config::Config;
use serde_json::{Value, json};

fn year(document: &Value) -> String {
    document["issue_date"].as_str().unwrap()[..4].to_owned()
}

#[tokio::test]
async fn an_issued_document_keeps_its_data_amounts_and_number() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    let project = app
        .ok(
            &mut owner,
            "POST",
            "/api/projects",
            json!({ "customer_id": customer, "name": "Website", "hourly_rate": 9000 }),
        )
        .await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({
                "kind": "invoice", "customer_id": customer, "project_id": project["id"],
                "lines": [
                    { "description": "Entwicklung", "quantity": "2.5", "unit": "hour",
                      "project_id": project["id"] },
                    { "description": "Hosting", "quantity": "1", "unit": "month", "unit_price": 1250 }
                ]
            }),
        )
        .await;
    assert_eq!(draft["status"], "draft");
    assert_eq!(draft["number"], Value::Null);
    assert_eq!(draft["data"]["lines"][0]["unit_price"], "90.00");
    assert_eq!(draft["data"]["totals"]["gross"], "237.50");
    assert_eq!(draft["missing"], json!({}));

    let id = draft["id"].as_i64().unwrap();
    let issued = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/issue"),
            json!({}),
        )
        .await;
    let number = format!("RE-{}-001", year(&issued));
    assert_eq!(issued["number"], number.as_str());
    assert_eq!(issued["data"]["number"], number.as_str());
    assert_eq!(issued["lines"][0]["unit_price"], 9000);
    assert_eq!(issued["data"]["vat"][0]["rate"], "0");

    app.ok(
        &mut owner,
        "PUT",
        &format!("/api/customers/{customer}"),
        json!({ "name": "Krume GmbH", "language": "de", "street": "Neuer Weg 9",
                "postcode": "04109", "city": "Leipzig" }),
    )
    .await;
    app.ok(
        &mut owner,
        "PUT",
        &format!("/api/projects/{}", project["id"]),
        json!({ "customer_id": customer, "name": "Website", "hourly_rate": 12000 }),
    )
    .await;
    app.ok(
        &mut owner,
        "PUT",
        "/api/business",
        json!({ "name": "swilk UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
                "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
                "small_business": true, "invoice_number_format": "R{yy}-{nnnn}" }),
    )
    .await;

    let later = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/documents/{id}"),
            json!({}),
        )
        .await;
    assert_eq!(later["number"], number.as_str());
    assert_eq!(later["data"], issued["data"]);
    assert_eq!(later["data"]["customer"]["street"], "Brotgasse 2");
    assert_eq!(later["data"]["lines"][0]["unit_price"], "90.00");
    assert_eq!(later["data"]["totals"]["gross"], "237.50");

    let second = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{id}/follow-up"),
            json!({ "kind": "cancellation" }),
        )
        .await;
    let second = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", second["id"]),
            json!({}),
        )
        .await;
    assert_eq!(
        second["number"],
        format!("R{}-0002", &year(&second)[2..]).as_str(),
        "a cancellation invoice draws from the invoice range, and the new format applies from the next number on"
    );
}

#[tokio::test]
async fn drafts_change_until_issued_and_only_the_owner_writes() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let mut staff = app.with_role(&mut owner, "staff").await;
    let mut accountant = app.with_role(&mut owner, "accountant").await;
    let customer = app
        .ok(
            &mut owner,
            "POST",
            "/api/customers",
            json!({ "name": "Ohne Anschrift", "language": "en" }),
        )
        .await;

    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "quote", "customer_id": customer["id"], "lines": [
                { "description": "Beratung", "quantity": "3", "unit": "hour" }
            ] }),
        )
        .await;
    assert_eq!(
        draft["language"], "en",
        "the customer's language by default"
    );
    assert_eq!(draft["missing"]["customer.city"], "required");
    assert_eq!(draft["missing"]["business.name"], "required");
    assert_eq!(draft["missing"]["lines.0.unit_price"], "required");
    let path = format!("/api/documents/{}", draft["id"]);
    let refused = app
        .as_browser(&mut owner, "POST", &format!("{path}/issue"), json!({}))
        .await;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let staff_read = app
        .as_browser(&mut staff, "GET", "/api/documents", json!({}))
        .await;
    assert_eq!(staff_read.status(), StatusCode::FORBIDDEN);
    let listed = app
        .ok(&mut accountant, "GET", "/api/documents", json!({}))
        .await;
    assert_eq!(listed[0]["kind"], "quote");
    let accountant_edit = app
        .as_browser(&mut accountant, "DELETE", &path, json!({}))
        .await;
    assert_eq!(accountant_edit.status(), StatusCode::FORBIDDEN);

    let deleted = app.as_browser(&mut owner, "DELETE", &path, json!({})).await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let customer = app.ready_to_issue(&mut owner).await;
    let quote = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "quote", "customer_id": customer, "lines": [
                { "description": "Beratung", "quantity": "3", "unit": "hour", "unit_price": 8000 }
            ] }),
        )
        .await;
    let path = format!("/api/documents/{}", quote["id"]);
    let issued = app
        .ok(&mut owner, "POST", &format!("{path}/issue"), json!({}))
        .await;
    assert_eq!(
        issued["number"],
        format!("AN-{}-001", year(&issued)).as_str(),
        "a deleted draft leaves no gap"
    );
    for (method, target) in [("DELETE", path.clone()), ("POST", format!("{path}/issue"))] {
        let response = app.as_browser(&mut owner, method, &target, json!({})).await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "{method} {target}");
    }
    let edit = app
        .as_browser(
            &mut owner,
            "PUT",
            &path,
            json!({ "kind": "quote", "customer_id": customer, "lines": [] }),
        )
        .await;
    assert_eq!(json(edit).await["code"], "issued");

    let log = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/audit?subject=document:{}", quote["id"]),
            json!({}),
        )
        .await;
    assert_eq!(log[0]["action"], "document.issue");
    assert_eq!(log[1]["action"], "document.create");
}

#[tokio::test]
async fn number_formats_need_one_counter() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let response = app
        .as_browser(
            &mut owner,
            "PUT",
            "/api/business",
            json!({ "quote_number_format": "AN-{yyyy}", "invoice_number_format": "{n}{n}",
                    "delivery_note_number_format": "LS {nn}", "iban": "DE00370400440532013000" }),
        )
        .await;
    let body = json(response).await;
    for key in [
        "quote_number_format",
        "invoice_number_format",
        "delivery_note_number_format",
        "iban",
    ] {
        assert_eq!(body["fields"][key], "invalid_format", "{key}");
    }
}
