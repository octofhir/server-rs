use crate::mapping::{IdPolicy, envelope_from_json, json_from_envelope};
use crate::patch::{apply_fhirpath_patch, apply_json_patch};
use axum::body::Bytes;
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

/// Query parameters for capabilities endpoint
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CapabilitiesParams {
    /// Summary mode: true, text, data, count, false
    #[serde(rename = "_summary")]
    pub summary: Option<String>,
}

pub async fn metadata(
    State(state): State<crate::server::AppState>,
    Query(params): Query<CapabilitiesParams>,
) -> impl IntoResponse {
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
    // Core FHIR resources advertised (commonly used clinical and administrative resources)
    let resource_types = [
        // Foundation
        "CapabilityStatement",
        "OperationDefinition",
        "StructureDefinition",
        "ValueSet",
        "CodeSystem",
        "Bundle",
        // Clinical
        "Patient",
        "Observation",
        "Condition",
        "Procedure",
        "DiagnosticReport",
        "MedicationRequest",
        "Medication",
        "AllergyIntolerance",
        "Immunization",
        "CarePlan",
        "CareTeam",
        "Goal",
        // Administrative
        "Practitioner",
        "PractitionerRole",
        "Organization",
        "Location",
        "Encounter",
        "EpisodeOfCare",
        "Appointment",
        "Schedule",
        "Slot",
        // Financial
        "Coverage",
        "Claim",
        "ClaimResponse",
        // Documents
        "DocumentReference",
        "Composition",
        "Binary",
    ];

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
            .with_interactions(
                &[
                    "read",
                    "vread",
                    "search-type",
                    "create",
                    "update",
                    "delete",
                    "history-instance",
                    "history-type",
                ][..],
            )
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

    // Add implementation section
    body["implementation"] = json!({
        "description": "OctoFHIR FHIR Server",
        "url": &state.base_url
    });

    // Add security section to rest
    if let Some(rest) = body.get_mut("rest")
        && let Some(rest_arr) = rest.as_array_mut()
        && let Some(rest_item) = rest_arr.first_mut()
    {
        rest_item["security"] = json!({
            "cors": true,
            "service": [{
                "coding": [{
                    "system": "http://terminology.hl7.org/CodeSystem/restful-security-service",
                    "code": "SMART-on-FHIR",
                    "display": "SMART-on-FHIR"
                }]
            }],
            "description": "OAuth2 using SMART-on-FHIR profile (when enabled)"
        });
    }

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

    // Handle _summary parameter
    let response = match params.summary.as_deref() {
        Some("true") => summarize_capability_statement(&body),
        Some("text") => text_summary_capability_statement(&body),
        Some("data") => data_summary_capability_statement(&body),
        Some("count") => count_summary_capability_statement(&body),
        _ => body, // "false" or no summary
    };

    (StatusCode::OK, Json(response))
}

/// Return minimal summary of CapabilityStatement (_summary=true)
fn summarize_capability_statement(cs: &Value) -> Value {
    json!({
        "resourceType": "CapabilityStatement",
        "status": cs["status"],
        "date": cs["date"],
        "fhirVersion": cs["fhirVersion"],
        "format": cs["format"],
        "rest": cs["rest"].as_array().map(|rest| {
            rest.iter().map(|r| {
                json!({
                    "mode": r["mode"],
                    "resource": r["resource"].as_array().map(|resources| {
                        resources.iter().map(|res| json!({"type": res["type"]})).collect::<Vec<_>>()
                    })
                })
            }).collect::<Vec<_>>()
        })
    })
}

/// Return text narrative only (_summary=text)
fn text_summary_capability_statement(cs: &Value) -> Value {
    json!({
        "resourceType": "CapabilityStatement",
        "text": cs.get("text").cloned().unwrap_or_else(|| json!({
            "status": "generated",
            "div": "<div xmlns=\"http://www.w3.org/1999/xhtml\"><p>OctoFHIR Server CapabilityStatement</p></div>"
        })),
        "status": cs["status"]
    })
}

