use std::{collections::BTreeMap, sync::Arc};

use axum::{
    extract::{Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{
    customers::Language,
    error::{ApiError, ErrorCode, FieldError, Json, Path},
    input::field_error,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, OwnerOrAccountant, now},
    files::ContentHash,
    process::{self, Part, QUESTIONS},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ProcessQuestion {
    key: String,
    part: Part,
    question: String,
    help: String,
    suggestion: String,
    answer: Option<String>,
    updated_at: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProcessVersion {
    id: i64,
    valid_from: String,
    /// The day before the next version's; none for the current one.
    valid_until: Option<String>,
    created_at: i64,
    /// What differs from the version before, by label.
    changes: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProcessDocumentation {
    questions: Vec<ProcessQuestion>,
    /// Newest first.
    versions: Vec<ProcessVersion>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LanguageQuery {
    /// German when unset.
    language: Option<Language>,
}

/// Answers unanswered or unchanged for twelve months, which closing a year asks to review.
pub async fn stale(db: &mut sqlx::SqliteConnection) -> Result<Vec<String>, sqlx::Error> {
    let fresh: Vec<String> =
        sqlx::query_scalar("SELECT key FROM process_answer WHERE updated_at > ?")
            .bind(now() - 365 * 86_400)
            .fetch_all(db)
            .await?;
    Ok(QUESTIONS
        .iter()
        .filter(|q| !fresh.iter().any(|k| k == q.key))
        .map(|q| q.key.to_owned())
        .collect())
}

async fn view(app: &App, language: Language) -> Result<ProcessDocumentation, ApiError> {
    let mut db = app.db.acquire().await?;
    let answers: BTreeMap<String, (String, i64)> = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT key, answer, updated_at FROM process_answer",
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|(key, answer, at)| (key, (answer, at)))
    .collect();
    let questions = QUESTIONS
        .iter()
        .map(|q| ProcessQuestion {
            key: q.key.to_owned(),
            part: q.part,
            question: q.question.get(language).to_owned(),
            help: q.help.get(language).to_owned(),
            suggestion: q.suggestion.get(language).to_owned(),
            answer: answers.get(q.key).map(|a| a.0.clone()),
            updated_at: answers.get(q.key).map(|a| a.1),
        })
        .collect();
    let rows: Vec<(i64, String, Option<String>, i64, String)> = sqlx::query_as(
        "SELECT v.id, v.valid_from,
         (SELECT date(min(n.valid_from), '-1 day') FROM process_version n WHERE n.id > v.id),
         v.created_at, v.changes FROM process_version v ORDER BY v.id DESC",
    )
    .fetch_all(&mut *db)
    .await?;
    let versions = rows
        .into_iter()
        .map(|(id, valid_from, valid_until, created_at, changes)| {
            let changes: Vec<String> = serde_json::from_str(&changes).unwrap_or_default();
            ProcessVersion {
                id,
                valid_from,
                // two versions on one day: the earlier one never applied
                valid_until,
                created_at,
                changes: changes
                    .iter()
                    .map(|k| process::label(k, language))
                    .collect(),
            }
        })
        .collect();
    Ok(ProcessDocumentation {
        questions,
        versions,
    })
}

#[utoipa::path(get, path = "/process-documentation", tag = "storage", params(LanguageQuery),
    responses((status = OK, body = ProcessDocumentation), (status = "default", body = ApiError)))]
pub async fn get_process_documentation(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Query(query): Query<LanguageQuery>,
) -> Result<Json<ProcessDocumentation>, ApiError> {
    Ok(Json(
        view(&app, query.language.unwrap_or(Language::De)).await?,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AnswersInput {
    /// Answers by question key; an empty one removes the answer.
    answers: BTreeMap<String, String>,
}

/// Saves answers and makes a new version when one changed.
#[utoipa::path(put, path = "/process-documentation/answers", tag = "storage",
    params(LanguageQuery), request_body = AnswersInput,
    responses((status = OK, body = ProcessDocumentation), (status = "default", body = ApiError)))]
pub async fn put_answers(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Query(query): Query<LanguageQuery>,
    Json(input): Json<AnswersInput>,
) -> Result<Json<ProcessDocumentation>, ApiError> {
    for (key, answer) in &input.answers {
        if !QUESTIONS.iter().any(|q| q.key == key) {
            return Err(field_error(key, FieldError::InvalidFormat));
        }
        if answer.chars().count() > 5_000 {
            return Err(field_error(key, FieldError::TooLong));
        }
    }
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = process::answers(&mut tx).await?;
    for (key, answer) in &input.answers {
        let answer = answer.trim();
        if before.get(key).map(String::as_str).unwrap_or_default() == answer {
            continue;
        }
        if answer.is_empty() {
            sqlx::query("DELETE FROM process_answer WHERE key = ?")
                .bind(key)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query(
                "INSERT INTO process_answer (key, answer, updated_at, updated_by) VALUES (?, ?, ?, ?)
                 ON CONFLICT (key) DO UPDATE SET answer = excluded.answer,
                 updated_at = excluded.updated_at, updated_by = excluded.updated_by",
            )
            .bind(key)
            .bind(answer)
            .bind(now())
            .bind(auth.user.id)
            .execute(&mut *tx)
            .await?;
        }
        audit::record(
            &mut tx,
            auth.user.id,
            "process.answer",
            &format!("process_answer:{key}"),
            before.get(key).map(|a| serde_json::json!(a)),
            (!answer.is_empty()).then(|| serde_json::json!(answer)),
        )
        .await?;
    }
    tx.commit().await?;
    process::refresh(&app).await.map_err(ApiError::internal)?;
    Ok(Json(
        view(&app, query.language.unwrap_or(Language::De)).await?,
    ))
}

#[utoipa::path(get, path = "/process-documentation/versions/{id}/pdf", tag = "storage",
    params(("id" = i64, Path), LanguageQuery),
    responses((status = OK, content_type = "application/pdf", body = Vec<u8>),
        (status = "default", body = ApiError)))]
pub async fn process_pdf(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrAccountant>,
    Path(id): Path<i64>,
    Query(query): Query<LanguageQuery>,
) -> Result<Response, ApiError> {
    let language = query.language.unwrap_or(Language::De);
    let (de, en): (String, String) =
        sqlx::query_as("SELECT pdf_de, pdf_en FROM process_version WHERE id = ?")
            .bind(id)
            .fetch_optional(&app.db)
            .await?
            .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let hash = match language {
        Language::De => de,
        Language::En => en,
    };
    super::retention::kept(&mut *app.db.acquire().await?, &format!("process:{id}")).await?;
    let hash: ContentHash = hash.parse().map_err(ApiError::internal)?;
    let bytes = app.files.get(hash).await.map_err(ApiError::internal)?;
    let name = match language {
        Language::De => format!("Verfahrensdokumentation-v{id}.pdf"),
        Language::En => format!("process-documentation-v{id}.pdf"),
    };
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}
