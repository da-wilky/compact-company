use std::{collections::BTreeSet, str::FromStr, sync::Arc};

use axum::{
    extract::{Multipart, State, multipart::MultipartRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use typst::{foundations::Bytes, syntax::package::PackageSpec, text::Font};
use utoipa::{IntoParams, ToSchema};

use super::{
    customers::Language,
    documents::business,
    error::{ApiError, ErrorCode, FieldError, Json, Path, Query},
    input::{Check, field_error},
};
use crate::{
    App, audit,
    auth::{Auth, OwnerOnly, now},
    documents::{self, Kind, SCHEMA},
    layouts,
    render::{self, Layout, is_font},
};

const MAX_SOURCE: usize = 1 << 20;
const MAX_FILES: usize = 20 << 20;

/// A layout as uploaded: multipart fields `name`, `kinds` (repeated), the Typst file `source`
/// (`layout.typ`, defining `schema` and `render(data)`), and `files`: images it reads under
/// `assets/` and the fonts it uses.
#[derive(ToSchema)]
#[allow(
    dead_code,
    reason = "describes the multipart body for the OpenAPI document"
)]
pub struct LayoutUpload {
    name: String,
    kinds: Vec<Kind>,
    /// Required when uploading; kept when left out of an update.
    #[schema(value_type = Option<String>, format = Binary)]
    source: Option<Vec<u8>>,
    /// When sent with an update, they replace all files.
    #[schema(value_type = Vec<String>, format = Binary)]
    files: Vec<Vec<u8>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LayoutView {
    id: i64,
    name: String,
    kinds: Vec<Kind>,
    /// The data schema it is written for; it renders only while that is the app's.
    schema: u32,
    current: bool,
    files: Vec<LayoutFile>,
    packages: Vec<String>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct LayoutFile {
    name: String,
    font: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
pub struct LayoutChoice {
    kind: Kind,
    /// Only for this customer; for every customer when left out.
    customer_id: Option<i64>,
    layout_id: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Package {
    /// Such as `@preview/tablex:0.0.8`.
    spec: String,
    approved_at: i64,
    approved_by: String,
    /// Packages it imports that are not approved yet.
    requires: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageInput {
    spec: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SampleQuery {
    kind: Kind,
    language: Language,
}

#[utoipa::path(get, path = "/layouts", tag = "layouts",
    responses((status = OK, body = Vec<LayoutView>), (status = "default", body = ApiError)))]
pub async fn list_layouts(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<Vec<LayoutView>>, ApiError> {
    let mut db = app.db.acquire().await?;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM layout ORDER BY name COLLATE NOCASE")
        .fetch_all(&mut *db)
        .await?;
    let mut list = Vec::new();
    for id in ids {
        list.push(view(&mut db, id).await?);
    }
    Ok(Json(list))
}

#[utoipa::path(post, path = "/layouts", tag = "layouts",
    request_body(content_type = "multipart/form-data", content = LayoutUpload),
    responses((status = OK, body = LayoutView), (status = "default", body = ApiError)))]
pub async fn create_layout(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<LayoutView>, ApiError> {
    let upload = read(multipart).await?;
    let Some(source) = upload.source.clone() else {
        return Err(field_error("source", FieldError::Required));
    };
    let packages = prepare(&app, &upload, &source).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let at = now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO layout (name, source, schema, kinds, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&upload.name)
    .bind(&source)
    .bind(SCHEMA)
    .bind(serde_json::to_string(&upload.kinds).map_err(ApiError::internal)?)
    .bind(at)
    .bind(at)
    .fetch_one(&mut *tx)
    .await?;
    store_files(&app, &mut tx, id, &upload.files).await?;
    store_packages(&mut tx, id, &packages).await?;
    let layout = view(&mut tx, id).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "layout.create",
        &format!("layout:{id}"),
        None,
        Some(serde_json::to_value(&layout).map_err(ApiError::internal)?),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(layout))
}

#[utoipa::path(put, path = "/layouts/{id}", tag = "layouts", params(("id" = i64, Path)),
    request_body(content_type = "multipart/form-data", content = LayoutUpload),
    responses((status = OK, body = LayoutView), (status = "default", body = ApiError)))]
pub async fn put_layout(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<LayoutView>, ApiError> {
    let mut upload = read(multipart).await?;
    let (before, stored_source) = {
        let mut db = app.db.acquire().await?;
        let before = view(&mut db, id).await?;
        if upload.files.is_empty() {
            upload.files = stored_files(&app, &mut db, id).await?;
        }
        let source: String = sqlx::query_scalar("SELECT source FROM layout WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *db)
            .await?;
        (before, source)
    };
    let source = upload.source.clone().unwrap_or(stored_source);
    let packages = prepare(&app, &upload, &source).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let used_elsewhere: Vec<Kind> =
        sqlx::query_scalar("SELECT kind FROM layout_choice WHERE layout_id = ?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    if used_elsewhere.iter().any(|k| !upload.kinds.contains(k)) {
        let mut check = Check::default();
        check.fail("kinds", FieldError::Locked);
        check.done()?;
    }
    sqlx::query(
        "UPDATE layout SET name = ?, source = ?, schema = ?, kinds = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&upload.name)
    .bind(&source)
    .bind(SCHEMA)
    .bind(serde_json::to_string(&upload.kinds).map_err(ApiError::internal)?)
    .bind(now())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    for delete in [
        "DELETE FROM layout_file WHERE layout_id = ?",
        "DELETE FROM layout_package WHERE layout_id = ?",
    ] {
        sqlx::query(delete).bind(id).execute(&mut *tx).await?;
    }
    store_files(&app, &mut tx, id, &upload.files).await?;
    store_packages(&mut tx, id, &packages).await?;
    let layout = view(&mut tx, id).await?;
    if let Some((old, new)) = audit::changes(&before, &layout) {
        audit::record(
            &mut tx,
            auth.user.id,
            "layout.update",
            &format!("layout:{id}"),
            Some(old),
            Some(new),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(layout))
}

/// Documents it was chosen for fall back to the built-in layout; issued ones keep their PDF.
#[utoipa::path(delete, path = "/layouts/{id}", tag = "layouts", params(("id" = i64, Path)),
    responses((status = NO_CONTENT), (status = "default", body = ApiError)))]
pub async fn delete_layout(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let before = view(&mut tx, id).await?;
    sqlx::query("DELETE FROM layout WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "layout.delete",
        &format!("layout:{id}"),
        Some(serde_json::to_value(&before).map_err(ApiError::internal)?),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The layout rendered with sample data, as PDF.
#[utoipa::path(get, path = "/layouts/{id}/sample", tag = "layouts",
    params(("id" = i64, Path), SampleQuery),
    responses((status = OK, content_type = "application/pdf", body = Vec<u8>),
        (status = "default", body = ApiError)))]
pub async fn layout_sample(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
    Path(id): Path<i64>,
    Query(query): Query<SampleQuery>,
) -> Result<Response, ApiError> {
    let layout = if id == 0 {
        Layout::built_in()
    } else {
        layouts::load(&app, &mut *app.db.acquire().await?, id).await?
    };
    let small_business = business(&app).await?.small_business();
    let data = documents::sample(query.kind, query.language, small_business);
    let (rendered, _) = render::render_blocking(Arc::new(layout), data, Vec::new())
        .await
        .map_err(ApiError::layout)?;
    Ok((
        [(header::CONTENT_TYPE, "application/pdf")],
        rendered.pdf().map_err(ApiError::layout)?,
    )
        .into_response())
}

#[utoipa::path(get, path = "/layout-choices", tag = "layouts",
    responses((status = OK, body = Vec<LayoutChoice>), (status = "default", body = ApiError)))]
pub async fn list_choices(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<Vec<LayoutChoice>>, ApiError> {
    Ok(Json(choices(&mut *app.db.acquire().await?).await?))
}

/// Replaces every choice; a kind without one uses the built-in layout.
#[utoipa::path(put, path = "/layout-choices", tag = "layouts", request_body = Vec<LayoutChoice>,
    responses((status = OK, body = Vec<LayoutChoice>), (status = "default", body = ApiError)))]