/// Return data elements only, no text narrative (_summary=data)
fn data_summary_capability_statement(cs: &Value) -> Value {
    let mut result = cs.clone();
    if let Some(obj) = result.as_object_mut() {
        obj.remove("text");
    }
    result
}

/// Return count only (_summary=count)
fn count_summary_capability_statement(cs: &Value) -> Value {
    let resource_count = cs["rest"]
        .as_array()
        .and_then(|rest| rest.first())
        .and_then(|r| r["resource"].as_array())
        .map(|resources| resources.len())
        .unwrap_or(0);

    json!({
        "resourceType": "CapabilityStatement",
        "status": cs["status"],
        "total": resource_count
    })
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

// ---- History Query Parameters ----

/// Query parameters for history endpoints
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct HistoryQueryParams {
    /// Only include entries from after this time
    #[serde(rename = "_since")]
    pub since: Option<String>,
    /// Only include entries from before this time (point-in-time)
    #[serde(rename = "_at")]
    pub at: Option<String>,
    /// Maximum number of entries to return
    #[serde(rename = "_count")]
    pub count: Option<u32>,
    /// Number of entries to skip for pagination
    #[serde(rename = "__offset")]
    pub offset: Option<u32>,
}

/// Parse a FHIR instant/datetime string into OffsetDateTime
fn parse_fhir_instant(value: &str) -> Result<time::OffsetDateTime, ApiError> {
    use time::format_description::well_known::Rfc3339;
    use time::macros::format_description;

    // Try RFC3339 first (full instant with timezone)
    if let Ok(dt) = time::OffsetDateTime::parse(value, &Rfc3339) {
        return Ok(dt);
    }

    // Try datetime without timezone (assume UTC)
    let datetime_format = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    if let Ok(dt) = time::PrimitiveDateTime::parse(value, &datetime_format) {
        return Ok(dt.assume_utc());
    }

    // Try date only (assume start of day UTC)
    let date_format = format_description!("[year]-[month]-[day]");
    if let Ok(date) = time::Date::parse(value, &date_format) {
        return Ok(date.with_hms(0, 0, 0).unwrap().assume_utc());
    }

    Err(ApiError::bad_request(format!(
        "Invalid datetime format: {}. Expected FHIR instant format (e.g., 2023-01-15T10:30:00Z or 2023-01-15)",
        value
    )))
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

// ---- History Handlers ----

/// Instance history: GET /{type}/{id}/_history
pub async fn instance_history(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    Query(params): Query<HistoryQueryParams>,
) -> Result<impl IntoResponse, ApiError> {
    use octofhir_api::{HistoryBundleEntry, HistoryBundleMethod, bundle_from_history};
    use octofhir_storage::HistoryParams;
    use time::format_description::well_known::Rfc3339;

    let span =
        tracing::info_span!("fhir.history.instance", resource_type = %resource_type, id = %id);
    let _g = span.enter();

    // Parse resource type
    let rt: ResourceType = match resource_type.parse() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };

    // First check if resource exists
    let exists = state.storage.exists(&rt, &id).await;
    if !exists {
        // Check if it might be a deleted resource by trying to get it
        match state.storage.get(&rt, &id).await {
            Err(octofhir_core::CoreError::ResourceDeleted { .. }) => {
                // Resource was deleted, we can still show history
            }
            Ok(None) => {
                return Err(ApiError::not_found(format!(
                    "{resource_type}/{id} not found"
                )));
            }
            _ => {}
        }
    }

    // Build history params
    let mut history_params = HistoryParams::new();
    if let Some(ref since) = params.since {
        history_params.since = Some(parse_fhir_instant(since)?);
    }
    if let Some(ref at) = params.at {
        history_params.at = Some(parse_fhir_instant(at)?);
    }
    let count = params.count.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);
    history_params.count = Some(count);
    history_params.offset = Some(offset);

    // Get history from storage
    let result = state
        .storage
        .history(&resource_type, Some(&id), &history_params)
        .await
        .map_err(map_core_error)?;

    // Convert to bundle entries
    let entries: Vec<HistoryBundleEntry> = result
        .entries
        .into_iter()
        .map(|entry| {
            let method = match entry.method {
                octofhir_storage::HistoryMethod::Create => HistoryBundleMethod::Create,
                octofhir_storage::HistoryMethod::Update => HistoryBundleMethod::Update,
                octofhir_storage::HistoryMethod::Delete => HistoryBundleMethod::Delete,
            };
            HistoryBundleEntry {
                resource: entry.resource.resource,
                id: entry.resource.id,
                resource_type: entry.resource.resource_type,
                version_id: entry.resource.version_id,
                last_modified: entry
                    .resource
                    .last_updated
                    .format(&Rfc3339)
                    .unwrap_or_default(),
                method,
            }
        })
        .collect();

    let bundle = bundle_from_history(
        entries,
        &state.base_url,
        &resource_type,
        Some(&id),
        offset as usize,
        count as usize,
        result.total,
    );

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
    );

    Ok((StatusCode::OK, response_headers, Json(bundle)))
}

