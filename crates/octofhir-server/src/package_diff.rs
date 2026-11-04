//! `POST /api/packages/diff`: the semantic diff of two package versions (fhir-diff).
//!
//! An installed package comes from the package store (`fcm.resources`). One that is not
//! installed is downloaded from the package registry and read in memory, never installed: the
//! server's profiles and validation do not change because someone compared versions. A
//! profile's bases come from the manifest's dependencies, then from the installed packages, and
//! core bases from the built-in FHIR Schemas. The report is JSON for the console, or Markdown,
//! SARIF or text to download.

use std::collections::{BTreeSet, VecDeque};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use fhir_diff::{Definition, Filter, Loaded, Package, Pool, Report, Severity, report};
use octofhir_api::ApiError;
use serde::Deserialize;
use serde_json::Value;
use sqlx_core::query::query;
use sqlx_core::row::Row;
use sqlx_postgres::PgPool;

use crate::server::AppState;

const CORE_PREFIX: &str = "http://hl7.org/fhir/StructureDefinition/";

/// What to compare and how to report it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDiffRequest {
    /// The old package, `name@version`.
    pub old: String,
    /// The new package, `name@version`.
    pub new: String,
    /// Compare only these profiles (canonical URL or name).
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub format: DiffFormat,
    /// Changes below this severity are left out of Markdown, SARIF and text.
    #[serde(default)]
    pub min_severity: MinSeverity,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffFormat {
    #[default]
    Json,
    Markdown,
    Sarif,
    Text,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MinSeverity {
    #[default]
    Info,
    Warning,
    Breaking,
}

impl From<MinSeverity> for Severity {
    fn from(value: MinSeverity) -> Self {
        match value {
            MinSeverity::Info => Severity::Info,
            MinSeverity::Warning => Severity::Warning,
            MinSeverity::Breaking => Severity::Breaking,
        }
    }
}

pub async fn api_packages_diff(
    State(state): State<AppState>,
    Json(request): Json<PackageDiffRequest>,
) -> Result<Response, ApiError> {
    let (old_name, old_version) = split_id(&request.old)?;
    let (new_name, new_version) = split_id(&request.new)?;
    let key = format!(
        "{}|{}|{}",
        request.old,
        request.new,
        request.profiles.join(",")
    );
    let report = match REPORTS.get(&key) {
        Some(report) => report,
        None => {
            let report = compare(
                &state,
                (old_name, old_version),
                (new_name, new_version),
                &request.profiles,
            )
            .await?;
            REPORTS.put(key, report)
        }
    };

    let min = Severity::from(request.min_severity);
    let (content_type, body) = match request.format {
        DiffFormat::Json => return Ok((StatusCode::OK, Json(&*report)).into_response()),
        DiffFormat::Markdown => (
            "text/markdown; charset=utf-8",
            report::markdown(&report, min),
        ),
        DiffFormat::Sarif => ("application/sarif+json", report::sarif(&report, min)),
        DiffFormat::Text => ("text/plain; charset=utf-8", report::text(&report, min)),
    };
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, content_type)], body).into_response())
}

async fn compare(
    state: &AppState,
    old: (&str, &str),
    new: (&str, &str),
    profiles: &[String],
) -> Result<Report, ApiError> {
    let (old, old_downloaded) = load(state, old.0, old.1).await?;
    let (new, new_downloaded) = load(state, new.0, new.1).await?;
    let (old_id, new_id) = (old.package.id(), new.package.id());
    let downloaded: Vec<String> = [(old_downloaded, &old_id), (new_downloaded, &new_id)]
        .into_iter()
        .filter(|(downloaded, _)| *downloaded)
        .map(|(_, id)| format!("{id}: not installed; read from the package registry"))
        .collect();
    let filter = Filter {
        profiles: profiles.to_vec(),
    };
    // Building the effective trees of a large guide takes a while; keep it off the runtime.
    let mut report = tokio::task::spawn_blocking(move || {
        let (old_pool, new_pool) = Pool::pair(&old, &new);
        fhir_diff::diff(&old_id, &old_pool, &new_id, &new_pool, &filter)
    })
    .await
    .map_err(|error| ApiError::Internal(format!("package diff failed: {error}")))?;
    report.notes.splice(0..0, downloaded);
    Ok(report)
}