pub async fn put_choices(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<Vec<LayoutChoice>>,
) -> Result<Json<Vec<LayoutChoice>>, ApiError> {
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let mut check = Check::default();
    let mut seen = BTreeSet::new();
    for (i, choice) in input.iter().enumerate() {
        let kinds: Option<String> = sqlx::query_scalar("SELECT kinds FROM layout WHERE id = ?")
            .bind(choice.layout_id)
            .fetch_optional(&mut *tx)
            .await?;
        let supported = kinds
            .and_then(|k| serde_json::from_str::<Vec<Kind>>(&k).ok())
            .is_some_and(|k| k.contains(&choice.kind));
        if !supported {
            check.fail(&format!("{i}.layout_id"), FieldError::InvalidFormat);
        }
        if let Some(customer) = choice.customer_id {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM customer WHERE id = ?)")
                    .bind(customer)
                    .fetch_one(&mut *tx)
                    .await?;
            if !exists {
                check.fail(&format!("{i}.customer_id"), FieldError::InvalidFormat);
            }
        }
        if !seen.insert((choice.kind.name(), choice.customer_id)) {
            check.fail(&format!("{i}.kind"), FieldError::InvalidFormat);
        }
    }
    check.done()?;
    let before = choices(&mut tx).await?;
    sqlx::query("DELETE FROM layout_choice")
        .execute(&mut *tx)
        .await?;
    for choice in &input {
        sqlx::query("INSERT INTO layout_choice (kind, customer_id, layout_id) VALUES (?, ?, ?)")
            .bind(choice.kind)
            .bind(choice.customer_id)
            .bind(choice.layout_id)
            .execute(&mut *tx)
            .await?;
    }
    let after = choices(&mut tx).await?;
    audit::record(
        &mut tx,
        auth.user.id,
        "layout.choose",
        "layouts",
        Some(serde_json::to_value(&before).map_err(ApiError::internal)?),
        Some(serde_json::to_value(&after).map_err(ApiError::internal)?),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(after))
}

