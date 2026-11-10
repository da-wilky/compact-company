use std::sync::Arc;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::{IntoParams, ToSchema};

use super::{
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::Check,
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, now},
    money,
};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectInput {
    customer_id: i64,
    name: Option<String>,
    /// In the minor unit of the customer's currency; the customer's rate applies when left out.
    hourly_rate: Option<i64>,
    #[serde(default = "yes")]
    billable: bool,
    #[serde(default)]
    archived: bool,
}

fn yes() -> bool {
    true
}

/// What an edit may change; also the snapshot the audit log compares.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ProjectFields {
    name: String,
    /// In the minor unit of `currency`.
    hourly_rate: Option<i64>,
    /// Whether time on the project is billable unless an entry says otherwise.
    billable: bool,
    archived: bool,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Project {
    id: i64,
    customer_id: i64,
    customer_name: String,
    customer_number: String,
    /// The customer's currency.
    currency: String,
    /// Whether the project is closed to new work, itself or through its customer.
    closed: bool,
    #[sqlx(flatten)]
    #[serde(flatten)]
    fields: ProjectFields,
    created_at: i64,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ProjectFilter {
    customer_id: Option<i64>,
    /// Archived projects instead of active ones.
    #[serde(default)]
    archived: bool,
}

#[utoipa::path(get, path = "/projects", tag = "projects", params(ProjectFilter),
    responses((status = OK, body = Vec<Project>), (status = "default", body = ApiError)))]
pub async fn list_projects(
    State(app): State<Arc<App>>,
    _: Auth,
    Query(filter): Query<ProjectFilter>,
) -> Result<Json<Vec<Project>>, ApiError> {
    Ok(Json(
        sqlx::query_as(
            "SELECT * FROM project_view WHERE archived = ? AND (?2 IS NULL OR customer_id = ?2)
             ORDER BY customer_name COLLATE NOCASE, name COLLATE NOCASE",
        )
        .bind(filter.archived)
        .bind(filter.customer_id)
        .fetch_all(&app.db)
        .await?,
    ))
}

#[utoipa::path(get, path = "/projects/{id}", tag = "projects", params(("id" = i64, Path)),
    responses((status = OK, body = Project), (status = "default", body = ApiError)))]
pub async fn get_project(
    State(app): State<Arc<App>>,
    _: Auth,
    Path(id): Path<i64>,
) -> Result<Json<Project>, ApiError> {
    Ok(Json(find(&mut *app.db.acquire().await?, id).await?))
}

#[utoipa::path(post, path = "/projects", tag = "projects", request_body = ProjectInput,
    responses((status = OK, body = Project), (status = "default", body = ApiError)))]
pub async fn create_project(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Project>, ApiError> {
    let customer_id = input.customer_id;
    let fields = validate(input)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let archived: bool = sqlx::query_scalar("SELECT archived FROM customer WHERE id = ?")
        .bind(customer_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    if archived {
        return Err(ApiError::new(ErrorCode::Archived));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO project (customer_id, name, hourly_rate, billable, archived, created_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(customer_id)
    .bind(&fields.name)
    .bind(fields.hourly_rate)
    .bind(fields.billable)
    .bind(fields.archived)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    let project = find(&mut tx, id).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "project.create",
        &format!("project:{id}"),
        None,
        Some(serde_json::to_value(&project).map_err(ApiError::internal)?),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(project))
}

#[utoipa::path(put, path = "/projects/{id}", tag = "projects", request_body = ProjectInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Project), (status = "default", body = ApiError)))]
pub async fn put_project(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Project>, ApiError> {
    let customer_id = input.customer_id;
    let fields = validate(input)?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    if customer_id != before.customer_id {
        let mut check = Check::default();
        check.fail("customer_id", FieldError::Locked);
        check.done()?;
    }
    sqlx::query(
        "UPDATE project SET name = ?, hourly_rate = ?, billable = ?, archived = ? WHERE id = ?",
    )
    .bind(&fields.name)
    .bind(fields.hourly_rate)
    .bind(fields.billable)
    .bind(fields.archived)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if let Some((old, new)) = audit::changes(&before.fields, &fields) {
        audit::record(
            &mut tx,
            auth.user.id,
            "project.update",
            &format!("project:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    let project = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(project))
}

pub async fn find(db: &mut SqliteConnection, id: i64) -> Result<Project, ApiError> {
    sqlx::query_as("SELECT * FROM project_view WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}

fn validate(input: ProjectInput) -> Result<ProjectFields, ApiError> {
    let mut check = Check::default();
    if input.hourly_rate.is_some_and(|rate| !money::is_rate(rate)) {
        check.fail("hourly_rate", FieldError::InvalidFormat);
    }
    let fields = ProjectFields {
        name: check.required("name", input.name, 200),
        hourly_rate: input.hourly_rate,
        billable: input.billable,
        archived: input.archived,
    };
    check.done()?;
    Ok(fields)
}