fn split_id(id: &str) -> Result<(&str, &str), ApiError> {
    id.split_once('@')
        .filter(|(name, version)| !name.is_empty() && !version.is_empty())
        .ok_or_else(|| ApiError::bad_request(format!("`{id}` is not name@version")))
}

/// At most this many manifest dependencies are read for one package.
const MAX_DEPENDENCIES: usize = 24;

/// The last few values by key, most recently used last.
struct Recent<T> {
    items: Mutex<VecDeque<(String, Arc<T>)>>,
    capacity: usize,
}

impl<T> Recent<T> {
    const fn new(capacity: usize) -> Self {
        Self {
            items: Mutex::new(VecDeque::new()),
            capacity,
        }
    }

    fn get(&self, key: &str) -> Option<Arc<T>> {
        let mut items = self.items.lock().unwrap_or_else(|p| p.into_inner());
        let index = items.iter().position(|(k, _)| k == key)?;
        let item = items.remove(index)?;
        let value = Arc::clone(&item.1);
        items.push_back(item);
        Some(value)
    }

    fn put(&self, key: String, value: T) -> Arc<T> {
        let value = Arc::new(value);
        let mut items = self.items.lock().unwrap_or_else(|p| p.into_inner());
        items.retain(|(k, _)| *k != key);
        items.push_back((key, Arc::clone(&value)));
        while items.len() > self.capacity {
            items.pop_front();
        }
        value
    }
}

/// Packages read from the registry: comparing one guide with several versions downloads it once.
static DOWNLOADED: Recent<Package> = Recent::new(8);
/// Reports, so that exporting the comparison on screen does not compute it again.
static REPORTS: Recent<Report> = Recent::new(16);

/// The canonical URLs a package's profiles are based on: those found so far, those still
/// missing, and the definitions taken from other installed packages.
#[derive(Default)]
struct Bases {
    known: BTreeSet<String>,
    missing: BTreeSet<String>,
    asked: BTreeSet<String>,
    borrowed: Vec<Definition>,
}

impl Bases {
    fn add(&mut self, definitions: &[Definition]) {
        self.known.extend(
            definitions
                .iter()
                .filter_map(|d| d.json["url"].as_str().map(str::to_owned)),
        );
        let known = &self.known;
        self.missing.retain(|url| !known.contains(url));
        let next = bases(definitions, &self.known);
        self.missing.extend(next);
    }

    /// Take the missing bases from the installed packages (never another version of the
    /// package compared), following their own bases.
    async fn from_installed(&mut self, pool: &PgPool, name: &str) -> Result<(), ApiError> {
        loop {
            let urls: Vec<String> = self.missing.difference(&self.asked).cloned().collect();
            if urls.is_empty() {
                return Ok(());
            }
            self.asked.extend(urls.iter().cloned());
            let rows = query(
                "SELECT DISTINCT ON (url) resource_id, content FROM fcm.resources \
                 WHERE resource_type = 'StructureDefinition' AND url = ANY($1) \
                   AND package_name <> $2 \
                 ORDER BY url, package_version DESC",
            )
            .bind(&urls)
            .bind(name)
            .fetch_all(pool)
            .await
            .map_err(database)?;
            let found: Vec<Definition> = rows.iter().map(definition).collect();
            self.add(&found);
            self.borrowed.extend(found);
        }
    }
}

