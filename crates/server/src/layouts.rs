use std::{
    collections::{BTreeSet, HashMap},
    io::Read,
    str::FromStr,
    sync::Arc,
};

use sqlx::SqliteConnection;
use typst::{foundations::Bytes, syntax::package::PackageSpec};

use crate::{
    App,
    api::error::{ApiError, ErrorCode},
    documents::Kind,
    files::ContentHash,
    render::Layout,
};

/// The layout a document of `kind` for `customer_id` is rendered with: the customer's choice,
/// else the business's, else the built-in one.
pub async fn for_document(
    app: &App,
    db: &mut SqliteConnection,
    kind: Kind,
    customer_id: i64,
) -> Result<Arc<Layout>, ApiError> {
    let chosen: Option<i64> = sqlx::query_scalar(
        "SELECT layout_id FROM layout_choice WHERE kind = ? AND (customer_id = ? OR customer_id IS NULL)
         ORDER BY customer_id IS NULL LIMIT 1",
    )
    .bind(kind)
    .bind(customer_id)
    .fetch_optional(&mut *db)
    .await?;
    Ok(Arc::new(match chosen {
        Some(id) => load(app, db, id).await?,
        None => Layout::built_in(),
    }))
}

/// A stored layout with its files and pinned packages.
pub async fn load(app: &App, db: &mut SqliteConnection, id: i64) -> Result<Layout, ApiError> {
    let (name, source): (String, String) =
        sqlx::query_as("SELECT name, source FROM layout WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or(ApiError::new(ErrorCode::NotFound))?;
    let files: Vec<(String, bool, String)> =
        sqlx::query_as("SELECT name, font, hash FROM layout_file WHERE layout_id = ?")
            .bind(id)
            .fetch_all(&mut *db)
            .await?;
    let mut layout = Layout {
        name: Some(name),
        source,
        ..Layout::default()
    };
    for (name, font, hash) in files {
        let bytes = Bytes::new(get(app, &hash).await?);
        if font {
            layout.fonts.push(bytes);
        } else {
            layout.assets.insert(name, bytes);
        }
    }
    let packages: Vec<(String, String, String)> =
        sqlx::query_as("SELECT namespace, name, version FROM layout_package WHERE layout_id = ?")
            .bind(id)
            .fetch_all(&mut *db)
            .await?;
    for (namespace, name, version) in packages {
        let spec = spec(&namespace, &name, &version)?;
        let archive = package_archive(app, db, &spec)
            .await?
            .ok_or_else(|| ApiError::internal(format!("pinned package {spec} is not stored")))?;
        layout
            .packages
            .insert(spec, unpack(&archive).map_err(ApiError::internal)?);
    }
    Ok(layout)
}

async fn get(app: &App, hash: &str) -> Result<Vec<u8>, ApiError> {
    let hash = ContentHash::from_str(hash).map_err(ApiError::internal)?;
    Ok(app
        .files
        .get(hash)
        .await
        .map_err(ApiError::internal)?
        .to_vec())
}

fn spec(namespace: &str, name: &str, version: &str) -> Result<PackageSpec, ApiError> {
    format!("@{namespace}/{name}:{version}")
        .parse()
        .map_err(ApiError::internal)
}

pub async fn package_archive(
    app: &App,
    db: &mut SqliteConnection,
    spec: &PackageSpec,
) -> Result<Option<Vec<u8>>, ApiError> {
    let hash: Option<String> = sqlx::query_scalar(
        "SELECT hash FROM package WHERE namespace = ? AND name = ? AND version = ?",
    )
    .bind(spec.namespace.as_str())
    .bind(spec.name.as_str())
    .bind(spec.version.to_string())
    .fetch_optional(db)
    .await?;
    match hash {
        Some(hash) => Ok(Some(get(app, &hash).await?)),
        None => Ok(None),
    }
}

/// The files of a package archive (`.tar.gz`) by their path within the package.
pub fn unpack(archive: &[u8]) -> std::io::Result<HashMap<String, Bytes>> {
    let mut files = HashMap::new();
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.insert(format!("/{path}"), Bytes::new(bytes));
    }
    Ok(files)
}

/// The packages a Typst source imports, such as `@preview/tablex:0.0.8`.
pub fn imports(source: &str) -> BTreeSet<String> {
    source
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| s.starts_with('@') && PackageSpec::from_str(s).is_ok())
        .map(str::to_owned)
        .collect()
}

/// Every package `source` needs, directly or through other packages; the error lists those not
/// approved yet.
pub async fn required_packages(
    app: &App,
    db: &mut SqliteConnection,
    source: &str,
) -> Result<BTreeSet<String>, ApiError> {
    let mut queue: Vec<String> = imports(source).into_iter().collect();
    let mut found = BTreeSet::new();
    let mut unapproved = BTreeSet::new();
    while let Some(name) = queue.pop() {
        if found.contains(&name) || unapproved.contains(&name) {
            continue;
        }
        let spec = PackageSpec::from_str(&name).map_err(ApiError::internal)?;
        let Some(archive) = package_archive(app, db, &spec).await? else {
            unapproved.insert(name);
            continue;
        };
        for (path, bytes) in unpack(&archive).map_err(ApiError::internal)? {
            if path.ends_with(".typ")
                && let Ok(text) = std::str::from_utf8(&bytes)
            {
                queue.extend(imports(text));
            }
        }
        found.insert(name);
    }
    if !unapproved.is_empty() {
        return Err(ApiError::new(ErrorCode::PackageNotApproved)
            .with_detail(unapproved.into_iter().collect::<Vec<_>>().join(", ")));
    }
    Ok(found)
}

/// Downloads a package from the registry; only the `preview` namespace is published there.
pub async fn download(app: &App, spec: &PackageSpec) -> Result<Vec<u8>, ApiError> {
    if spec.namespace != "preview" {
        return Err(ApiError::new(ErrorCode::PackageUnavailable));
    }
    let url = app
        .typst_packages
        .join(&format!("preview/{}-{}.tar.gz", spec.name, spec.version))
        .map_err(ApiError::internal)?;
    let unavailable = |e: reqwest::Error| {
        eprintln!("cannot download {spec}: {e}");
        ApiError::new(ErrorCode::PackageUnavailable)
    };
    let archive = reqwest::get(url)
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(unavailable)?
        .bytes()
        .await
        .map_err(unavailable)?
        .to_vec();
    if unpack(&archive).is_err() {
        return Err(ApiError::new(ErrorCode::PackageUnavailable));
    }
    Ok(archive)
}
