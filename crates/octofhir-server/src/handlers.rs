use crate::mapping::{IdPolicy, envelope_from_json, json_from_envelope};
use axum::response::Response;
use axum::{
    Json,
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use include_dir::{Dir, include_dir};
use mime_guess::MimeGuess;
use octofhir_api::ApiError;
use octofhir_core::{CoreError, ResourceType};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Serialize)]
pub struct HealthResponse<'a> {
    status: &'a str,
}

// New API response types for /api/* endpoints
#[derive(Serialize)]
pub struct ApiHealthResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
}

#[derive(Serialize)]
pub struct BuildInfoResponse {
    #[serde(rename = "serverVersion")]
    server_version: String,
    commit: String,
    #[serde(rename = "commitTimestamp")]
    commit_timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "uiVersion")]
    ui_version: Option<String>,
}

pub async fn root() -> impl IntoResponse {
    let body = json!({
        "service": "OctoFHIR Server",
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    });
    (StatusCode::OK, Json(body))
}

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" }))
}

pub async fn readyz() -> impl IntoResponse {
    // In future, perform checks for DB/connectivity etc.
    (StatusCode::OK, Json(HealthResponse { status: "ready" }))
}

pub async fn metadata(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    use octofhir_api::{CapabilityStatementBuilder, SearchParam};

    // Build base CapabilityStatement per spec
    // Determine FHIR version from app state (from config)
    let fhir_version_str = state.fhir_version.clone();
    let mut builder = CapabilityStatementBuilder::new_json_r4b()
        .status("active")
        .kind("instance")
        .add_format("application/fhir+json")
        .add_format("application/json");

    // Apply FHIR version field
    builder = builder.fhir_version(match fhir_version_str.as_str() {
        "R4" | "4.0.1" => "4.0.1",
        "R5" | "5.0.0" => "5.0.0",
        "R6" | "6.0.0" => "6.0.0",
        _ => "4.3.0",
    });

    // Reflect capabilities from canonical manager search parameters when available
    // MVP resources advertised
    let resource_types = ["Patient", "Observation"]; // MVP scope

    // Optionally augment with canonical manager data when available
    let manager = crate::canonical::get_manager();
    for rt in resource_types.iter() {
        // Start with manager-provided params if any; then extend with our known supported params
        let mut mapped: Vec<SearchParam> = if let Some(mgr) = &manager {
            match mgr.get_search_parameters(rt).await {
                Ok(params) if !params.is_empty() => params
                    .into_iter()
                    .map(|p| SearchParam {
                        name: p.code,
                        type_: p.type_field,
                        documentation: p.description,
                    })
                    .collect(),
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        // Common params per FHIR spec and manager-provided params from packages (no hardcoded resource params)
        mapped.extend(octofhir_api::common_search_params());

        // Profiles for resource (StructureDefinition.type == rt)
        let mut base_profile: Option<String> = None;
        let mut supported_profiles: Vec<String> = Vec::new();
        if let Some(mgr) = &manager {
            let query = octofhir_canonical_manager::search::SearchQuery {
                text: None,
                resource_types: vec!["StructureDefinition".to_string()],
                packages: vec![],
                canonical_pattern: None,
                version_constraints: vec![],
                limit: Some(2000),
                offset: Some(0),
            };
            if let Ok(results) = mgr.search_engine().search(&query).await {
                for rm in results.resources {
                    let content = &rm.resource.content;
                    if content.get("type").and_then(|v| v.as_str()) == Some(rt)
                        && let Some(url) = content.get("url").and_then(|v| v.as_str())
                    {
                        match content.get("derivation").and_then(|v| v.as_str()) {
                            Some("specialization") => base_profile = Some(url.to_string()),
                            Some("constraint") => supported_profiles.push(url.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }

        let resource = octofhir_api::CapabilityStatementRestResource::new(*rt)
            .with_interactions(&["read", "vread", "search-type", "create", "update", "delete"][..])
            .with_search_params(mapped)
            .with_profile(base_profile)
            .with_supported_profiles(supported_profiles);
        builder = builder.add_resource_struct(resource);
    }

    let cs = builder.build();

    // Add software section consistent with spec
    // Insert via serde_json patch to avoid remodeling types
    let mut body = serde_json::to_value(&cs).unwrap_or_else(|_| {
        json!({
            "resourceType": "CapabilityStatement",
            "status": "active",
            "kind": "instance",
            "fhirVersion": "4.3.0",
            "format": ["application/fhir+json"],
            "rest": [{"mode": "server"}]
        })
    });
    body["software"] = json!({ "name": "OctoFHIR Server", "version": env!("CARGO_PKG_VERSION") });

    // Reflect loaded canonical packages via an extension (Phase 8)
    if let Some(pkgs) = crate::canonical::with_registry(|r| {
        r.list()
            .iter()
            .map(|p| {
                json!({
                    "url": "urn:octofhir:loaded-package",
                    "extension": [
                        {"url": "id", "valueString": p.id},
                        {"url": "version", "valueString": p.version.clone().unwrap_or_default()},
                        {"url": "path", "valueString": p.path.clone().unwrap_or_default()},
                    ]
                })
            })
            .collect::<Vec<_>>()
    }) {
        if let Some(ext) = body.get_mut("extension") {
            if let Some(arr) = ext.as_array_mut() {
                arr.extend(pkgs);
            }
        } else {
            body["extension"] = Value::Array(pkgs);
        }
    }

    (StatusCode::OK, Json(body))
}

// ---- CRUD & Search placeholders ----

pub async fn create_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.create", resource_type = %resource_type);
    let _g = span.enter();
    if let Err(e) = crate::validation::validate_resource(&resource_type, &payload) {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    // Handle conditional create (If-None-Exist)
    if let Some(condition) = headers.get("If-None-Exist")
        && let Ok(condition_str) = condition.to_str()
    {
        let rt = match resource_type.parse::<ResourceType>() {
            Ok(rt) => rt,
            Err(_) => {
                return Err(ApiError::bad_request(format!(
                    "Unknown resourceType '{resource_type}'"
                )));
            }
        };
        // Execute search with If-None-Exist criteria
        let search_cfg = &state.search_cfg;
        match octofhir_search::SearchEngine::execute(
            &state.storage,
            rt.clone(),
            condition_str,
            search_cfg,
        )
        .await
        {
            Ok(result) => match result.resources.len() {
                0 => {
                    // No match - proceed with create
                }
                1 => {
                    // One match - return existing resource with 200
                    let existing = &result.resources[0];
                    let version_id = existing.meta.version_id.as_deref().unwrap_or("1");
                    let mut response_headers = HeaderMap::new();

                    // ETag
                    let etag = format!("W/\"{}\"", version_id);
                    if let Ok(val) = header::HeaderValue::from_str(&etag) {
                        response_headers.insert(header::ETAG, val);
                    }

                    // Last-Modified
                    let last_modified = httpdate::fmt_http_date(
                        std::time::UNIX_EPOCH
                            + std::time::Duration::from_secs(
                                existing.meta.last_updated.timestamp() as u64,
                            ),
                    );
                    if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                        response_headers.insert(header::LAST_MODIFIED, val);
                    }

                    // Content-Location
                    let content_loc = format!("/{}/{}", resource_type, existing.id);
                    if let Ok(val) = header::HeaderValue::from_str(&content_loc) {
                        response_headers.insert(header::CONTENT_LOCATION, val);
                    }

                    // Content-Type
                    response_headers.insert(
                        header::CONTENT_TYPE,
                        header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
                    );

                    let body = json_from_envelope(existing);
                    return Ok((StatusCode::OK, response_headers, Json(body)));
                }
                _ => {
                    // Multiple matches - return 412 Precondition Failed
                    return Err(ApiError::precondition_failed(
                        "Multiple resources match If-None-Exist criteria",
                    ));
                }
            },
            Err(e) => {
                tracing::warn!("If-None-Exist search failed: {}, proceeding with create", e);
                // Proceed with create if search fails
            }
        }
    }

    // Parse Prefer header for return preference
    let prefer_return = headers
        .get("Prefer")
        .and_then(|h| h.to_str().ok())
        .map(parse_prefer_return);

    // Map JSON -> envelope and insert into storage
    match envelope_from_json(&resource_type, &payload, IdPolicy::Create) {
        Err(err) => {
            tracing::warn!(error.kind = "invalid-payload", message = %err);
            Err(ApiError::bad_request(err))
        }
        Ok(env) => {
            let id = env.id.clone();
            let version_id = env
                .meta
                .version_id
                .clone()
                .unwrap_or_else(|| "1".to_string());
            let last_updated = env.meta.last_updated.clone();
            let rt = env.resource_type.clone();
            let body = json_from_envelope(&env);
            match state.storage.insert(&rt, env).await {
                Ok(()) => {
                    let mut response_headers = HeaderMap::new();

                    // Location header
                    let loc = format!("/{resource_type}/{id}");
                    if let Ok(val) = header::HeaderValue::from_str(&loc) {
                        response_headers.insert(header::LOCATION, val);
                    }

                    // ETag
                    let etag = format!("W/\"{}\"", version_id);
                    if let Ok(val) = header::HeaderValue::from_str(&etag) {
                        response_headers.insert(header::ETAG, val);
                    }

                    // Last-Modified
                    let last_modified = httpdate::fmt_http_date(
                        std::time::UNIX_EPOCH
                            + std::time::Duration::from_secs(last_updated.timestamp() as u64),
                    );
                    if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                        response_headers.insert(header::LAST_MODIFIED, val);
                    }

                    // Content-Type
                    response_headers.insert(
                        header::CONTENT_TYPE,
                        header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
                    );

                    // Handle Prefer return preference
                    match prefer_return {
                        Some(PreferReturn::Minimal) => {
                            Ok((StatusCode::CREATED, response_headers, Json(json!({}))))
                        }
                        Some(PreferReturn::OperationOutcome) => {
                            let outcome = json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!("Resource created: {}/{}", resource_type, id)
                                }]
                            });
                            Ok((StatusCode::CREATED, response_headers, Json(outcome)))
                        }
                        _ => {
                            // Default: return representation
                            Ok((StatusCode::CREATED, response_headers, Json(body)))
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error.kind = "create-failed", message = %e);
                    Err(map_core_error(e))
                }
            }
        }
    }
}

/// Prefer header return preference
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferReturn {
    Minimal,
    Representation,
    OperationOutcome,
}

/// Parse Prefer header for return preference
fn parse_prefer_return(header: &str) -> PreferReturn {
    if header.contains("return=minimal") {
        PreferReturn::Minimal
    } else if header.contains("return=OperationOutcome") {
        PreferReturn::OperationOutcome
    } else {
        PreferReturn::Representation
    }
}

pub async fn read_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.read", resource_type = %resource_type, id = %id);
    let _g = span.enter();
    let rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };
    match state.storage.get(&rt, &id).await {
        Ok(Some(env)) => {
            // Get version_id for ETag (default to "1" if not set)
            let version_id = env.meta.version_id.as_deref().unwrap_or("1");

            // Check If-None-Match (conditional read - return 304 if version matches)
            if octofhir_api::check_if_none_match(&headers, version_id) {
                return Ok((StatusCode::NOT_MODIFIED, HeaderMap::new(), Json(json!({}))));
            }

            // Check If-Modified-Since (conditional read)
            if let Some(if_modified_since) = headers.get(header::IF_MODIFIED_SINCE)
                && let Ok(since_str) = if_modified_since.to_str()
                && let Ok(since) = httpdate::parse_http_date(since_str)
            {
                let last_updated_ts = std::time::UNIX_EPOCH
                    + std::time::Duration::from_secs(env.meta.last_updated.timestamp() as u64);
                if last_updated_ts <= since {
                    return Ok((StatusCode::NOT_MODIFIED, HeaderMap::new(), Json(json!({}))));
                }
            }

            // Build response headers
            let mut response_headers = HeaderMap::new();

            // ETag: W/"version_id"
            let etag = format!("W/\"{}\"", version_id);
            if let Ok(val) = header::HeaderValue::from_str(&etag) {
                response_headers.insert(header::ETAG, val);
            }

            // Last-Modified: HTTP date format
            let last_modified = httpdate::fmt_http_date(
                std::time::UNIX_EPOCH
                    + std::time::Duration::from_secs(env.meta.last_updated.timestamp() as u64),
            );
            if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                response_headers.insert(header::LAST_MODIFIED, val);
            }

            // Content-Type
            response_headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
            );

            let body = json_from_envelope(&env);
            Ok((StatusCode::OK, response_headers, Json(body)))
        }
        Ok(None) => Err(ApiError::not_found(format!(
            "{resource_type} with id '{id}' not found"
        ))),
        Err(e) => Err(map_core_error(e)),
    }
}