/// A package to compare with the definitions its profiles are based on, and whether it came
/// from the registry (not installed).
async fn load(state: &AppState, name: &str, version: &str) -> Result<(Loaded, bool), ApiError> {
    let pool = state.db_pool.as_ref();
    let (package, from_registry) = match installed(pool, name, version).await? {
        Some(package) => (package, false),
        None => ((*registry(state, name, version).await?).clone(), true),
    };
    let mut bases = Bases::default();
    bases.add(&package.definitions);
    bases.from_installed(pool, name).await?;

    // The manifest's dependencies (a downloaded package has a manifest; the package store keeps
    // none), installed or else downloaded, only while a base is still missing.
    let mut dependencies: Vec<Package> = Vec::new();
    let mut seen = BTreeSet::from([package.id()]);
    let mut queue: VecDeque<(String, String)> = listed(&package).collect();
    while !bases.missing.is_empty() && dependencies.len() < MAX_DEPENDENCIES {
        let Some((dep_name, dep_version)) = queue.pop_front() else {
            break;
        };
        if !seen.insert(format!("{dep_name}@{dep_version}")) {
            continue;
        }
        let dependency = match installed(pool, &dep_name, &dep_version).await? {
            Some(dependency) => dependency,
            None => match registry(state, &dep_name, &dep_version).await {
                Ok(dependency) => (*dependency).clone(),
                Err(error) => {
                    // A missing dependency only costs the profiles based on it.
                    tracing::warn!(package = %format!("{dep_name}@{dep_version}"), ?error, "package diff: dependency unavailable");
                    continue;
                }
            },
        };
        queue.extend(listed(&dependency));
        bases.add(&dependency.definitions);
        dependencies.push(dependency);
        bases.from_installed(pool, name).await?;
    }
    if !bases.borrowed.is_empty() {
        dependencies.push(Package {
            name: "installed packages".to_owned(),
            definitions: bases.borrowed,
            ..Package::default()
        });
    }
    Ok((
        Loaded {
            package,
            dependencies,
        },
        from_registry,
    ))
}

fn database(error: sqlx_core::Error) -> ApiError {
    ApiError::Internal(format!("database error: {error}"))
}

/// The packages a manifest depends on that may hold a base (no core or terminology package).
fn listed(package: &Package) -> impl Iterator<Item = (String, String)> + '_ {
    fhir_diff::package::base_dependencies(package)
        .map(|(name, version)| (name.to_owned(), version.to_owned()))
}

/// The StructureDefinitions of an installed package, from the package store.
async fn installed(pool: &PgPool, name: &str, version: &str) -> Result<Option<Package>, ApiError> {
    let Some(row) = query("SELECT fhir_version FROM fcm.packages WHERE name = $1 AND version = $2")
        .bind(name)
        .bind(version)
        .fetch_optional(pool)
        .await
        .map_err(database)?
    else {
        return Ok(None);
    };
    let fhir_version: Option<String> = row.get("fhir_version");
    let rows = query(
        "SELECT resource_id, content FROM fcm.resources \
         WHERE package_name = $1 AND package_version = $2 \
           AND resource_type = 'StructureDefinition' \
         ORDER BY resource_id",
    )
    .bind(name)
    .bind(version)
    .fetch_all(pool)
    .await
    .map_err(database)?;
    Ok(Some(Package {
        name: name.to_owned(),
        version: version.to_owned(),
        fhir_versions: fhir_version.into_iter().collect(),
        dependencies: Default::default(),
        definitions: rows.iter().map(definition).collect(),
    }))
}

/// A package that is not installed, downloaded from the registry and read in memory. The
/// download runs as its own task: when the client gives up, the package is still kept for the
/// next request.
async fn registry(state: &AppState, name: &str, version: &str) -> Result<Arc<Package>, ApiError> {
    let id = format!("{name}@{version}");
    if let Some(package) = DOWNLOADED.get(&id) {
        return Ok(package);
    }
    let config = Arc::clone(&state.config);
    let (name, version) = (name.to_owned(), version.to_owned());
    tokio::spawn(async move {
        let tarball = crate::canonical::download_registry_package(&config, &name, &version)
            .await
            .map_err(|error| {
                ApiError::NotFound(format!(
                    "{id} is neither installed nor available from the package registry: {error}"
                ))
            })?;
        let package = tokio::task::spawn_blocking(move || read_tarball(&tarball))
            .await
            .map_err(|error| ApiError::Internal(format!("reading {id} failed: {error}")))?
            .map_err(|error| ApiError::Internal(format!("reading {id} failed: {error}")))?;
        Ok(DOWNLOADED.put(id, package))
    })
    .await
    .map_err(|error| ApiError::Internal(format!("package download failed: {error}")))?
}

