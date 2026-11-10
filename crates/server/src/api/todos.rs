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
    auth::{Auth, OwnerOrStaff, now},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Normal,
    High,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TodoInput {
    project_id: i64,
    title: Option<String>,
    description: Option<String>,
    /// The first status when left out.
    status_id: Option<i64>,
    #[serde(default = "normal")]
    priority: Priority,
    /// `YYYY-MM-DD`.
    due_date: Option<String>,
    assignee_id: Option<i64>,
}

fn normal() -> Priority {
    Priority::Normal
}

/// What an edit may change; also the snapshot the audit log compares.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct TodoFields {
    project_id: i64,
    title: String,
    description: Option<String>,
    status_id: i64,
    priority: Priority,
    due_date: Option<String>,
    assignee_id: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Todo {
    id: i64,
    #[sqlx(flatten)]
    #[serde(flatten)]
    fields: TodoFields,
    project_name: String,
    customer_id: i64,
    customer_name: String,
    /// Whether its status counts as done.
    done: bool,
    assignee_name: Option<String>,
    /// Order within its status on the board.
    position: i64,
    comments: i64,
    created_by: i64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Comment {
    id: i64,
    user_id: i64,
    user_name: String,
    body: String,
    created_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TodoDetail {
    todo: Todo,
    comments: Vec<Comment>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Assignee {
    id: i64,
    name: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TodoFilter {
    project_id: Option<i64>,
    customer_id: Option<i64>,
    assignee_id: Option<i64>,
    status_id: Option<i64>,
    /// Only todos whose status counts as done (`true`) or not (`false`).
    done: Option<bool>,
    /// Only todos due before this day (`YYYY-MM-DD`), e.g. today for overdue ones.
    due_before: Option<String>,
}

#[utoipa::path(get, path = "/todos", tag = "todos", params(TodoFilter),
    responses((status = OK, body = Vec<Todo>), (status = "default", body = ApiError)))]
pub async fn list_todos(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrStaff>,
    Query(filter): Query<TodoFilter>,
) -> Result<Json<Vec<Todo>>, ApiError> {
    Ok(Json(
        sqlx::query_as(
            "SELECT t.* FROM todo_view t JOIN todo_status s ON s.id = t.status_id
             WHERE (?1 IS NULL OR t.project_id = ?1) AND (?2 IS NULL OR t.customer_id = ?2)
             AND (?3 IS NULL OR t.assignee_id = ?3) AND (?4 IS NULL OR t.status_id = ?4)
             AND (?5 IS NULL OR t.done = ?5) AND (?6 IS NULL OR t.due_date < ?6)
             ORDER BY s.position, t.position",
        )
        .bind(filter.project_id)
        .bind(filter.customer_id)
        .bind(filter.assignee_id)
        .bind(filter.status_id)
        .bind(filter.done)
        .bind(filter.due_before)
        .fetch_all(&app.db)
        .await?,
    ))
}

/// Users a todo may be assigned to: active owners and staff.
#[utoipa::path(get, path = "/assignees", tag = "todos",
    responses((status = OK, body = Vec<Assignee>), (status = "default", body = ApiError)))]
pub async fn list_assignees(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrStaff>,
) -> Result<Json<Vec<Assignee>>, ApiError> {
    Ok(Json(
        sqlx::query_as(
            "SELECT id, name FROM user WHERE active = 1 AND role IN ('owner', 'staff')
             ORDER BY name COLLATE NOCASE",
        )
        .fetch_all(&app.db)
        .await?,
    ))
}

#[utoipa::path(get, path = "/todos/{id}", tag = "todos", params(("id" = i64, Path)),
    responses((status = OK, body = TodoDetail), (status = "default", body = ApiError)))]
pub async fn get_todo(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOrStaff>,
    Path(id): Path<i64>,
) -> Result<Json<TodoDetail>, ApiError> {
    let mut db = app.db.acquire().await?;
    let todo = find(&mut db, id).await?;
    let comments = sqlx::query_as(
        "SELECT m.id, m.user_id, u.name AS user_name, m.body, m.created_at
         FROM todo_comment m JOIN user u ON u.id = m.user_id
         WHERE m.todo_id = ? ORDER BY m.id",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    Ok(Json(TodoDetail { todo, comments }))
}

#[utoipa::path(post, path = "/todos", tag = "todos", request_body = TodoInput,
    responses((status = OK, body = Todo), (status = "default", body = ApiError)))]