/// GET /[type]/[id]/_history/[vid] - Read a specific version of a resource
pub async fn vread_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id, version_id)): Path<(String, String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.vread", resource_type = %resource_type, id = %id, version_id = %version_id);
    let _g = span.enter();
    let rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };
    match state.storage.get(&rt, &id).await {
        Ok(Some(env)) => {
            // Get current version_id (default to "1" if not set)
            let current_version = env.meta.version_id.as_deref().unwrap_or("1");

            // Check if requested version matches current version
            // Note: In-memory storage doesn't support historical versions yet
            if current_version != version_id {
                return Err(ApiError::not_found(format!(
                    "{resource_type}/{id}/_history/{version_id} not found"
                )));
            }

            // Build response headers
            let mut response_headers = HeaderMap::new();

            // ETag: W/"version_id"
            let etag = format!("W/\"{}\"", current_version);
            if let Ok(val) = header::HeaderValue::from_str(&etag) {
                response_headers.insert(header::ETAG, val);
            }

            // Last-Modified: HTTP date format
            let last_modified = httpdate::fmt_http_date(
                std::time::UNIX_EPOCH
                    + std::time::Duration::from_secs(env.meta.last_updated.timestamp() as u64),
            );
            if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                response_headers.insert(header::LAST_MODIFIED, val);
            }

            // Content-Type
            response_headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
            );

            let body = json_from_envelope(&env);
            Ok((StatusCode::OK, response_headers, Json(body)))
        }
        Ok(None) => Err(ApiError::not_found(format!(
            "{resource_type}/{id}/_history/{version_id} not found"
        ))),
        Err(e) => Err(map_core_error(e)),
    }
}