/// Type history: GET /{type}/_history
pub async fn type_history(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    Query(params): Query<HistoryQueryParams>,
) -> Result<impl IntoResponse, ApiError> {
    use octofhir_api::{HistoryBundleEntry, HistoryBundleMethod, bundle_from_history};
    use octofhir_storage::HistoryParams;
    use time::format_description::well_known::Rfc3339;

    let span = tracing::info_span!("fhir.history.type", resource_type = %resource_type);
    let _g = span.enter();

    // Validate resource type
    let _rt: ResourceType = match resource_type.parse() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };

    // Build history params
    let mut history_params = HistoryParams::new();
    if let Some(ref since) = params.since {
        history_params.since = Some(parse_fhir_instant(since)?);
    }
    if let Some(ref at) = params.at {
        history_params.at = Some(parse_fhir_instant(at)?);
    }
    let count = params.count.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);
    history_params.count = Some(count);
    history_params.offset = Some(offset);

    // Get history from storage (None for id = type-level history)
    let result = state
        .storage
        .history(&resource_type, None, &history_params)
        .await
        .map_err(map_core_error)?;

    // Convert to bundle entries
    let entries: Vec<HistoryBundleEntry> = result
        .entries
        .into_iter()
        .map(|entry| {
            let method = match entry.method {
                octofhir_storage::HistoryMethod::Create => HistoryBundleMethod::Create,
                octofhir_storage::HistoryMethod::Update => HistoryBundleMethod::Update,
                octofhir_storage::HistoryMethod::Delete => HistoryBundleMethod::Delete,
            };
            HistoryBundleEntry {
                resource: entry.resource.resource,
                id: entry.resource.id,
                resource_type: entry.resource.resource_type,
                version_id: entry.resource.version_id,
                last_modified: entry
                    .resource
                    .last_updated
                    .format(&Rfc3339)
                    .unwrap_or_default(),
                method,
            }
        })
        .collect();

    let bundle = bundle_from_history(
        entries,
        &state.base_url,
        &resource_type,
        None,
        offset as usize,
        count as usize,
        result.total,
    );

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
    );

    Ok((StatusCode::OK, response_headers, Json(bundle)))
}