pub async fn create_todo(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Json(input): Json<TodoInput>,
) -> Result<Json<Todo>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let fields = validate(&mut tx, input, None).await?;
    let position: i64 =
        sqlx::query_scalar("SELECT coalesce(max(position), 0) + 1 FROM todo WHERE status_id = ?")
            .bind(fields.status_id)
            .fetch_one(&mut *tx)
            .await?;
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO todo (project_id, title, description, status_id, priority, due_date,
         assignee_id, position, created_by, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(fields.project_id)
    .bind(&fields.title)
    .bind(&fields.description)
    .bind(fields.status_id)
    .bind(fields.priority)
    .bind(&fields.due_date)
    .bind(fields.assignee_id)
    .bind(position)
    .bind(auth.user.id)
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "todo.create",
        &format!("todo:{id}"),
        None,
        Some(serde_json::to_value(&fields).map_err(ApiError::internal)?),
    )
    .await?;
    let todo = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(todo))
}

#[utoipa::path(put, path = "/todos/{id}", tag = "todos", request_body = TodoInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Todo), (status = "default", body = ApiError)))]
pub async fn put_todo(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Path(id): Path<i64>,
    Json(input): Json<TodoInput>,
) -> Result<Json<Todo>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    if input.project_id != before.fields.project_id && has_time(&mut tx, id).await? {
        let mut check = Check::default();
        check.fail("project_id", FieldError::Locked);
        check.done()?;
    }
    let fields = validate(&mut tx, input, Some(&before)).await?;
    sqlx::query(
        "UPDATE todo SET project_id = ?, title = ?, description = ?, status_id = ?, priority = ?,
         due_date = ?, assignee_id = ?, updated_at = ? WHERE id = ?",
    )
    .bind(fields.project_id)
    .bind(&fields.title)
    .bind(&fields.description)
    .bind(fields.status_id)
    .bind(fields.priority)
    .bind(&fields.due_date)
    .bind(fields.assignee_id)
    .bind(now())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if fields.status_id != before.fields.status_id {
        place(&mut tx, id, fields.status_id, None).await?;
    }
    record_changes(&mut tx, auth.user.id, id, &before.fields, &fields).await?;
    let todo = find(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(todo))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TodoMove {
    status_id: i64,
    /// The todo it lands in front of, in the same status; at the end when left out.
    before_id: Option<i64>,
}

/// Moves a todo on the board: to another status, another place in the same status, or both.
#[utoipa::path(post, path = "/todos/{id}/move", tag = "todos", request_body = TodoMove,
    params(("id" = i64, Path)),
    responses((status = OK, body = Todo), (status = "default", body = ApiError)))]
pub async fn move_todo(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Path(id): Path<i64>,
    Json(movement): Json<TodoMove>,
) -> Result<Json<Todo>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = find(&mut tx, id).await?;
    let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM todo_status WHERE id = ?)")
        .bind(movement.status_id)
        .fetch_one(&mut *tx)
        .await?;
    if !known {
        return Err(ApiError::new(ErrorCode::NotFound));
    }
    sqlx::query("UPDATE todo SET status_id = ?, updated_at = ? WHERE id = ?")
        .bind(movement.status_id)
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    place(&mut tx, id, movement.status_id, movement.before_id).await?;
    let todo = find(&mut tx, id).await?;
    record_changes(&mut tx, auth.user.id, id, &before.fields, &todo.fields).await?;
    tx.commit().await?;
    Ok(Json(todo))
}

#[utoipa::path(delete, path = "/todos/{id}", tag = "todos", params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn delete_todo(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let todo = find(&mut tx, id).await?;
    if has_time(&mut tx, id).await? {
        return Err(ApiError::new(ErrorCode::HasTime));
    }
    sqlx::query("DELETE FROM todo_comment WHERE todo_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM todo WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "todo.delete",
        &format!("todo:{id}"),
        Some(serde_json::to_value(&todo.fields).map_err(ApiError::internal)?),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentInput {
    body: Option<String>,
}

#[utoipa::path(post, path = "/todos/{id}/comments", tag = "todos", request_body = CommentInput,
    params(("id" = i64, Path)),
    responses((status = OK, body = Comment), (status = "default", body = ApiError)))]
pub async fn add_comment(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOrStaff>,
    Path(id): Path<i64>,
    Json(input): Json<CommentInput>,
) -> Result<Json<Comment>, ApiError> {
    let mut check = Check::default();
    let body = check.required("body", input.body, 10_000);
    check.done()?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    find(&mut tx, id).await?;
    let comment_id: i64 = sqlx::query_scalar(
        "INSERT INTO todo_comment (todo_id, user_id, body, created_at) VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(id)
    .bind(auth.user.id)
    .bind(&body)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "todo.comment",
        &format!("todo:{id}"),
        None,
        Some(serde_json::json!({ "comment_id": comment_id, "body": body })),
    )
    .await?;
    let comment = sqlx::query_as(
        "SELECT m.id, m.user_id, u.name AS user_name, m.body, m.created_at
         FROM todo_comment m JOIN user u ON u.id = m.user_id WHERE m.id = ?",
    )
    .bind(comment_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(comment))
}

