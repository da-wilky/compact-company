use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    documents::{Document, business, find, row},
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, now},
    documents::Status,
    mail::{self, File},
};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SendInput {
    /// The customer's email when left out.
    to: Option<String>,
    /// `<kind> <number>` when left out.
    subject: Option<String>,
    /// A short greeting in the document's language when left out.
    body: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Sent {
    at: i64,
    user_name: String,
    recipient: String,
    subject: String,
    /// What the mail server answered when it refused; none once sent.
    error: Option<String>,
}

/// Emails an issued document as PDF, with the XRechnung XML when the customer gets one; every
/// attempt is logged.
#[utoipa::path(post, path = "/documents/{id}/send", tag = "documents",
    params(("id" = i64, Path)), request_body = SendInput,
    responses((status = OK, body = Document), (status = "default", body = ApiError)))]
pub async fn send_document(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<SendInput>,
) -> Result<Json<Document>, ApiError> {
    let Some(mailer) = &app.mailer else {
        return Err(ApiError::new(ErrorCode::MailNotConfigured));
    };
    let business = business(&app).await?;
    let mut db = app.db.acquire().await?;
    let row = row(&mut db, id).await?;
    let (Status::Issued, Some(number), Some(pdf)) = (row.status, &row.number, &row.pdf) else {
        return Err(ApiError::new(ErrorCode::FollowUpNotAllowed));
    };
    let customer_email: Option<String> =
        sqlx::query_scalar("SELECT email FROM customer WHERE id = ?")
            .bind(row.customer_id)
            .fetch_one(&mut *db)
            .await?;
    drop(db);
    let (subject, body) = mail::default_text(
        row.kind,
        number,
        row.language,
        business.name.as_deref().unwrap_or_default(),
    );
    let mut check = Check::default();
    let to = input.to.or(customer_email);
    let to = check.required("to", to, 254);
    if !to.contains('@') || to.contains(char::is_whitespace) {
        check.fail("to", FieldError::InvalidFormat);
    }
    let subject = check.text("subject", input.subject, 200).unwrap_or(subject);
    let body = check.text("body", input.body, 20_000).unwrap_or(body);
    check.done()?;

    let get = async |hash: &str| -> Result<Vec<u8>, ApiError> {
        Ok(app
            .files
            .get(hash.parse().map_err(ApiError::internal)?)
            .await
            .map_err(ApiError::internal)?
            .to_vec())
    };
    let mut files = vec![File {
        name: format!("{number}.pdf"),
        content_type: "application/pdf",
        bytes: get(pdf).await?,
    }];
    if let Some(xml) = &row.xml {
        files.push(File {
            name: format!("{number}.xml"),
            content_type: "application/xml",
            bytes: get(xml).await?,
        });
    }
    let error = mailer.send(&to, &subject, &body, files).await.err();

    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        "INSERT INTO document_send (document_id, at, user_id, recipient, subject, error)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(now())
    .bind(auth.user.id)
    .bind(&to)
    .bind(&subject)
    .bind(&error)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "document.send",
        &format!("document:{id}"),
        None,
        Some(serde_json::json!({ "to": to, "subject": subject, "error": error })),
    )
    .await?;
    let document = find(&mut tx, &business, id).await?;
    tx.commit().await?;
    match error {
        None => Ok(Json(document)),
        Some(error) => Err(ApiError::new(ErrorCode::MailFailed).with_detail(error)),
    }
}

pub async fn sent(db: &mut sqlx::SqliteConnection, id: i64) -> Result<Vec<Sent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.at, u.name AS user_name, s.recipient, s.subject, s.error
         FROM document_send s JOIN user u ON u.id = s.user_id
         WHERE s.document_id = ? ORDER BY s.at DESC, s.id DESC",
    )
    .bind(id)
    .fetch_all(db)
    .await
}