/// System history: GET /_history
pub async fn system_history(
    State(_state): State<crate::server::AppState>,
    Query(_params): Query<HistoryQueryParams>,
) -> Result<Json<octofhir_api::Bundle>, ApiError> {
    // System-level history is not yet implemented
    // This would require iterating over all resource types
    Err(ApiError::bad_request(
        "System-level history is not yet implemented. Use type-level or instance-level history."
            .to_string(),
    ))
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

/// DELETE /[type]?[search params] - Conditional delete based on search criteria
pub async fn conditional_delete_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.conditional_delete", resource_type = %resource_type);
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

    // Conditional delete requires search parameters
    let raw_q = raw.unwrap_or_default();
    if raw_q.is_empty() {
        return Err(ApiError::bad_request(
            "Conditional delete requires search parameters",
        ));
    }

    // Search for matching resources
    let cfg = &state.search_cfg;
    let search_result =
        octofhir_search::SearchEngine::execute(&state.storage, rt.clone(), &raw_q, cfg).await;

    match search_result {
        Ok(result) => match result.resources.len() {
            0 => {
                // No match - return 204 No Content (idempotent)
                Ok(StatusCode::NO_CONTENT)
            }
            1 => {
                // One match - delete that resource
                let resource_to_delete = &result.resources[0];
                match state.storage.delete(&rt, &resource_to_delete.id).await {
                    Ok(_) => Ok(StatusCode::NO_CONTENT),
                    Err(e) => Err(map_core_error(e)),
                }
            }
            _ => {
                // Multiple matches - return 412 Precondition Failed
                // Per FHIR spec, conditional delete with multiple matches should fail
                Err(ApiError::precondition_failed(
                    "Multiple resources match the search criteria for conditional delete",
                ))
            }
        },
        Err(e) => Err(ApiError::bad_request(format!("Search failed: {}", e))),
    }
}

/// PATCH /[type]/[id] - Patch a resource using JSON Patch (RFC 6902)
pub async fn patch_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.patch", resource_type = %resource_type, id = %id);
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

    // Check Content-Type header to determine patch format
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let is_json_patch = content_type.contains("application/json-patch+json");
    let is_fhirpath_patch = content_type.contains("application/fhir+json");

    if !is_json_patch && !is_fhirpath_patch {
        return Err(ApiError::unsupported_media_type(format!(
            "PATCH requires Content-Type: application/json-patch+json or application/fhir+json, got: {}",
            content_type
        )));
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

    // Read current resource
    let existing = state
        .storage
        .get(&rt, &id)
        .await
        .map_err(map_core_error)?
        .ok_or_else(|| ApiError::not_found(format!("{resource_type} with id '{id}' not found")))?;

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

    // Convert envelope to JSON for patching
    let current_json = json_from_envelope(&existing);

    // Apply patch based on content type
    let patched_json = if is_json_patch {
        apply_json_patch(&current_json, &body)?
    } else {
        // FHIRPath Patch
        apply_fhirpath_patch(
            &state.fhirpath_engine,
            &state.model_provider,
            &current_json,
            &body,
        )
        .await?
    };

    // Validate patched resource
    if let Err(e) = crate::validation::validate_resource(&resource_type, &patched_json) {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    // Verify resourceType hasn't changed (extra safety check)
    if patched_json["resourceType"].as_str() != Some(&resource_type) {
        return Err(ApiError::bad_request(
            "Patch resulted in changed resourceType".to_string(),
        ));
    }

    // Verify id hasn't changed (extra safety check)
    if patched_json["id"].as_str() != Some(&id) {
        return Err(ApiError::bad_request(
            "Patch resulted in changed id".to_string(),
        ));
    }

    // Convert patched JSON back to envelope and update
    let patched_env = envelope_from_json(
        &resource_type,
        &patched_json,
        IdPolicy::Update {
            path_id: id.clone(),
        },
    )
    .map_err(ApiError::bad_request)?;

    match state.storage.update(&rt, &id, patched_env.clone()).await {
        Ok(_) => {
            let version_id = patched_env
                .meta
                .version_id
                .clone()
                .unwrap_or_else(|| "1".to_string());
            let last_updated = patched_env.meta.last_updated.clone();
            let response_body = json_from_envelope(&patched_env);

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
                header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
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
                            "diagnostics": format!("Resource patched: {}/{}", resource_type, id)
                        }]
                    });
                    Ok((StatusCode::OK, response_headers, Json(outcome)))
                }
                _ => Ok((StatusCode::OK, response_headers, Json(response_body))),
            }
        }
        Err(e) => Err(map_core_error(e)),
    }
}