pub async fn update_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.update", resource_type = %resource_type, id = %id);
    let _g = span.enter();
    if let Err(e) = crate::validation::validate_resource(&resource_type, &payload) {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }
    let rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };

    // Extract If-Match header for version checking
    let if_match = headers
        .get(header::IF_MATCH)
        .and_then(|h| h.to_str().ok())
        .map(parse_etag);

    // Parse Prefer header for return preference
    let prefer_return = headers
        .get("Prefer")
        .and_then(|h| h.to_str().ok())
        .map(parse_prefer_return);

    // Check if resource exists
    let existing = state.storage.get(&rt, &id).await;

    match existing {
        Ok(Some(existing_env)) => {
            // Resource exists - check If-Match if provided
            if let Some(expected_version) = &if_match {
                let current_version = existing_env.meta.version_id.as_deref().unwrap_or("1");
                if expected_version != current_version {
                    return Err(ApiError::conflict(format!(
                        "Version conflict: expected {}, but current is {}",
                        expected_version, current_version
                    )));
                }
            }

            // Update existing resource
            match envelope_from_json(
                &resource_type,
                &payload,
                IdPolicy::Update {
                    path_id: id.clone(),
                },
            ) {
                Err(err) => Err(ApiError::bad_request(err)),
                Ok(env) => match state.storage.update(&rt, &id, env.clone()).await {
                    Ok(_) => {
                        let version_id = env
                            .meta
                            .version_id
                            .clone()
                            .unwrap_or_else(|| "1".to_string());
                        let last_updated = env.meta.last_updated.clone();
                        let body = json_from_envelope(&env);

                        let mut response_headers = HeaderMap::new();

                        // ETag
                        let etag = format!("W/\"{}\"", version_id);
                        if let Ok(val) = header::HeaderValue::from_str(&etag) {
                            response_headers.insert(header::ETAG, val);
                        }

                        // Last-Modified
                        let last_modified = httpdate::fmt_http_date(
                            std::time::UNIX_EPOCH
                                + std::time::Duration::from_secs(last_updated.timestamp() as u64),
                        );
                        if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                            response_headers.insert(header::LAST_MODIFIED, val);
                        }

                        // Content-Type
                        response_headers.insert(
                            header::CONTENT_TYPE,
                            header::HeaderValue::from_static(
                                "application/fhir+json; charset=utf-8",
                            ),
                        );

                        // Handle Prefer return preference
                        match prefer_return {
                            Some(PreferReturn::Minimal) => {
                                Ok((StatusCode::OK, response_headers, Json(json!({}))))
                            }
                            Some(PreferReturn::OperationOutcome) => {
                                let outcome = json!({
                                    "resourceType": "OperationOutcome",
                                    "issue": [{
                                        "severity": "information",
                                        "code": "informational",
                                        "diagnostics": format!("Resource updated: {}/{}", resource_type, id)
                                    }]
                                });
                                Ok((StatusCode::OK, response_headers, Json(outcome)))
                            }
                            _ => Ok((StatusCode::OK, response_headers, Json(body))),
                        }
                    }
                    Err(e) => Err(map_core_error(e)),
                },
            }
        }
        Ok(None) => {
            // Resource doesn't exist - create-on-update
            if if_match.is_some() {
                // If-Match provided but resource doesn't exist
                return Err(ApiError::precondition_failed(
                    "Resource does not exist but If-Match was provided",
                ));
            }

            // Create new resource with provided ID
            match envelope_from_json(
                &resource_type,
                &payload,
                IdPolicy::Update {
                    path_id: id.clone(),
                },
            ) {
                Err(err) => Err(ApiError::bad_request(err)),
                Ok(env) => match state.storage.insert(&rt, env.clone()).await {
                    Ok(()) => {
                        let version_id = env
                            .meta
                            .version_id
                            .clone()
                            .unwrap_or_else(|| "1".to_string());
                        let last_updated = env.meta.last_updated.clone();
                        let body = json_from_envelope(&env);

                        let mut response_headers = HeaderMap::new();

                        // Location header (for create)
                        let loc = format!("/{resource_type}/{id}");
                        if let Ok(val) = header::HeaderValue::from_str(&loc) {
                            response_headers.insert(header::LOCATION, val);
                        }

                        // ETag
                        let etag = format!("W/\"{}\"", version_id);
                        if let Ok(val) = header::HeaderValue::from_str(&etag) {
                            response_headers.insert(header::ETAG, val);
                        }

                        // Last-Modified
                        let last_modified = httpdate::fmt_http_date(
                            std::time::UNIX_EPOCH
                                + std::time::Duration::from_secs(last_updated.timestamp() as u64),
                        );
                        if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                            response_headers.insert(header::LAST_MODIFIED, val);
                        }

                        // Content-Type
                        response_headers.insert(
                            header::CONTENT_TYPE,
                            header::HeaderValue::from_static(
                                "application/fhir+json; charset=utf-8",
                            ),
                        );

                        // Handle Prefer return preference
                        match prefer_return {
                            Some(PreferReturn::Minimal) => {
                                Ok((StatusCode::CREATED, response_headers, Json(json!({}))))
                            }
                            Some(PreferReturn::OperationOutcome) => {
                                let outcome = json!({
                                    "resourceType": "OperationOutcome",
                                    "issue": [{
                                        "severity": "information",
                                        "code": "informational",
                                        "diagnostics": format!("Resource created: {}/{}", resource_type, id)
                                    }]
                                });
                                Ok((StatusCode::CREATED, response_headers, Json(outcome)))
                            }
                            _ => Ok((StatusCode::CREATED, response_headers, Json(body))),
                        }
                    }
                    Err(e) => Err(map_core_error(e)),
                },
            }
        }
        Err(e) => Err(map_core_error(e)),
    }
}