#[utoipa::path(get, path = "/packages", tag = "layouts",
    responses((status = OK, body = Vec<Package>), (status = "default", body = ApiError)))]
pub async fn list_packages(
    State(app): State<Arc<App>>,
    _: Auth<OwnerOnly>,
) -> Result<Json<Vec<Package>>, ApiError> {
    let mut db = app.db.acquire().await?;
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT namespace, name, version FROM package ORDER BY name, version")
            .fetch_all(&mut *db)
            .await?;
    let mut list = Vec::new();
    for (namespace, name, version) in rows {
        list.push(package(&app, &mut db, &format!("@{namespace}/{name}:{version}")).await?);
    }
    Ok(Json(list))
}

/// Downloads a package by name and version and approves it for layouts.
#[utoipa::path(post, path = "/packages", tag = "layouts", request_body = PackageInput,
    responses((status = OK, body = Package), (status = "default", body = ApiError)))]
pub async fn approve_package(
    State(app): State<Arc<App>>,
    auth: Auth<OwnerOnly>,
    Json(input): Json<PackageInput>,
) -> Result<Json<Package>, ApiError> {
    let spec = PackageSpec::from_str(input.spec.trim())
        .map_err(|_| field_error("spec", FieldError::InvalidFormat))?;
    let name = spec.to_string();
    if layouts::package_archive(&app, &mut *app.db.acquire().await?, &spec)
        .await?
        .is_none()
    {
        let archive = layouts::download(&app, &spec).await?;
        let hash = app
            .files
            .put(archive.into())
            .await
            .map_err(ApiError::internal)?;
        let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "INSERT OR IGNORE INTO package (namespace, name, version, hash, approved_by, approved_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(spec.namespace.as_str())
        .bind(spec.name.as_str())
        .bind(spec.version.to_string())
        .bind(hash.to_string())
        .bind(auth.user.id)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        audit::record(
            &mut tx,
            auth.user.id,
            "package.approve",
            &format!("package:{name}"),
            None,
            Some(serde_json::json!({ "spec": name, "hash": hash.to_string() })),
        )
        .await?;
        tx.commit().await?;
    }
    Ok(Json(
        package(&app, &mut *app.db.acquire().await?, &name).await?,
    ))
}