/// The manifest and StructureDefinitions of a package tarball (`package/*.json`).
fn read_tarball(path: &Path) -> Result<Package, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut manifest = Value::Null;
    let mut definitions = Vec::new();
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let mut parts = entry_path
            .components()
            .map(|c| c.as_os_str().to_string_lossy());
        let (Some(root), Some(file), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if root != "package" || !file.ends_with(".json") {
            continue;
        }
        let file = file.into_owned();
        let mut text = String::new();
        if entry.read_to_string(&mut text).is_err() {
            continue;
        }
        if file == "package.json" {
            manifest = serde_json::from_str(&text).unwrap_or(Value::Null);
            continue;
        }
        // Most files are examples and other resources; parse only what may be a definition.
        if !text.contains("\"StructureDefinition\"") {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<Value>(&text)
            && json["resourceType"] == "StructureDefinition"
        {
            definitions.push(Definition { file, json });
        }
    }
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    Ok(Package {
        name: text(&manifest["name"]),
        version: text(&manifest["version"]),
        fhir_versions: manifest["fhirVersions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        dependencies: manifest["dependencies"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(name, version)| Some((name.clone(), version.as_str()?.to_owned())))
            .collect(),
        definitions,
    })
}

fn definition(row: &sqlx_postgres::PgRow) -> Definition {
    let id: Option<String> = row.get("resource_id");
    let json: Value = row.get("content");
    Definition {
        file: format!("StructureDefinition-{}.json", id.unwrap_or_default()),
        json,
    }
}

/// Base definitions of `definitions` that are neither core nor already `known`.
fn bases<'a>(
    definitions: impl IntoIterator<Item = &'a Definition>,
    known: &BTreeSet<String>,
) -> BTreeSet<String> {
    definitions
        .into_iter()
        .filter_map(|d| d.json["baseDefinition"].as_str())
        .map(|url| url.split('|').next().unwrap_or(url).to_owned())
        .filter(|url| !url.starts_with(CORE_PREFIX) && !known.contains(url))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sd(url: &str, base: &str) -> Definition {
        Definition {
            file: String::new(),
            json: json!({"resourceType": "StructureDefinition", "url": url, "baseDefinition": base}),
        }
    }

    #[test]
    fn a_tarball_gives_its_manifest_and_definitions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("p.tgz");
        let file = std::fs::File::create(&path).expect("create");
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::fast(),
        ));
        let mut add = |name: &str, json: Value| {
            let bytes = serde_json::to_vec(&json).expect("json");
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, name, bytes.as_slice())
                .expect("append");
        };
        add(
            "package/package.json",
            json!({"name": "example.pkg", "version": "2.0.0", "fhirVersions": ["4.0.1"],
                   "dependencies": {"hl7.fhir.r4.core": "4.0.1", "example.base": "1.0.0"}}),
        );
        add(
            "package/StructureDefinition-p.json",
            json!({"resourceType": "StructureDefinition", "url": "http://example.org/p"}),
        );
        add(
            "package/Patient-example.json",
            json!({"resourceType": "Patient", "id": "example"}),
        );
        add(
            "package/other/StructureDefinition-nested.json",
            json!({"resourceType": "StructureDefinition", "url": "http://example.org/nested"}),
        );
        builder.into_inner().expect("tar").finish().expect("gzip");

        let package = read_tarball(&path).expect("read");
        assert_eq!(package.id(), "example.pkg@2.0.0");
        assert_eq!(package.fhir_versions, ["4.0.1"]);
        assert_eq!(
            listed(&package).collect::<Vec<_>>(),
            [("example.base".to_owned(), "1.0.0".to_owned())]
        );
        assert_eq!(package.definitions.len(), 1);
        assert_eq!(package.definitions[0].file, "StructureDefinition-p.json");
    }

    #[test]
    fn ids_are_name_at_version() {
        assert_eq!(
            split_id("hl7.fhir.us.core@6.1.0").unwrap(),
            ("hl7.fhir.us.core", "6.1.0")
        );
        assert!(split_id("hl7.fhir.us.core").is_err());
        assert!(split_id("@6.1.0").is_err());
    }

    #[test]
    fn bases_skip_core_known_and_versions() {
        let definitions = [
            sd(
                "http://example.org/a",
                "http://hl7.org/fhir/StructureDefinition/Patient",
            ),
            sd("http://example.org/b", "http://example.org/a"),
            sd(
                "http://example.org/c",
                "http://hl7.org/fhir/uv/sdc/StructureDefinition/sdc-questionnaireresponse|3.0.0",
            ),
        ];
        let known = BTreeSet::from(["http://example.org/a".to_owned()]);
        assert_eq!(
            bases(definitions.iter(), &known),
            BTreeSet::from([
                "http://hl7.org/fhir/uv/sdc/StructureDefinition/sdc-questionnaireresponse"
                    .to_owned()
            ])
        );
    }
}