/// Parse ETag header value (W/"version" or "version")
fn parse_etag(header: &str) -> String {
    let trimmed = header.trim();
    if trimmed.starts_with("W/\"") && trimmed.ends_with('"') {
        trimmed[3..trimmed.len() - 1].to_string()
    } else if trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

/// PUT /[type]?[search params] - Conditional update based on search criteria
pub async fn conditional_update_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.conditional_update", resource_type = %resource_type);
    let _g = span.enter();

    // Validate resource type
    let rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };

    // Conditional update requires search parameters
    let raw_q = raw.unwrap_or_default();
    if raw_q.is_empty() {
        return Err(ApiError::bad_request(
            "Conditional update requires search parameters",
        ));
    }

    // Validate payload
    if let Err(e) = crate::validation::validate_resource(&resource_type, &payload) {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    // Extract If-Match header for version checking
    let if_match = headers
        .get(header::IF_MATCH)
        .and_then(|h| h.to_str().ok())
        .map(parse_etag);

    // Parse Prefer header for return preference
    let prefer_return = headers
        .get("Prefer")
        .and_then(|h| h.to_str().ok())
        .map(parse_prefer_return);

    // Search for matching resources
    let cfg = &state.search_cfg;
    let search_result =
        octofhir_search::SearchEngine::execute(&state.storage, rt.clone(), &raw_q, cfg).await;

    match search_result {
        Ok(result) => match result.resources.len() {
            0 => {
                // No match - create new resource (201)
                match envelope_from_json(&resource_type, &payload, IdPolicy::Create) {
                    Err(err) => Err(ApiError::bad_request(err)),
                    Ok(env) => {
                        let id = env.id.clone();
                        let version_id = env
                            .meta
                            .version_id
                            .clone()
                            .unwrap_or_else(|| "1".to_string());
                        let last_updated = env.meta.last_updated.clone();
                        let body = json_from_envelope(&env);

                        match state.storage.insert(&rt, env).await {
                            Ok(()) => {
                                let mut response_headers = HeaderMap::new();

                                // Location header
                                let loc = format!("/{resource_type}/{id}");
                                if let Ok(val) = header::HeaderValue::from_str(&loc) {
                                    response_headers.insert(header::LOCATION, val);
                                }

                                // ETag
                                let etag = format!("W/\"{}\"", version_id);
                                if let Ok(val) = header::HeaderValue::from_str(&etag) {
                                    response_headers.insert(header::ETAG, val);
                                }

                                // Last-Modified
                                let last_modified = httpdate::fmt_http_date(
                                    std::time::UNIX_EPOCH
                                        + std::time::Duration::from_secs(
                                            last_updated.timestamp() as u64
                                        ),
                                );
                                if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                                    response_headers.insert(header::LAST_MODIFIED, val);
                                }

                                // Content-Type
                                response_headers.insert(
                                    header::CONTENT_TYPE,
                                    header::HeaderValue::from_static(
                                        "application/fhir+json; charset=utf-8",
                                    ),
                                );

                                // Handle Prefer return preference
                                match prefer_return {
                                    Some(PreferReturn::Minimal) => {
                                        Ok((StatusCode::CREATED, response_headers, Json(json!({}))))
                                    }
                                    Some(PreferReturn::OperationOutcome) => {
                                        let outcome = json!({
                                            "resourceType": "OperationOutcome",
                                            "issue": [{
                                                "severity": "information",
                                                "code": "informational",
                                                "diagnostics": format!("Resource created: {}/{}", resource_type, id)
                                            }]
                                        });
                                        Ok((StatusCode::CREATED, response_headers, Json(outcome)))
                                    }
                                    _ => Ok((StatusCode::CREATED, response_headers, Json(body))),
                                }
                            }
                            Err(e) => Err(map_core_error(e)),
                        }
                    }
                }
            }
            1 => {
                // One match - update that resource (200)
                let existing = &result.resources[0];
                let id = existing.id.clone();

                // Check If-Match if provided
                if let Some(expected_version) = &if_match {
                    let current_version = existing.meta.version_id.as_deref().unwrap_or("1");
                    if expected_version != current_version {
                        return Err(ApiError::conflict(format!(
                            "Version conflict: expected {}, but current is {}",
                            expected_version, current_version
                        )));
                    }
                }

                // Update the matched resource
                match envelope_from_json(
                    &resource_type,
                    &payload,
                    IdPolicy::Update {
                        path_id: id.clone(),
                    },
                ) {
                    Err(err) => Err(ApiError::bad_request(err)),
                    Ok(env) => match state.storage.update(&rt, &id, env.clone()).await {
                        Ok(_) => {
                            let version_id = env
                                .meta
                                .version_id
                                .clone()
                                .unwrap_or_else(|| "1".to_string());
                            let last_updated = env.meta.last_updated.clone();
                            let body = json_from_envelope(&env);

                            let mut response_headers = HeaderMap::new();

                            // Content-Location header
                            let content_loc = format!("/{resource_type}/{id}");
                            if let Ok(val) = header::HeaderValue::from_str(&content_loc) {
                                response_headers.insert(header::CONTENT_LOCATION, val);
                            }

                            // ETag
                            let etag = format!("W/\"{}\"", version_id);
                            if let Ok(val) = header::HeaderValue::from_str(&etag) {
                                response_headers.insert(header::ETAG, val);
                            }

                            // Last-Modified
                            let last_modified = httpdate::fmt_http_date(
                                std::time::UNIX_EPOCH
                                    + std::time::Duration::from_secs(
                                        last_updated.timestamp() as u64
                                    ),
                            );
                            if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                                response_headers.insert(header::LAST_MODIFIED, val);
                            }

                            // Content-Type
                            response_headers.insert(
                                header::CONTENT_TYPE,
                                header::HeaderValue::from_static(
                                    "application/fhir+json; charset=utf-8",
                                ),
                            );

                            // Handle Prefer return preference
                            match prefer_return {
                                Some(PreferReturn::Minimal) => {
                                    Ok((StatusCode::OK, response_headers, Json(json!({}))))
                                }
                                Some(PreferReturn::OperationOutcome) => {
                                    let outcome = json!({
                                        "resourceType": "OperationOutcome",
                                        "issue": [{
                                            "severity": "information",
                                            "code": "informational",
                                            "diagnostics": format!("Resource updated: {}/{}", resource_type, id)
                                        }]
                                    });
                                    Ok((StatusCode::OK, response_headers, Json(outcome)))
                                }
                                _ => Ok((StatusCode::OK, response_headers, Json(body))),
                            }
                        }
                        Err(e) => Err(map_core_error(e)),
                    },
                }
            }
            _ => {
                // Multiple matches - return 412 Precondition Failed
                Err(ApiError::precondition_failed(
                    "Multiple resources match the search criteria",
                ))
            }
        },
        Err(e) => Err(ApiError::bad_request(format!("Search failed: {}", e))),
    }
}

