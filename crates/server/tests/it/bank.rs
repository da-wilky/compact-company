use crate::common;

use std::collections::BTreeMap;

use axum::http::StatusCode;
use common::{Browser, TestApp, json as body, start};
use compact_company::config::Config;
use serde_json::{Value, json};

const UBL: &[u8] = include_bytes!("../fixtures/xrechnung-ubl.xml");

async fn upload(app: &TestApp, owner: &mut Browser, path: &str, name: &str, bytes: &[u8]) -> Value {
    let response = app
        .multipart(owner, "POST", path, &[("file", Some(name), bytes)])
        .await;
    let status = response.status();
    let value = body(response).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}

async fn balances(app: &TestApp, owner: &mut Browser) -> BTreeMap<String, i64> {
    let accounts = app.ok(owner, "GET", "/api/accounts", json!({})).await;
    let balances: BTreeMap<String, i64> = accounts
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["number"].as_str().unwrap().to_owned(),
                a["balance"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(balances.values().sum::<i64>(), 0, "the journal balances");
    balances
}

async fn account(app: &TestApp, owner: &mut Browser, number: &str) -> i64 {
    app.ok(owner, "GET", "/api/accounts", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == number)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

fn camt(invoice: &str, date: &str) -> String {
    let entry = |amount: &str, direction: &str, status: &str, party: &str, purpose: &str| {
        format!(
            "<Ntry><Amt Ccy=\"EUR\">{amount}</Amt><CdtDbtInd>{direction}</CdtDbtInd>
             <Sts><Cd>{status}</Cd></Sts><BookgDt><Dt>{date}</Dt></BookgDt>
             <ValDt><Dt>{date}</Dt></ValDt><AcctSvcrRef>{amount}{direction}</AcctSvcrRef>
             <NtryDtls><TxDtls><Refs><EndToEndId>NOTPROVIDED</EndToEndId></Refs>
             <RltdPties>{party}</RltdPties><RmtInf><Ustrd>{purpose}</Ustrd></RmtInf>
             </TxDtls></NtryDtls></Ntry>"
        )
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.053.001.08\"><BkToCstmrStmt>
<GrpHdr><MsgId>1</MsgId></GrpHdr><Stmt><Id>1</Id>
<Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>{}{}{}{}</Stmt></BkToCstmrStmt></Document>",
        entry(
            "119.00",
            "CRDT",
            "BOOK",
            "<Dbtr><Nm>Baeckerei Krume</Nm></Dbtr><DbtrAcct><Id><IBAN>DE44500105175407324931</IBAN></Id></DbtrAcct>",
            &format!("Rechnung {invoice} vielen Dank")
        ),
        entry(
            "140.40",
            "DBIT",
            "BOOK",
            "<Cdtr><Nm>Hosting und Mehr AG</Nm></Cdtr><CdtrAcct><Id><IBAN>DE02120300000000202051</IBAN></Id></CdtrAcct>",
            "HA-2026-0815"
        ),
        entry("4.90", "DBIT", "BOOK", "", "Kontofuehrung"),
        entry("999.00", "CRDT", "PDNG", "", "vorgemerkt"),
    )
}

#[tokio::test]
async fn a_statement_is_imported_and_its_transactions_matched_from_suggestions() {
    let app = start(Config::default()).await;
    let mut owner = app.setup_owner().await;
    let customer = app.ready_to_issue(&mut owner).await;
    app.ok(
        &mut owner,
        "PUT",
        "/api/business",
        json!({
            "name": "swilk UG (haftungsbeschränkt)", "street": "Hauptstraße 1",
            "postcode": "01067", "city": "Dresden", "tax_number": "201/123/45678",
            "taxation": "accrual", "iban": "DE89370400440532013000"
        }),
    )
    .await;
    app.ok(
        &mut owner,
        "POST",
        "/api/books",
        json!({ "chart": "skr03", "start_date": "2026-01-01" }),
    )
    .await;
    let draft = app
        .ok(
            &mut owner,
            "POST",
            "/api/documents",
            json!({ "kind": "invoice", "customer_id": customer, "lines": [
                { "description": "Wartung", "quantity": "1", "unit": "piece", "unit_price": 10000 }
            ] }),
        )
        .await;
    let invoice = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/documents/{}/issue", draft["id"]),
            json!({}),
        )
        .await;
    let receipt = upload(&app, &mut owner, "/api/receipts", "x.xml", UBL).await;
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/receipts/{}/book", receipt["id"]),
        json!({}),
    )
    .await;

    let ledger = account(&app, &mut owner, "1200").await;
    let girokonto = app
        .ok(
            &mut owner,
            "POST",
            "/api/bank-accounts",
            json!({ "name": "Girokonto", "iban": "DE89 3704 0044 0532 0130 00",
                    "ledger_account_id": ledger }),
        )
        .await;
    let import = format!("/api/bank-accounts/{}/import", girokonto["id"]);
    let date = invoice["issue_date"].as_str().unwrap();
    let statement = camt(invoice["number"].as_str().unwrap(), date);
    let first = upload(&app, &mut owner, &import, "camt.xml", statement.as_bytes()).await;
    assert_eq!(
        first,
        json!({ "format": "camt053", "imported": 3, "skipped": 0 })
    );
    let again = upload(&app, &mut owner, &import, "camt.xml", statement.as_bytes()).await;
    assert_eq!(
        again["imported"], 0,
        "an overlapping statement adds nothing"
    );
    assert_eq!(again["skipped"], 3);
    let wrong = app
        .multipart(
            &mut owner,
            "POST",
            &import,
            &[(
                "file",
                Some("camt.xml"),
                statement
                    .replace("DE89370400440532013000", "DE02120300000000202051")
                    .as_bytes(),
            )],
        )
        .await;
    assert_eq!(body(wrong).await["fields"]["file"], "invalid_format");

    let open = app
        .ok(
            &mut owner,
            "GET",
            "/api/bank-transactions?open=true",
            json!({}),
        )
        .await;
    let open = open.as_array().unwrap();
    assert_eq!(open.len(), 3);
    let by_amount = |amount: i64| {
        open.iter().find(|t| t["amount"] == amount).unwrap()["id"]
            .as_i64()
            .unwrap()
    };
    let (income, hosting, fee) = (by_amount(11_900), by_amount(-14_040), by_amount(-490));
    let transaction = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/bank-transactions/{income}"),
            json!({}),
        )
        .await;
    assert_eq!(transaction["counterparty"], "Baeckerei Krume");
    assert_eq!(
        transaction["purpose"],
        format!(
            "Rechnung {} vielen Dank",
            invoice["number"].as_str().unwrap()
        )
    );
    let best = &transaction["candidates"][0];
    assert_eq!(best["kind"], "invoice");
    assert_eq!(best["id"], invoice["id"]);
    assert!(best["score"].as_u64().unwrap() >= 80, "{best}");
    let matched = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/bank-transactions/{income}/match"),
            json!({ "parts": [{ "kind": "invoice", "id": best["id"], "amount": best["amount"] }] }),
        )
        .await;
    assert_eq!(matched["parts"][0]["label"], invoice["number"]);
    let paid = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/documents/{}", invoice["id"]),
            json!({}),
        )
        .await;
    assert_eq!(paid["amount_due"]["minor"], 0);

    let transaction = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/bank-transactions/{hosting}"),
            json!({}),
        )
        .await;
    let best = &transaction["candidates"][0];
    assert_eq!(best["kind"], "receipt");
    assert_eq!(best["amount"], -14_040);
    assert!(
        best["score"].as_u64().unwrap() >= 110,
        "number, amount and IBAN: {best}"
    );
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/bank-transactions/{hosting}/match"),
        json!({ "parts": [{ "kind": "receipt", "id": best["id"], "amount": -14_040 }] }),
    )
    .await;

    let fees = account(&app, &mut owner, "4970").await;
    let unbalanced = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/bank-transactions/{fee}/match"),
            json!({ "parts": [{ "kind": "account", "id": fees, "amount": -400 }] }),
        )
        .await;
    assert_eq!(body(unbalanced).await["fields"]["parts"], "invalid_format");
    app.ok(
        &mut owner,
        "POST",
        &format!("/api/bank-transactions/{fee}/match"),
        json!({ "parts": [{ "kind": "account", "id": fees, "amount": -490 }] }),
    )
    .await;

    let totals = balances(&app, &mut owner).await;
    assert_eq!(totals["1400"], 0);
    assert_eq!(totals["1600"], 0);
    assert_eq!(totals["4970"], 490);
    assert_eq!(totals["1200"], 11_900 - 14_040 - 490);
    let open = app
        .ok(
            &mut owner,
            "GET",
            "/api/bank-transactions?open=true",
            json!({}),
        )
        .await;
    assert_eq!(open, json!([]));

    let payment = paid["payments"][0]["id"].clone();
    let refused = app
        .as_browser(
            &mut owner,
            "POST",
            &format!("/api/payments/{payment}/reverse"),
            json!({}),
        )
        .await;
    assert_eq!(body(refused).await["code"], "bank_matched");
    let undone = app
        .ok(
            &mut owner,
            "POST",
            &format!("/api/bank-transactions/{income}/unmatch"),
            json!({}),
        )
        .await;
    assert_eq!(undone["match_id"], Value::Null);
    assert_eq!(undone["candidates"][0]["id"], invoice["id"]);
    let totals = balances(&app, &mut owner).await;
    assert_eq!(totals["1400"], 11_900, "the invoice is open again");

    // a CSV export in Windows-1252 with the bank's lines above the headings
    app.ok(
        &mut owner,
        "POST",
        "/api/accounts",
        json!({ "number": "1210", "name": "Tagesgeld", "type": "asset" }),
    )
    .await;
    let ledger = account(&app, &mut owner, "1210").await;
    let savings = app
        .ok(
            &mut owner,
            "POST",
            "/api/bank-accounts",
            json!({ "name": "Tagesgeld", "ledger_account_id": ledger }),
        )
        .await;
    let csv = "\"Umsätze Tagesgeld\";\"Zeitraum: 01.11.2026 - 30.11.2026\"\r\n\r\n\
        \"Buchungstag\";\"Valutadatum\";\"Name Zahlungsbeteiligter\";\"IBAN Zahlungsbeteiligter\";\"Verwendungszweck\";\"Betrag\";\"Waehrung\"\r\n\
        \"03.11.2026\";\"03.11.2026\";\"Bäckerei Krume\";\"DE44 5001 0517 5407 3249 31\";\"Zinsen; Teil 1\";\"1.190,00\";\"EUR\"\r\n\
        \"04.11.26\";\"04.11.26\";\"Finanzamt\";\"\";\"USt 10/2026\";\"-50,00\";\"EUR\"\r\n";
    let latin1: Vec<u8> = csv
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap())
        .collect();
    let result = upload(
        &app,
        &mut owner,
        &format!("/api/bank-accounts/{}/import", savings["id"]),
        "umsaetze.csv",
        &latin1,
    )
    .await;
    assert_eq!(
        result,
        json!({ "format": "csv", "imported": 2, "skipped": 0 })
    );
    let rows = app
        .ok(
            &mut owner,
            "GET",
            &format!("/api/bank-transactions?bank_account_id={}", savings["id"]),
            json!({}),
        )
        .await;
    assert_eq!(rows[0]["booking_date"], "2026-11-04");
    assert_eq!(rows[0]["amount"], -5_000);
    assert_eq!(rows[1]["counterparty"], "Bäckerei Krume");
    assert_eq!(rows[1]["counterparty_iban"], "DE44500105175407324931");
    assert_eq!(rows[1]["purpose"], "Zinsen; Teil 1");
    assert_eq!(rows[1]["amount"], 119_000);
}