async fn has_time(db: &mut SqliteConnection, id: i64) -> Result<bool, ApiError> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM time_entry WHERE todo_id = ?)")
            .bind(id)
            .fetch_one(db)
            .await?,
    )
}

pub async fn find(db: &mut SqliteConnection, id: i64) -> Result<Todo, ApiError> {
    sqlx::query_as("SELECT * FROM todo_view WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))
}

/// Puts the todo in front of `before_id` among the todos of its status, or last, and numbers
/// that status's todos afresh.
async fn place(
    db: &mut SqliteConnection,
    id: i64,
    status_id: i64,
    before_id: Option<i64>,
) -> Result<(), ApiError> {
    let mut order: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM todo WHERE status_id = ? AND id != ? ORDER BY position")
            .bind(status_id)
            .bind(id)
            .fetch_all(&mut *db)
            .await?;
    let at = before_id
        .and_then(|before| order.iter().position(|other| *other == before))
        .unwrap_or(order.len());
    order.insert(at, id);
    for (position, todo) in order.iter().enumerate() {
        sqlx::query("UPDATE todo SET position = ? WHERE id = ?")
            .bind(position as i64 + 1)
            .bind(todo)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}

async fn record_changes(
    db: &mut SqliteConnection,
    user_id: i64,
    id: i64,
    before: &TodoFields,
    after: &TodoFields,
) -> Result<(), ApiError> {
    if let Some((old, new)) = audit::changes(before, after) {
        audit::record(
            db,
            user_id,
            "todo.update",
            &format!("todo:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    Ok(())
}

async fn validate(
    db: &mut SqliteConnection,
    input: TodoInput,
    current: Option<&Todo>,
) -> Result<TodoFields, ApiError> {
    let mut check = Check::default();
    let due_date = check.text("due_date", input.due_date, 10);
    if due_date.as_deref().is_some_and(|d| !is_date(d)) {
        check.fail("due_date", FieldError::InvalidFormat);
    }
    let closed: Option<bool> = sqlx::query_scalar("SELECT closed FROM project_view WHERE id = ?")
        .bind(input.project_id)
        .fetch_optional(&mut *db)
        .await?;
    match closed {
        None => check.fail("project_id", FieldError::InvalidFormat),
        // a todo may stay on its project after that closes, but nothing new goes there
        Some(true) if current.is_none_or(|t| t.fields.project_id != input.project_id) => {
            return Err(ApiError::new(ErrorCode::Archived));
        }
        Some(_) => {}
    }
    let status_id = match input.status_id {
        Some(id) => {
            let known: bool =
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM todo_status WHERE id = ?)")
                    .bind(id)
                    .fetch_one(&mut *db)
                    .await?;
            if !known {
                check.fail("status_id", FieldError::InvalidFormat);
            }
            id
        }
        None => match current {
            Some(todo) => todo.fields.status_id,
            None => {
                sqlx::query_scalar("SELECT id FROM todo_status ORDER BY position LIMIT 1")
                    .fetch_one(&mut *db)
                    .await?
            }
        },
    };
    if let Some(assignee) = input.assignee_id {
        let assignable: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM user
             WHERE id = ? AND active = 1 AND role IN ('owner', 'staff'))",
        )
        .bind(assignee)
        .fetch_one(&mut *db)
        .await?;
        // someone who left may stay on the todos they had
        let unchanged = current.is_some_and(|t| t.fields.assignee_id == Some(assignee));
        if !assignable && !unchanged {
            check.fail("assignee_id", FieldError::InvalidFormat);
        }
    }
    let fields = TodoFields {
        project_id: input.project_id,
        title: check.required("title", input.title, 300),
        description: check.text("description", input.description, 20_000),
        status_id,
        priority: input.priority,
        due_date,
        assignee_id: input.assignee_id,
    };
    check.done()?;
    Ok(fields)
}

/// A real calendar day written as `YYYY-MM-DD`.
pub fn is_date(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    let [year, month, day] = parts[..] else {
        return false;
    };
    if year.len() != 4
        || month.len() != 2
        || day.len() != 2
        || !value.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<u32>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}