pub async fn delete_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.delete", resource_type = %resource_type, id = %id);
    let _g = span.enter();
    let rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };
    match state.storage.delete(&rt, &id).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(map_core_error(e)),
    }
}

pub async fn search_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.search", resource_type = %resource_type);
    let _g = span.enter();
    // Pagination is enforced by search engine config; no extra handling here.

    // Execute search using search engine config (respects allow-lists)
    let rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };

    let raw_q = raw.unwrap_or_default();
    let cfg = &state.search_cfg;
    match octofhir_search::SearchEngine::execute(&state.storage, rt.clone(), &raw_q, cfg).await {
        Ok(result) => {
            // Strip _count/_offset from query suffix
            let suffix = if raw_q.is_empty() {
                None
            } else {
                let filtered: Vec<_> = raw_q
                    .split('&')
                    .filter(|kv| !kv.starts_with("_count=") && !kv.starts_with("_offset="))
                    .collect();
                let s = filtered.join("&");
                Some(s)
            };

            let resources_json: Vec<Value> =
                result.resources.iter().map(json_from_envelope).collect();
            let bundle = octofhir_api::bundle_from_search(
                result.total,
                resources_json,
                "",
                &resource_type,
                result.offset,
                result.count,
                suffix.as_deref(),
            );
            let val =
                serde_json::to_value(bundle).map_err(|e| ApiError::internal(e.to_string()))?;
            Ok((StatusCode::OK, Json(val)))
        }
        Err(e) => Err(ApiError::bad_request(e.to_string())),
    }
}