async fn package(app: &App, db: &mut SqliteConnection, name: &str) -> Result<Package, ApiError> {
    let spec = PackageSpec::from_str(name).map_err(ApiError::internal)?;
    let (approved_at, approved_by): (i64, String) = sqlx::query_as(
        "SELECT p.approved_at, u.name FROM package p JOIN user u ON u.id = p.approved_by
         WHERE p.namespace = ? AND p.name = ? AND p.version = ?",
    )
    .bind(spec.namespace.as_str())
    .bind(spec.name.as_str())
    .bind(spec.version.to_string())
    .fetch_one(&mut *db)
    .await?;
    let archive = layouts::package_archive(app, db, &spec)
        .await?
        .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let mut requires = BTreeSet::new();
    for (path, bytes) in layouts::unpack(&archive).map_err(ApiError::internal)? {
        if path.ends_with(".typ")
            && let Ok(text) = std::str::from_utf8(&bytes)
        {
            requires.extend(layouts::imports(text));
        }
    }
    let mut missing = Vec::new();
    for required in requires {
        let spec = PackageSpec::from_str(&required).map_err(ApiError::internal)?;
        if layouts::package_archive(app, db, &spec).await?.is_none() {
            missing.push(required);
        }
    }
    Ok(Package {
        spec: name.to_owned(),
        approved_at,
        approved_by,
        requires: missing,
    })
}

async fn choices(db: &mut SqliteConnection) -> Result<Vec<LayoutChoice>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT kind, customer_id, layout_id FROM layout_choice ORDER BY kind, customer_id",
    )
    .fetch_all(db)
    .await?)
}

async fn view(db: &mut SqliteConnection, id: i64) -> Result<LayoutView, ApiError> {
    let row: Option<(String, u32, String, i64, i64)> = sqlx::query_as(
        "SELECT name, schema, kinds, created_at, updated_at FROM layout WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *db)
    .await?;
    let (name, schema, kinds, created_at, updated_at) =
        row.ok_or(ApiError::new(ErrorCode::NotFound))?;
    let files =
        sqlx::query_as("SELECT name, font FROM layout_file WHERE layout_id = ? ORDER BY name")
            .bind(id)
            .fetch_all(&mut *db)
            .await?;
    let packages = sqlx::query_scalar(
        "SELECT '@' || namespace || '/' || name || ':' || version FROM layout_package
         WHERE layout_id = ? ORDER BY name",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    Ok(LayoutView {
        id,
        name,
        kinds: serde_json::from_str(&kinds).map_err(ApiError::internal)?,
        schema,
        current: schema == SCHEMA,
        files,
        packages,
        created_at,
        updated_at,
    })
}

struct Upload {
    name: String,
    kinds: Vec<Kind>,
    source: Option<String>,
    files: Vec<(String, Bytes)>,
}

async fn read(multipart: Result<Multipart, MultipartRejection>) -> Result<Upload, ApiError> {
    fn malformed(_: impl std::fmt::Display) -> ApiError {
        ApiError::new(ErrorCode::MalformedRequest)
    }
    let mut multipart = multipart.map_err(malformed)?;
    let mut check = Check::default();
    let (mut name, mut kinds, mut source, mut files) = (None, Vec::new(), None, Vec::new());
    let mut total = 0;
    while let Some(field) = multipart.next_field().await.map_err(malformed)? {
        let field_name = field.name().unwrap_or_default().to_owned();
        let file_name = field.file_name().map(str::to_owned);
        let bytes = field.bytes().await.map_err(malformed)?;
        total += bytes.len();
        match field_name.as_str() {
            "name" => name = Some(String::from_utf8_lossy(&bytes).into_owned()),
            "kinds" => match serde_json::from_value::<Kind>(serde_json::Value::from(
                String::from_utf8_lossy(&bytes).trim(),
            )) {
                Ok(kind) if !kinds.contains(&kind) => kinds.push(kind),
                Ok(_) => {}
                Err(_) => check.fail("kinds", FieldError::InvalidFormat),
            },
            "source" => match String::from_utf8(bytes.to_vec()) {
                Ok(text) if text.len() <= MAX_SOURCE => source = Some(text),
                Ok(_) => check.fail("source", FieldError::TooLong),
                Err(_) => check.fail("source", FieldError::InvalidFormat),
            },
            "files" => {
                let file_name = file_name.unwrap_or_default();
                let safe = !file_name.is_empty()
                    && file_name.len() <= 100
                    && !file_name.starts_with('.')
                    && file_name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
                if safe {
                    files.push((file_name, Bytes::new(bytes.to_vec())));
                } else {
                    check.fail("files", FieldError::InvalidFormat);
                }
            }
            _ => return Err(ApiError::new(ErrorCode::MalformedRequest)),
        }
    }
    if total > MAX_FILES {
        check.fail("files", FieldError::TooLong);
    }
    let name = check.required("name", name, 100);
    if kinds.is_empty() {
        check.fail("kinds", FieldError::Required);
    }
    for (file_name, bytes) in &files {
        if is_font(file_name) && Font::iter(bytes.clone()).next().is_none() {
            check.fail("files", FieldError::InvalidFormat);
        }
    }
    check.done()?;
    Ok(Upload {
        name,
        kinds,
        source,
        files,
    })
}