/// PATCH /[type]?[search params] - Conditional patch based on search criteria
pub async fn conditional_patch_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.conditional_patch", resource_type = %resource_type);
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

    // Conditional patch requires search parameters
    let raw_q = raw.unwrap_or_default();
    if raw_q.is_empty() {
        return Err(ApiError::bad_request(
            "Conditional patch requires search parameters",
        ));
    }

    // Check Content-Type header to determine patch format
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let is_json_patch = content_type.contains("application/json-patch+json");
    let is_fhirpath_patch = content_type.contains("application/fhir+json");

    if !is_json_patch && !is_fhirpath_patch {
        return Err(ApiError::unsupported_media_type(format!(
            "PATCH requires Content-Type: application/json-patch+json or application/fhir+json, got: {}",
            content_type
        )));
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
                // No match - return 404
                Err(ApiError::not_found(
                    "No resources match the search criteria for conditional patch",
                ))
            }
            1 => {
                // One match - patch that resource
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

                // Convert envelope to JSON for patching
                let current_json = json_from_envelope(existing);

                // Apply patch based on content type
                let patched_json = if is_json_patch {
                    apply_json_patch(&current_json, &body)?
                } else {
                    // FHIRPath Patch
                    apply_fhirpath_patch(
                        &state.fhirpath_engine,
                        &state.model_provider,
                        &current_json,
                        &body,
                    )
                    .await?
                };

                // Validate patched resource
                if let Err(e) = crate::validation::validate_resource(&resource_type, &patched_json)
                {
                    return Err(ApiError::bad_request(format!("Validation failed: {e}")));
                }

                // Verify resourceType hasn't changed
                if patched_json["resourceType"].as_str() != Some(&resource_type) {
                    return Err(ApiError::bad_request(
                        "Patch resulted in changed resourceType".to_string(),
                    ));
                }

                // Verify id hasn't changed
                if patched_json["id"].as_str() != Some(&id) {
                    return Err(ApiError::bad_request(
                        "Patch resulted in changed id".to_string(),
                    ));
                }

                // Convert patched JSON back to envelope and update
                let patched_env = envelope_from_json(
                    &resource_type,
                    &patched_json,
                    IdPolicy::Update {
                        path_id: id.clone(),
                    },
                )
                .map_err(ApiError::bad_request)?;

                match state.storage.update(&rt, &id, patched_env.clone()).await {
                    Ok(_) => {
                        let version_id = patched_env
                            .meta
                            .version_id
                            .clone()
                            .unwrap_or_else(|| "1".to_string());
                        let last_updated = patched_env.meta.last_updated.clone();
                        let response_body = json_from_envelope(&patched_env);

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
                                        "diagnostics": format!("Resource patched: {}/{}", resource_type, id)
                                    }]
                                });
                                Ok((StatusCode::OK, response_headers, Json(outcome)))
                            }
                            _ => Ok((StatusCode::OK, response_headers, Json(response_body))),
                        }
                    }
                    Err(e) => Err(map_core_error(e)),
                }
            }
            _ => {
                // Multiple matches - return 412 Precondition Failed
                Err(ApiError::precondition_failed(
                    "Multiple resources match the search criteria for conditional patch",
                ))
            }
        },
        Err(e) => Err(ApiError::bad_request(format!("Search failed: {}", e))),
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
                &state.base_url,
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
        CoreError::ResourceDeleted { resource_type, id } => {
            ApiError::gone(format!("{resource_type} with id '{id}' has been deleted"))
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