// Removed: unified ApiError is used for error responses now

// Special handler for browsers requesting /favicon.ico
pub async fn favicon() -> impl IntoResponse {
    // Minimal, fast response to avoid 404 noise in logs. Browsers accept 204 with no body.
    // Also set a caching header to reduce repeated requests.
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=86400"),
    );
    (StatusCode::NO_CONTENT, headers)
}

// ---- New API endpoints for UI ----

/// GET /api/health - Enhanced health check with system status
pub async fn api_health(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    let mut status = "ok".to_string();
    let mut details: Option<String> = None;

    // Check storage connectivity by trying to get a count
    let _storage_count = state.storage.count().await;
    // Storage is working if we reach here without panic

    // Check canonical manager status
    if let Some(manager) = crate::canonical::get_manager() {
        match manager.storage().list_packages().await {
            Err(e) => {
                status = "degraded".to_string();
                details = Some(format!("Canonical manager issue: {}", e));
            }
            Ok(packages) => {
                if packages.is_empty() {
                    // This could be degraded but might be normal for fresh installs
                    tracing::debug!("No canonical packages loaded");
                }
            }
        }
    }

    let response = ApiHealthResponse { status, details };
    (StatusCode::OK, Json(response))
}

/// GET /api/build-info - Build and version information
pub async fn api_build_info() -> impl IntoResponse {
    let response = BuildInfoResponse {
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        commit: option_env!("GIT_COMMIT").unwrap_or("unknown").to_string(),
        commit_timestamp: option_env!("GIT_COMMIT_TIMESTAMP")
            .unwrap_or("unknown")
            .to_string(),
        ui_version: Some("1.0.0".to_string()), // Will be updated when we build UI
    };
    (StatusCode::OK, Json(response))
}

