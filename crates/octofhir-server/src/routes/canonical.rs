//! Canonical package management - Implementation Guide package upload.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use flate2::read::GzDecoder;
use octofhir_api::OperationOutcome;
use octofhir_db_postgres::PostgresPackageStore;
use serde::Serialize;
use std::io::Read;
use tar::Archive;
use tempfile::NamedTempFile;
use tracing::{error, info, warn};

use crate::canonical::{convert_and_store_package_schemas, rebuild_from_config_async};
use crate::config::AppConfig;
use std::sync::Arc;

/// State for canonical package operations.
#[derive(Clone)]
pub struct CanonicalPackageState {
    pub config: Arc<AppConfig>,
    pub package_store: Arc<PostgresPackageStore>,
    pub model_provider: Arc<crate::model_provider::OctoFhirModelProvider>,
}

impl axum::extract::FromRef<crate::server::AppState> for CanonicalPackageState {
    fn from_ref(app_state: &crate::server::AppState) -> Self {
        Self {
            config: app_state.config.clone(),
            package_store: app_state.package_store.clone(),
            model_provider: app_state.model_provider.clone(),
        }
    }
}

/// Upload package response.
#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub message: String,
    pub resources_loaded: usize,
}

/// Upload FHIR Implementation Guide package.
///
/// Accepts a tar.gz archive containing FHIR resources via multipart/form-data.
/// The "package" field must contain the tar.gz file.
///
/// Extracts all JSON files and loads them as FHIR resources into the canonical manager.
/// After loading, triggers a canonical registry reload.
///
/// # Endpoint
///
/// POST /api/canonical/$upload
///
/// # Authentication
///
/// Requires authentication (not admin-only).
///
/// # Example
///
/// ```bash
/// curl -X POST http://localhost:8888/api/canonical/\$upload \
///   -H "Authorization: Bearer <token>" \
///   -F "package=@psychportal-ig.tar.gz"
/// ```
pub async fn upload_package(
    State(state): State<CanonicalPackageState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, PackageError> {
    info!("Processing canonical package upload request");

    // Extract package file from multipart form
    let mut package_data: Option<Bytes> = None;

    while let Some(field) = multipart.next_field().await? {
        let field_name: Option<&str> = field.name();
        if field_name == Some("package") {
            package_data = Some(field.bytes().await?);
            break;
        }
    }

    let data = package_data.ok_or(PackageError::MissingPackage)?;
    info!("Received package archive: {} bytes", data.len());

    // Save to temporary file
    let mut temp_file = NamedTempFile::new()?;
    std::io::copy(&mut data.as_ref(), &mut temp_file)?;

    // Get FHIR version from config
    let fhir_version = &state.config.fhir.version;

    // Extract and load resources
    let (package_name, package_version, count) =
        extract_and_load_resources(temp_file.path(), state.package_store.as_ref(), fhir_version)
            .await?;

    // Convert and store FhirSchemas for the package
    info!(
        package = %package_name,
        version = %package_version,
        "converting and storing FhirSchemas after upload"
    );
    if let Err(e) = convert_and_store_package_schemas(
        state.package_store.as_ref(),
        &package_name,
        &package_version,
        fhir_version,
    )
    .await
    {
        warn!(
            package = %package_name,
            version = %package_version,
            error = %e,
            "failed to convert schemas for uploaded package"
        );
    }

    // Reload canonical registry to make resources available
    info!("Reloading canonical registry after package upload");
    rebuild_from_config_async(&state.config)
        .await
        .map_err(PackageError::RegistryReload)?;

    // Invalidate ModelProvider caches to force reload of new schemas
    info!("Invalidating ModelProvider schema caches");
    state.model_provider.invalidate_schema_caches();

    info!(
        package = %package_name,
        version = %package_version,
        resources = count,
        "package upload completed successfully"
    );

    Ok(Json(UploadResponse {
        message: format!("Successfully loaded {} resources", count),
        resources_loaded: count,
    }))
}

/// Extract tar.gz archive and load FHIR resources into canonical manager.
///
/// Returns (package_name, package_version, resource_count)
async fn extract_and_load_resources(
    path: &std::path::Path,
    package_store: &PostgresPackageStore,
    fhir_version: &str,
) -> Result<(String, String, usize), PackageError> {
    info!("Extracting package from path: {:?}", path);

    let file = std::fs::File::open(path)?;
    let decompressor = GzDecoder::new(file);
    let mut archive = Archive::new(decompressor);

    let mut package_json: Option<serde_json::Value> = None;
    let mut resources: Vec<(String, serde_json::Value)> = Vec::new();

    // First pass: extract all JSON files
    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let entry_path = entry.path()?.to_path_buf();

        // Process only JSON files
        if entry_path.extension().and_then(|s| s.to_str()) == Some("json") {
            let mut contents = String::new();
            entry.read_to_string(&mut contents)?;

            match serde_json::from_str::<serde_json::Value>(&contents) {
                Ok(json) => {
                    // Check if this is package.json (NPM package descriptor)
                    let file_name = entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");

                    if file_name == "package.json" {
                        package_json = Some(json.clone());
                        info!("Found package.json");
                    }

                    // Check if this is a FHIR resource (has resourceType)
                    if let Some(resource_type) = json.get("resourceType").and_then(|v| v.as_str()) {
                        info!(
                            resource_type = %resource_type,
                            file = %file_name,
                            "extracted FHIR resource"
                        );
                        resources.push((resource_type.to_string(), json));
                    }
                }
                Err(e) => {
                    warn!("Skipping invalid JSON file {:?}: {}", entry_path, e);
                }
            }
        }
    }

    // Extract package metadata
    let (package_name, package_version) = if let Some(pkg_json) = &package_json {
        let name = pkg_json
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PackageError::InvalidPackage("missing 'name' in package.json".into()))?
            .to_string();

        let version = pkg_json
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PackageError::InvalidPackage("missing 'version' in package.json".into())
            })?
            .to_string();

        (name, version)
    } else {
        return Err(PackageError::InvalidPackage(
            "package.json not found in archive".into(),
        ));
    };

    let resource_count = resources.len();

    info!(
        package = %package_name,
        version = %package_version,
        resources = resource_count,
        "extracted package metadata and resources"
    );

    // Load resources into PostgreSQL via PostgresPackageStore
    package_store
        .load_from_embedded(&package_name, &package_version, fhir_version, resources)
        .await
        .map_err(|e| PackageError::LoadFailed(format!("failed to load resources: {}", e)))?;

    info!(
        package = %package_name,
        version = %package_version,
        resources = resource_count,
        "successfully loaded package into database"
    );

    Ok((package_name, package_version, resource_count))
}

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("Multipart form error: {0}")]
    Multipart(#[from] axum::extract::multipart::MultipartError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Package file not provided in multipart form data")]
    MissingPackage,

    #[error("Invalid package: {0}")]
    InvalidPackage(String),

    #[error("Failed to load package resources: {0}")]
    LoadFailed(String),

    #[error("Failed to reload canonical registry: {0}")]
    RegistryReload(String),
}

impl IntoResponse for PackageError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            PackageError::Multipart(_)
            | PackageError::MissingPackage
            | PackageError::InvalidPackage(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PackageError::Io(_) | PackageError::LoadFailed(_) | PackageError::RegistryReload(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        error!("Package upload error: {}", message);

        let outcome = OperationOutcome {
            resource_type: "OperationOutcome",
            issue: vec![octofhir_api::OperationOutcomeIssue {
                severity: "error",
                code: "processing",
                diagnostics: Some(message),
            }],
        };

        (status, Json(outcome)).into_response()
    }
}

/// Creates the canonical package management routes.
///
/// These routes require CanonicalPackageState via `FromRef`.
///
/// # Type Parameters
///
/// - `S`: Application state that provides `CanonicalPackageState` via `FromRef`.
pub fn canonical_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    CanonicalPackageState: axum::extract::FromRef<S>,
{
    Router::new().route("/canonical/$upload", post(upload_package))
}