/// Checks the packages it imports are approved and that it renders every kind it lays out with
/// sample data, in both languages, showing every mandatory detail.
async fn prepare(app: &App, upload: &Upload, source: &str) -> Result<BTreeSet<String>, ApiError> {
    let packages = layouts::required_packages(app, &mut *app.db.acquire().await?, source).await?;
    let mut layout = Layout {
        source: source.to_owned(),
        ..Layout::default()
    };
    for (name, bytes) in &upload.files {
        if is_font(name) {
            layout.fonts.push(bytes.clone());
        } else {
            layout.assets.insert(name.clone(), bytes.clone());
        }
    }
    for name in &packages {
        let spec = PackageSpec::from_str(name).map_err(ApiError::internal)?;
        let archive = layouts::package_archive(app, &mut *app.db.acquire().await?, &spec)
            .await?
            .ok_or_else(|| ApiError::internal(format!("approved package {name} is gone")))?;
        layout
            .packages
            .insert(spec, layouts::unpack(&archive).map_err(ApiError::internal)?);
    }
    let layout = Arc::new(layout);
    let small_business = business(app).await?.small_business();
    let mut problems = Vec::new();
    for kind in &upload.kinds {
        for language in [Language::De, Language::En] {
            let data = documents::sample(*kind, language, small_business);
            let at = format!("{} ({})", kind.name(), language.code());
            match render::render_blocking(layout.clone(), data, Vec::new()).await {
                Err(e) => problems.push(format!("{at}: {e}")),
                Ok((rendered, data)) => {
                    let missing = render::missing(&data, &rendered);
                    if !missing.is_empty() {
                        problems.push(format!("{at}: does not show {}", missing.join(", ")));
                    }
                }
            }
        }
    }
    if !problems.is_empty() {
        return Err(ApiError::new(ErrorCode::LayoutFailed).with_detail(problems.join("\n")));
    }
    Ok(packages)
}

async fn store_files(
    app: &App,
    tx: &mut SqliteConnection,
    id: i64,
    files: &[(String, Bytes)],
) -> Result<(), ApiError> {
    for (name, bytes) in files {
        let hash = app
            .files
            .put(bytes.to_vec().into())
            .await
            .map_err(ApiError::internal)?;
        sqlx::query("INSERT INTO layout_file (layout_id, name, font, hash) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(name)
            .bind(is_font(name))
            .bind(hash.to_string())
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

async fn stored_files(
    app: &App,
    db: &mut SqliteConnection,
    id: i64,
) -> Result<Vec<(String, Bytes)>, ApiError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT name, hash FROM layout_file WHERE layout_id = ?")
            .bind(id)
            .fetch_all(db)
            .await?;
    let mut files = Vec::new();
    for (name, hash) in rows {
        let hash = hash.parse().map_err(ApiError::internal)?;
        let bytes = app.files.get(hash).await.map_err(ApiError::internal)?;
        files.push((name, Bytes::new(bytes.to_vec())));
    }
    Ok(files)
}

async fn store_packages(
    tx: &mut SqliteConnection,
    id: i64,
    packages: &BTreeSet<String>,
) -> Result<(), ApiError> {
    for name in packages {
        let spec = PackageSpec::from_str(name).map_err(ApiError::internal)?;
        sqlx::query(
            "INSERT INTO layout_package (layout_id, namespace, name, version) VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(spec.namespace.as_str())
        .bind(spec.name.as_str())
        .bind(spec.version.to_string())
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}