/// GET /api/resource-types - List available FHIR resource types
pub async fn api_resource_types() -> impl IntoResponse {
    let mut resource_types = Vec::new();

    // Try to get resource types from canonical manager
    if let Some(manager) = crate::canonical::get_manager() {
        let query = octofhir_canonical_manager::search::SearchQuery {
            text: None,
            resource_types: vec!["StructureDefinition".to_string()],
            packages: vec![],
            canonical_pattern: None,
            version_constraints: vec![],
            limit: Some(1000),
            offset: Some(0),
        };

        match manager.search_engine().search(&query).await {
            Ok(results) => {
                for resource_match in results.resources {
                    let content = &resource_match.resource.content;

                    // Only include base resource types (not profiles)
                    if let (Some(kind), Some(derivation), Some(resource_type)) = (
                        content.get("kind").and_then(|v| v.as_str()),
                        content.get("derivation").and_then(|v| v.as_str()),
                        content.get("type").and_then(|v| v.as_str()),
                    ) && kind == "resource"
                        && derivation == "specialization"
                        && !resource_types.contains(&resource_type.to_string())
                    {
                        resource_types.push(resource_type.to_string());
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to query canonical manager for resource types: {}",
                    e
                );
            }
        }
    }

    // Fallback to common FHIR resource types if canonical manager fails
    if resource_types.is_empty() {
        resource_types = vec![
            "Patient".to_string(),
            "Practitioner".to_string(),
            "Organization".to_string(),
            "Observation".to_string(),
            "DiagnosticReport".to_string(),
            "Medication".to_string(),
            "MedicationRequest".to_string(),
            "Procedure".to_string(),
            "Condition".to_string(),
            "Encounter".to_string(),
            "AllergyIntolerance".to_string(),
            "Immunization".to_string(),
        ];
    }

    // Sort alphabetically for consistency
    resource_types.sort();

    (StatusCode::OK, Json(resource_types))
}

// ---- Error mapping helpers ----
fn map_core_error(e: CoreError) -> ApiError {
    match e {
        CoreError::ResourceNotFound { resource_type, id } => {
            ApiError::not_found(format!("{resource_type} with id '{id}' not found"))
        }
        CoreError::ResourceConflict { resource_type, id } => {
            ApiError::conflict(format!("{resource_type} with id '{id}' already exists"))
        }
        CoreError::InvalidResource { message } => ApiError::bad_request(message),
        CoreError::InvalidResourceType(s) => {
            ApiError::bad_request(format!("Invalid resource type: {s}"))
        }
        other => ApiError::internal(other.to_string()),
    }
}

// ---- Embedded UI handlers ----
static UI_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../ui/dist");

fn ui_index_response() -> Response {
    if let Some(index) = UI_DIR.get_file("index.html") {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/html; charset=utf-8"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache"),
        );
        (StatusCode::OK, headers, index.contents().to_vec()).into_response()
    } else {
        (StatusCode::NOT_FOUND, "UI not bundled").into_response()
    }
}

pub async fn ui_index() -> Response {
    ui_index_response()
}

pub async fn ui_static(Path(path): Path<String>) -> Response {
    let rel = path.trim_start_matches('/');

    // Serve index for empty or root path
    if rel.is_empty() {
        return ui_index_response();
    }

    // Try to serve a static file from embedded dir
    if let Some(file) = UI_DIR.get_file(rel) {
        let mime = MimeGuess::from_path(rel).first_or_octet_stream();
        let mut headers = HeaderMap::new();
        if let Ok(hv) = header::HeaderValue::from_str(mime.as_ref()) {
            headers.insert(header::CONTENT_TYPE, hv);
        }
        // Cache immutable assets aggressively; Vite outputs hashed filenames
        headers.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
        return (StatusCode::OK, headers, file.contents().to_vec()).into_response();
    }

    // SPA fallback to index.html for client-side routes (no extension)
    if !rel.contains('.') {
        return ui_index_response();
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}
