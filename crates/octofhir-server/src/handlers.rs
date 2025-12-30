use crate::bootstrap::ADMIN_ACCESS_POLICY_ID;
use crate::mapping::{IdPolicy, envelope_from_json, json_from_envelope};
use crate::operation_registry::{OperationStorage, PostgresOperationStorage};
use crate::patch::{apply_fhirpath_patch, apply_json_patch};
use crate::server::SharedModelProvider;
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
use octofhir_core::fhir_reference::parse_reference_simple;
use octofhir_core::ResourceType;
use octofhir_fhir_model::ModelProvider;
use octofhir_storage::{FhirStorage, StorageError}; // For begin_transaction method and error mapping
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Check if validation should be skipped based on X-Skip-Validation header and config
fn should_skip_validation(headers: &HeaderMap, config: &crate::config::ValidationSettings) -> bool {
    // Feature must be enabled in config
    if !config.allow_skip_validation {
        return false;
    }

    // Check if X-Skip-Validation header is present and set to "true"
    headers
        .get("X-Skip-Validation")
        .and_then(|h| h.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

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

/// Prometheus metrics endpoint.
///
/// Returns metrics in Prometheus text format for scraping.
pub async fn metrics(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    // Update database pool metrics before rendering
    let pool_options = state.db_pool.options();
    let pool_size = state.db_pool.size();
    let pool_idle = state.db_pool.num_idle();

    crate::metrics::record_db_pool_stats(
        pool_options.get_max_connections(),
        pool_idle as u32,
        pool_size - pool_idle as u32,
    );

    // Render Prometheus metrics
    match crate::metrics::render_metrics() {
        Some(output) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            output,
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Metrics not initialized",
        )
            .into_response(),
    }
}

/// Query parameters for capabilities endpoint
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CapabilitiesParams {
    /// Summary mode: true, text, data, count, false
    #[serde(rename = "_summary")]
    pub summary: Option<String>,
}

/// Return cached CapabilityStatement with optional summary transformations.
///
/// The CapabilityStatement is built once at server startup and cached in AppState.
/// This handler just returns the cached value with optional summary transformations.
pub async fn metadata(
    State(state): State<crate::server::AppState>,
    Query(params): Query<CapabilitiesParams>,
) -> impl IntoResponse {
    // Use cached CapabilityStatement (built at startup)
    let body = (*state.capability_statement).clone();

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

/// Build the CapabilityStatement at server startup.
///
/// This function is called once during initialization and the result is cached in AppState.
/// This avoids building the CapabilityStatement on every /metadata request.
pub async fn build_capability_statement(
    fhir_version: &str,
    base_url: &str,
    db_pool: &sqlx_postgres::PgPool,
    resource_types: &[String],
) -> Value {
    use octofhir_api::{CapabilityStatementBuilder, SearchParam};

    // Build base CapabilityStatement per spec
    let mut builder = CapabilityStatementBuilder::new_json_r4b()
        .status("active")
        .kind("instance")
        .add_format("application/fhir+json")
        .add_format("application/json");

    // Apply FHIR version field
    builder = builder.fhir_version(match fhir_version {
        "R4" | "4.0.1" => "4.0.1",
        "R5" | "5.0.0" => "5.0.0",
        "R6" | "6.0.0" => "6.0.0",
        _ => "4.3.0",
    });

    // Fetch all search parameters and StructureDefinitions once (bulk queries)
    let manager = crate::canonical::get_manager();

    // Build maps for search params and profiles grouped by resource type
    let mut search_params_by_type: std::collections::HashMap<String, Vec<SearchParam>> =
        std::collections::HashMap::new();
    let mut base_profiles: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut supported_profiles: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    if let Some(mgr) = &manager {
        // Fetch ALL SearchParameter resources at once (paginated)
        const PAGE_SIZE: usize = 1000;
        let mut offset = 0;
        loop {
            if let Ok(results) = mgr
                .search()
                .await
                .resource_type("SearchParameter")
                .limit(PAGE_SIZE)
                .offset(offset)
                .execute()
                .await
            {
                let page_count = results.resources.len();
                for rm in &results.resources {
                    let content = &rm.resource.content;
                    // Get the base resource types this search param applies to
                    if let Some(base) = content.get("base").and_then(|v| v.as_array()) {
                        let code = content.get("code").and_then(|v| v.as_str()).unwrap_or("");
                        let type_ = content.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        let description = content
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        for base_type in base {
                            if let Some(rt) = base_type.as_str() {
                                search_params_by_type
                                    .entry(rt.to_string())
                                    .or_default()
                                    .push(SearchParam {
                                        name: code.to_string(),
                                        type_: type_.to_string(),
                                        documentation: description.clone(),
                                    });
                            }
                        }
                    }
                }
                if page_count < PAGE_SIZE {
                    break;
                }
                offset += PAGE_SIZE;
            } else {
                break;
            }
        }

        // Fetch ALL StructureDefinition resources at once (paginated)
        let mut offset = 0;
        loop {
            if let Ok(results) = mgr
                .search()
                .await
                .resource_type("StructureDefinition")
                .limit(PAGE_SIZE)
                .offset(offset)
                .execute()
                .await
            {
                let page_count = results.resources.len();
                for rm in &results.resources {
                    let content = &rm.resource.content;
                    if let Some(rt) = content.get("type").and_then(|v| v.as_str())
                        && let Some(url) = content.get("url").and_then(|v| v.as_str())
                    {
                        match content.get("derivation").and_then(|v| v.as_str()) {
                            Some("specialization") => {
                                base_profiles.insert(rt.to_string(), url.to_string());
                            }
                            Some("constraint") => {
                                supported_profiles
                                    .entry(rt.to_string())
                                    .or_default()
                                    .push(url.to_string());
                            }
                            _ => {}
                        }
                    }
                }
                if page_count < PAGE_SIZE {
                    break;
                }
                offset += PAGE_SIZE;
            } else {
                break;
            }
        }
    }

    // Build resources using pre-fetched data
    for rt in resource_types.iter() {
        // Get search params for this resource type, add common params
        let mut mapped: Vec<SearchParam> = search_params_by_type
            .get(rt)
            .cloned()
            .unwrap_or_default();
        mapped.extend(octofhir_api::common_search_params());

        let resource = octofhir_api::CapabilityStatementRestResource::new(rt)
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
            .with_profile(base_profiles.get(rt).cloned())
            .with_supported_profiles(supported_profiles.get(rt).cloned().unwrap_or_default());
        builder = builder.add_resource_struct(resource);
    }

    // Add extended operations from the operation registry to CapabilityStatement
    let op_storage = PostgresOperationStorage::new(db_pool.clone());
    if let Ok(operations) = op_storage.list_all().await {
        // Extended operations to advertise (not basic CRUD)
        let extended_ops = [
            "graphql.query",
            "graphql.instance",
            "system.validate",
            "system.everything",
        ];

        for op in operations {
            if extended_ops.contains(&op.id.as_str()) {
                // Format operation name with $ prefix
                let op_name = op
                    .id
                    .split('.')
                    .last()
                    .map(|s| format!("${}", s))
                    .unwrap_or_else(|| format!("${}", op.id));

                // Use a URN for the definition since these are server-specific operations
                let definition = format!("urn:abyxon:operation:{}", op.id);

                builder = builder.add_operation(op_name, definition);
            }
        }
    }

    let cs = builder.build();

    // Add software section consistent with spec
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
    body["software"] = json!({ "name": "Abyxon", "version": env!("CARGO_PKG_VERSION") });

    // Add implementation section
    body["implementation"] = json!({
        "description": "Abyxon FHIR Server",
        "url": base_url
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

    // Reflect loaded canonical packages via an extension
    if let Some(pkgs) = crate::canonical::with_registry(|r| {
        r.list()
            .iter()
            .map(|p| {
                json!({
                    "url": "urn:abyxon:loaded-package",
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

    body
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

/// Preprocess resource payload before storage (e.g. hashing passwords).
async fn preprocess_payload(resource_type: &str, payload: &mut Value) -> Result<(), ApiError> {
    if resource_type == "User" {
        if let Some(obj) = payload.as_object_mut() {
            if let Some(password) = obj.get("password").and_then(|v| v.as_str()) {
                let hashed = crate::bootstrap::hash_password(password)
                    .map_err(|e| ApiError::internal(format!("Failed to hash password: {}", e)))?;
                obj.insert("passwordHash".to_string(), Value::String(hashed));
                obj.remove("password");
            }
        }
    } else if resource_type == "Client" {
        if let Some(obj) = payload.as_object_mut() {
            if let Some(secret) = obj.get("clientSecret").and_then(|v| v.as_str()) {
                // Only hash if it doesn't look like a bcrypt hash already
                if !secret.starts_with("$2b$") {
                    let hashed = crate::bootstrap::hash_password(secret).map_err(|e| {
                        ApiError::internal(format!("Failed to hash client secret: {}", e))
                    })?;
                    obj.insert("clientSecret".to_string(), Value::String(hashed));
                }
            }
        }
    }
    Ok(())
}

// ---- CRUD & Search placeholders ----

#[tracing::instrument(name = "fhir.create", skip_all, fields(resource_type = %resource_type))]
pub async fn create_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    // Basic structural validation (resourceType match)
    let resource_types = state.resource_type_set.load();
    if let Err(e) =
        crate::validation::validate_resource(&resource_type, &payload, resource_types.as_ref())
    {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    // Full schema + FHIRPath constraint validation using ValidationService
    let skip_validation = should_skip_validation(&headers, &state.config.validation);

    if !skip_validation {
        let validation_outcome = state.validation_service.validate(&payload).await;
        if !validation_outcome.valid {
            return Err(ApiError::UnprocessableEntity {
                message: "Resource validation failed".to_string(),
                operation_outcome: Some(validation_outcome.to_operation_outcome()),
            });
        }
    } else {
        tracing::warn!(
            resource_type = %resource_type,
            operation = "create",
            "Validation skipped via X-Skip-Validation header"
        );
    }

    let mut payload = payload;
    preprocess_payload(&resource_type, &mut payload).await?;

    // Handle conditional create (If-None-Exist)
    if let Some(condition) = headers.get("If-None-Exist")
        && let Ok(condition_str) = condition.to_str()
    {
        // Execute search with If-None-Exist criteria using FhirStorage
        let search_params = octofhir_search::parse_query_string(condition_str, 10, 100);
        match state.storage.search(&resource_type, &search_params).await {
            Ok(result) => match result.entries.len() {
                0 => {
                    // No match - proceed with create
                }
                1 => {
                    // One match - return existing resource with 200
                    let existing = &result.entries[0];
                    let version_id = &existing.version_id;
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
                                existing.last_updated.unix_timestamp() as u64,
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

                    return Ok((
                        StatusCode::OK,
                        response_headers,
                        Json(existing.resource.clone()),
                    ));
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

    // Validate the payload structure using envelope_from_json (for validation only)
    if let Err(err) = envelope_from_json(&resource_type, &payload, IdPolicy::Create) {
        tracing::warn!(error.kind = "invalid-payload", message = %err);
        return Err(ApiError::bad_request(err));
    }

    // Create resource using FhirStorage
    match state.storage.create(&payload).await {
        Ok(stored) => {
            let id = stored.id.clone();
            let version_id = stored.version_id.clone();
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
                    + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64),
            );
            if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                response_headers.insert(header::LAST_MODIFIED, val);
            }

            // Content-Type
            response_headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
            );

            // X-Validation-Skipped header if validation was skipped
            if skip_validation {
                response_headers.insert(
                    "X-Validation-Skipped",
                    header::HeaderValue::from_static("true"),
                );
            }

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
                    Ok((StatusCode::CREATED, response_headers, Json(stored.resource)))
                }
            }
        }
        Err(e) => {
            tracing::error!(error.kind = "create-failed", message = %e);
            Err(map_storage_error(e))
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

#[tracing::instrument(name = "fhir.read", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn read_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    match state.storage.read(&resource_type, &id).await {
        Ok(Some(stored)) => {
            // Get version_id for ETag
            let version_id = &stored.version_id;

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
                    + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64);
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
                    + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64),
            );
            if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                response_headers.insert(header::LAST_MODIFIED, val);
            }

            // Content-Type
            response_headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
            );

            Ok((StatusCode::OK, response_headers, Json(stored.resource)))
        }
        Ok(None) => Err(ApiError::not_found(format!(
            "{resource_type} with id '{id}' not found"
        ))),
        Err(e) => Err(map_storage_error(e)),
    }
}

/// GET /[type]/[id]/_history/[vid] - Read a specific version of a resource
#[tracing::instrument(name = "fhir.vread", skip_all, fields(resource_type = %resource_type, id = %id, version_id = %version_id))]
pub async fn vread_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id, version_id)): Path<(String, String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    // Use vread to get the specific version from storage (including history)
    match state.storage.vread(&resource_type, &id, &version_id).await {
        Ok(Some(stored)) => {
            // Get version_id for ETag
            let version = &stored.version_id;

            // Build response headers
            let mut response_headers = HeaderMap::new();

            // ETag: W/"version_id"
            let etag = format!("W/\"{}\"", version);
            if let Ok(val) = header::HeaderValue::from_str(&etag) {
                response_headers.insert(header::ETAG, val);
            }

            // Last-Modified: HTTP date format
            let last_modified = httpdate::fmt_http_date(
                std::time::UNIX_EPOCH
                    + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64),
            );
            if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                response_headers.insert(header::LAST_MODIFIED, val);
            }

            // Content-Type
            response_headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
            );

            Ok((StatusCode::OK, response_headers, Json(stored.resource)))
        }
        Ok(None) => Err(ApiError::not_found(format!(
            "{resource_type}/{id}/_history/{version_id} not found"
        ))),
        Err(e) => Err(map_storage_error(e)),
    }
}

// ---- History Handlers ----

/// Instance history: GET /{type}/{id}/_history
#[tracing::instrument(name = "fhir.history.instance", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn instance_history(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    Query(params): Query<HistoryQueryParams>,
) -> Result<impl IntoResponse, ApiError> {
    use octofhir_api::{HistoryBundleEntry, HistoryBundleMethod, bundle_from_history};
    use octofhir_storage::HistoryParams;
    use time::format_description::well_known::Rfc3339;

    // Parse resource type
    let _rt: ResourceType = match resource_type.parse() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };

    // Check if resource exists (either current or was deleted)
    // For history, we allow viewing history of deleted resources
    match state.storage.read(&resource_type, &id).await {
        Ok(None) => {
            // Resource never existed
            return Err(ApiError::not_found(format!(
                "{resource_type}/{id} not found"
            )));
        }
        Err(StorageError::Deleted { .. }) => {
            // Resource was deleted, we can still show history
        }
        Ok(Some(_)) => {
            // Resource exists, proceed to get history
        }
        Err(e) => {
            // Other error - but continue anyway to try history
            tracing::debug!("Read check failed: {}, proceeding with history", e);
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
        .map_err(map_storage_error)?;

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
                resource: octofhir_api::RawJson::from(entry.resource.resource),
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
#[tracing::instrument(name = "fhir.history.type", skip_all, fields(resource_type = %resource_type))]
pub async fn type_history(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    Query(params): Query<HistoryQueryParams>,
) -> Result<impl IntoResponse, ApiError> {
    use octofhir_api::{HistoryBundleEntry, HistoryBundleMethod, bundle_from_history};
    use octofhir_storage::HistoryParams;
    use time::format_description::well_known::Rfc3339;

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
        .map_err(map_storage_error)?;

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
                resource: octofhir_api::RawJson::from(entry.resource.resource),
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
#[tracing::instrument(name = "fhir.history.system", skip_all)]
pub async fn system_history(
    State(state): State<crate::server::AppState>,
    Query(params): Query<HistoryQueryParams>,
) -> Result<impl IntoResponse, ApiError> {
    use octofhir_api::{HistoryBundleEntry, HistoryBundleMethod, bundle_from_system_history};
    use octofhir_storage::HistoryParams;
    use time::format_description::well_known::Rfc3339;

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

    // Get system-level history from storage
    let result = state
        .storage
        .system_history(&history_params)
        .await
        .map_err(map_storage_error)?;

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
                resource: octofhir_api::RawJson::from(entry.resource.resource),
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

    let bundle = bundle_from_system_history(
        entries,
        &state.base_url,
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

#[tracing::instrument(name = "fhir.update", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn update_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    // Basic structural validation (resourceType match)
    let resource_types = state.resource_type_set.load();
    if let Err(e) =
        crate::validation::validate_resource(&resource_type, &payload, resource_types.as_ref())
    {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    // Full schema + FHIRPath constraint validation using ValidationService
    let skip_validation = should_skip_validation(&headers, &state.config.validation);

    if !skip_validation {
        let validation_outcome = state.validation_service.validate(&payload).await;
        if !validation_outcome.valid {
            return Err(ApiError::UnprocessableEntity {
                message: "Resource validation failed".to_string(),
                operation_outcome: Some(validation_outcome.to_operation_outcome()),
            });
        }
    } else {
        tracing::warn!(
            resource_type = %resource_type,
            id = %id,
            operation = "update",
            "Validation skipped via X-Skip-Validation header"
        );
    }

    let _rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => {
            return Err(ApiError::bad_request(format!(
                "Unknown resourceType '{resource_type}'"
            )));
        }
    };

    // Protect the default admin access policy from modification
    if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
        return Err(ApiError::forbidden(
            "The default admin access policy cannot be modified",
        ));
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

    let mut payload = payload;
    preprocess_payload(&resource_type, &mut payload).await?;

    // Validate payload structure
    if let Err(err) = envelope_from_json(
        &resource_type,
        &payload,
        IdPolicy::Update {
            path_id: id.clone(),
        },
    ) {
        return Err(ApiError::bad_request(err));
    }

    // Inject id from URL path into payload (storage layer requires id in JSON)
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("id".to_string(), Value::String(id.clone()));
    }

    // Check if resource exists
    let existing = state.storage.read(&resource_type, &id).await;

    match existing {
        Ok(Some(existing_stored)) => {
            // Resource exists - check If-Match if provided
            if let Some(ref expected_version) = if_match {
                if expected_version != &existing_stored.version_id {
                    return Err(ApiError::conflict(format!(
                        "Version conflict: expected {}, but current is {}",
                        expected_version, existing_stored.version_id
                    )));
                }
            }

            // Update existing resource using FhirStorage
            match state.storage.update(&payload, if_match.as_deref()).await {
                Ok(stored) => {
                    let mut response_headers = HeaderMap::new();

                    // ETag
                    let etag = format!("W/\"{}\"", stored.version_id);
                    if let Ok(val) = header::HeaderValue::from_str(&etag) {
                        response_headers.insert(header::ETAG, val);
                    }

                    // Last-Modified
                    let last_modified = httpdate::fmt_http_date(
                        std::time::UNIX_EPOCH
                            + std::time::Duration::from_secs(
                                stored.last_updated.unix_timestamp() as u64
                            ),
                    );
                    if let Ok(val) = header::HeaderValue::from_str(&last_modified) {
                        response_headers.insert(header::LAST_MODIFIED, val);
                    }

                    // Content-Type
                    response_headers.insert(
                        header::CONTENT_TYPE,
                        header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
                    );

                    // X-Validation-Skipped header if validation was skipped
                    if skip_validation {
                        response_headers.insert(
                            "X-Validation-Skipped",
                            header::HeaderValue::from_static("true"),
                        );
                    }

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
                        _ => Ok((StatusCode::OK, response_headers, Json(stored.resource))),
                    }
                }
                Err(e) => Err(map_storage_error(e)),
            }
        }
        Ok(None) => {
            // Resource doesn't exist - create-on-update
            if if_match.is_some() {
                return Err(ApiError::precondition_failed(
                    "Resource does not exist but If-Match was provided",
                ));
            }

            // Create new resource with provided ID using FhirStorage
            match state.storage.create(&payload).await {
                Ok(stored) => {
                    let mut response_headers = HeaderMap::new();

                    // Location header (for create)
                    let loc = format!("/{resource_type}/{id}");
                    if let Ok(val) = header::HeaderValue::from_str(&loc) {
                        response_headers.insert(header::LOCATION, val);
                    }

                    // ETag
                    let etag = format!("W/\"{}\"", stored.version_id);
                    if let Ok(val) = header::HeaderValue::from_str(&etag) {
                        response_headers.insert(header::ETAG, val);
                    }

                    // Last-Modified
                    let last_modified = httpdate::fmt_http_date(
                        std::time::UNIX_EPOCH
                            + std::time::Duration::from_secs(
                                stored.last_updated.unix_timestamp() as u64
                            ),
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
                        _ => Ok((StatusCode::CREATED, response_headers, Json(stored.resource))),
                    }
                }
                Err(e) => Err(map_storage_error(e)),
            }
        }
        Err(e) => Err(map_storage_error(e)),
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
#[tracing::instrument(name = "fhir.conditional_update", skip_all, fields(resource_type = %resource_type))]
pub async fn conditional_update_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate resource type
    let _rt = match resource_type.parse::<ResourceType>() {
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

    // Basic structural validation (resourceType match)
    let resource_types = state.resource_type_set.load();
    if let Err(e) =
        crate::validation::validate_resource(&resource_type, &payload, resource_types.as_ref())
    {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    // Full schema + FHIRPath constraint validation using ValidationService
    let validation_outcome = state.validation_service.validate(&payload).await;
    if !validation_outcome.valid {
        return Err(ApiError::UnprocessableEntity {
            message: "Resource validation failed".to_string(),
            operation_outcome: Some(validation_outcome.to_operation_outcome()),
        });
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

    // Search for matching resources using FhirStorage
    let search_params = octofhir_search::parse_query_string(&raw_q, 10, 100);
    let search_result = state.storage.search(&resource_type, &search_params).await;

    match search_result {
        Ok(result) => match result.entries.len() {
            0 => {
                // No match - create new resource (201)
                if let Err(err) = envelope_from_json(&resource_type, &payload, IdPolicy::Create) {
                    return Err(ApiError::bad_request(err));
                }

                match state.storage.create(&payload).await {
                    Ok(stored) => {
                        let mut response_headers = HeaderMap::new();

                        // Location header
                        let loc = format!("/{resource_type}/{}", stored.id);
                        if let Ok(val) = header::HeaderValue::from_str(&loc) {
                            response_headers.insert(header::LOCATION, val);
                        }

                        // ETag
                        let etag = format!("W/\"{}\"", stored.version_id);
                        if let Ok(val) = header::HeaderValue::from_str(&etag) {
                            response_headers.insert(header::ETAG, val);
                        }

                        // Last-Modified
                        let last_modified = httpdate::fmt_http_date(
                            std::time::UNIX_EPOCH
                                + std::time::Duration::from_secs(
                                    stored.last_updated.unix_timestamp() as u64,
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
                                        "diagnostics": format!("Resource created: {}/{}", resource_type, stored.id)
                                    }]
                                });
                                Ok((StatusCode::CREATED, response_headers, Json(outcome)))
                            }
                            _ => Ok((StatusCode::CREATED, response_headers, Json(stored.resource))),
                        }
                    }
                    Err(e) => Err(map_storage_error(e)),
                }
            }
            1 => {
                // One match - update that resource (200)
                let existing = &result.entries[0];
                let id = existing.id.clone();

                // Protect the default admin access policy from modification
                if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
                    return Err(ApiError::forbidden(
                        "The default admin access policy cannot be modified",
                    ));
                }

                // Check If-Match if provided
                if let Some(ref expected_version) = if_match {
                    if expected_version != &existing.version_id {
                        return Err(ApiError::conflict(format!(
                            "Version conflict: expected {}, but current is {}",
                            expected_version, existing.version_id
                        )));
                    }
                }

                // Validate and update the matched resource
                if let Err(err) = envelope_from_json(
                    &resource_type,
                    &payload,
                    IdPolicy::Update {
                        path_id: id.clone(),
                    },
                ) {
                    return Err(ApiError::bad_request(err));
                }

                match state.storage.update(&payload, if_match.as_deref()).await {
                    Ok(stored) => {
                        let mut response_headers = HeaderMap::new();

                        // Content-Location header
                        let content_loc = format!("/{resource_type}/{id}");
                        if let Ok(val) = header::HeaderValue::from_str(&content_loc) {
                            response_headers.insert(header::CONTENT_LOCATION, val);
                        }

                        // ETag
                        let etag = format!("W/\"{}\"", stored.version_id);
                        if let Ok(val) = header::HeaderValue::from_str(&etag) {
                            response_headers.insert(header::ETAG, val);
                        }

                        // Last-Modified
                        let last_modified = httpdate::fmt_http_date(
                            std::time::UNIX_EPOCH
                                + std::time::Duration::from_secs(
                                    stored.last_updated.unix_timestamp() as u64,
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
                            _ => Ok((StatusCode::OK, response_headers, Json(stored.resource))),
                        }
                    }
                    Err(e) => Err(map_storage_error(e)),
                }
            }
            _ => {
                // Multiple matches - return 412 Precondition Failed
                Err(ApiError::precondition_failed(
                    "Multiple resources match the search criteria",
                ))
            }
        },
        Err(e) => Err(map_storage_error(e)),
    }
}

#[tracing::instrument(name = "fhir.delete", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn delete_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        )));
    }

    // Protect the default admin access policy from deletion
    if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
        return Err(ApiError::forbidden(
            "The default admin access policy cannot be deleted",
        ));
    }

    match state.storage.delete(&resource_type, &id).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(map_storage_error(e)),
    }
}

/// DELETE /[type]?[search params] - Conditional delete based on search criteria
#[tracing::instrument(name = "fhir.conditional_delete", skip_all, fields(resource_type = %resource_type))]
pub async fn conditional_delete_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        )));
    }

    // Conditional delete requires search parameters
    let raw_q = raw.unwrap_or_default();
    if raw_q.is_empty() {
        return Err(ApiError::bad_request(
            "Conditional delete requires search parameters",
        ));
    }

    // Search for matching resources using modern storage API
    let search_params = octofhir_search::parse_query_string(&raw_q, 10, 100);
    let search_result = state.storage.search(&resource_type, &search_params).await;

    match search_result {
        Ok(result) => match result.entries.len() {
            0 => {
                // No match - return 204 No Content (idempotent)
                Ok(StatusCode::NO_CONTENT)
            }
            1 => {
                // One match - delete that resource
                let resource_to_delete = &result.entries[0];

                // Protect the default admin access policy from deletion
                if resource_type == "AccessPolicy" && resource_to_delete.id == ADMIN_ACCESS_POLICY_ID
                {
                    return Err(ApiError::forbidden(
                        "The default admin access policy cannot be deleted",
                    ));
                }

                match state
                    .storage
                    .delete(&resource_type, &resource_to_delete.id)
                    .await
                {
                    Ok(_) => Ok(StatusCode::NO_CONTENT),
                    Err(e) => Err(map_storage_error(e)),
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
        Err(e) => Err(map_storage_error(e)),
    }
}

/// PATCH /[type]/[id] - Patch a resource using JSON Patch (RFC 6902)
#[tracing::instrument(name = "fhir.patch", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn patch_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        )));
    }

    // Protect the default admin access policy from modification
    if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
        return Err(ApiError::forbidden(
            "The default admin access policy cannot be modified",
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

    // Read current resource
    let existing = state
        .storage
        .read(&resource_type, &id)
        .await
        .map_err(map_storage_error)?
        .ok_or_else(|| ApiError::not_found(format!("{resource_type} with id '{id}' not found")))?;

    // Check If-Match if provided
    if let Some(ref expected_version) = if_match {
        if expected_version != &existing.version_id {
            return Err(ApiError::conflict(format!(
                "Version conflict: expected {}, but current is {}",
                expected_version, existing.version_id
            )));
        }
    }

    // Get current resource JSON for patching
    let current_json = existing.resource.clone();

    // Apply patch based on content type
    let mut patched_json = if is_json_patch {
        apply_json_patch(&current_json, &body)?
    } else {
        // FHIRPath Patch
        let model_provider_trait: SharedModelProvider = state.model_provider.clone();
        apply_fhirpath_patch(
            &state.fhirpath_engine,
            &model_provider_trait,
            &current_json,
            &body,
        )
        .await?
    };

    // Basic structural validation (resourceType match)
    let resource_types = state.resource_type_set.load();
    if let Err(e) = crate::validation::validate_resource(
        &resource_type,
        &patched_json,
        resource_types.as_ref(),
    ) {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    // Full schema + FHIRPath constraint validation using ValidationService
    let validation_outcome = state.validation_service.validate(&patched_json).await;
    if !validation_outcome.valid {
        return Err(ApiError::UnprocessableEntity {
            message: "Patched resource validation failed".to_string(),
            operation_outcome: Some(validation_outcome.to_operation_outcome()),
        });
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

    // Ensure id and resourceType are set in the payload
    patched_json["id"] = json!(id);
    patched_json["resourceType"] = json!(resource_type);

    // Update resource using modern storage API
    match state
        .storage
        .update(&patched_json, if_match.as_deref())
        .await
    {
        Ok(stored) => {
            let version_id = &stored.version_id;
            let last_updated_ts = stored.last_updated.unix_timestamp();

            let mut response_headers = HeaderMap::new();

            // ETag
            let etag = format!("W/\"{}\"", version_id);
            if let Ok(val) = header::HeaderValue::from_str(&etag) {
                response_headers.insert(header::ETAG, val);
            }

            // Last-Modified
            let last_modified = httpdate::fmt_http_date(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(last_updated_ts as u64),
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
                _ => Ok((StatusCode::OK, response_headers, Json(stored.resource))),
            }
        }
        Err(e) => Err(map_storage_error(e)),
    }
}

/// PATCH /[type]?[search params] - Conditional patch based on search criteria
#[tracing::instrument(name = "fhir.conditional_patch", skip_all, fields(resource_type = %resource_type))]
pub async fn conditional_patch_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        )));
    }

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

    // Search for matching resources using modern storage API
    let search_params = octofhir_search::parse_query_string(&raw_q, 10, 100);
    let search_result = state.storage.search(&resource_type, &search_params).await;

    match search_result {
        Ok(result) => match result.entries.len() {
            0 => {
                // No match - return 404
                Err(ApiError::not_found(
                    "No resources match the search criteria for conditional patch",
                ))
            }
            1 => {
                // One match - patch that resource
                let existing = &result.entries[0];
                let id = existing.id.clone();

                // Check If-Match if provided
                if let Some(ref expected_version) = if_match {
                    if expected_version != &existing.version_id {
                        return Err(ApiError::conflict(format!(
                            "Version conflict: expected {}, but current is {}",
                            expected_version, existing.version_id
                        )));
                    }
                }

                // Get current resource JSON for patching
                let current_json = existing.resource.clone();

                // Apply patch based on content type
                let mut patched_json = if is_json_patch {
                    apply_json_patch(&current_json, &body)?
                } else {
                    // FHIRPath Patch
                    let model_provider_trait: SharedModelProvider = state.model_provider.clone();
                    apply_fhirpath_patch(
                        &state.fhirpath_engine,
                        &model_provider_trait,
                        &current_json,
                        &body,
                    )
                    .await?
                };

                // Basic structural validation (resourceType match)
                let resource_types = state.resource_type_set.load();
                if let Err(e) = crate::validation::validate_resource(
                    &resource_type,
                    &patched_json,
                    resource_types.as_ref(),
                ) {
                    return Err(ApiError::bad_request(format!("Validation failed: {e}")));
                }

                // Full schema + FHIRPath constraint validation using ValidationService
                let validation_outcome = state.validation_service.validate(&patched_json).await;
                if !validation_outcome.valid {
                    return Err(ApiError::UnprocessableEntity {
                        message: "Patched resource validation failed".to_string(),
                        operation_outcome: Some(validation_outcome.to_operation_outcome()),
                    });
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

                // Ensure id and resourceType are set in the payload
                patched_json["id"] = json!(id.clone());
                patched_json["resourceType"] = json!(resource_type.clone());

                // Update using modern storage API
                match state
                    .storage
                    .update(&patched_json, if_match.as_deref())
                    .await
                {
                    Ok(stored) => {
                        let version_id = &stored.version_id;
                        let last_updated_ts = stored.last_updated.unix_timestamp();

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
                                + std::time::Duration::from_secs(last_updated_ts as u64),
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
                            _ => Ok((StatusCode::OK, response_headers, Json(stored.resource))),
                        }
                    }
                    Err(e) => Err(map_storage_error(e)),
                }
            }
            _ => {
                // Multiple matches - return 412 Precondition Failed
                Err(ApiError::precondition_failed(
                    "Multiple resources match the search criteria for conditional patch",
                ))
            }
        },
        Err(e) => Err(map_storage_error(e)),
    }
}

/// Query parameters for search result modification
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct SearchResultParams {
    /// _summary parameter: true, text, data, count, false
    #[serde(rename = "_summary")]
    pub summary: Option<String>,
    /// _elements parameter: comma-separated list of elements to include
    #[serde(rename = "_elements")]
    pub elements: Option<String>,
    /// _include parameter(s)
    #[serde(rename = "_include")]
    pub include: Option<String>,
    /// _revinclude parameter(s)
    #[serde(rename = "_revinclude")]
    pub revinclude: Option<String>,
}

#[tracing::instrument(name = "fhir.search", skip_all, fields(resource_type = %resource_type))]
pub async fn search_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        )));
    }

    let raw_q = raw.unwrap_or_default();
    let cfg = &state.search_cfg;

    // Parse query string to SearchParams
    let search_params =
        octofhir_search::parse_query_string(&raw_q, cfg.default_count as u32, cfg.max_count as u32);

    // Strip result params from query suffix for pagination links
    let suffix = build_query_suffix_for_links(&raw_q);
    let offset = search_params.offset.unwrap_or(0) as usize;
    let count = search_params.count.unwrap_or(10) as usize;

    // Execute search with raw JSON optimization
    let result = octofhir_db_postgres::queries::execute_search_raw(
        &state.db_pool,
        &resource_type,
        &search_params,
        Some(&cfg.registry),
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let total = result.total.map(|t| t as usize).unwrap_or(result.entries.len());

    // Convert main entries to RawJson
    let (resources, ids): (Vec<_>, Vec<_>) = result
        .entries
        .into_iter()
        .map(|e| (octofhir_api::RawJson::from_string(e.resource_json), e.id))
        .unzip();

    // Convert included entries
    let included: Vec<octofhir_api::RawIncludedEntry> = result
        .included
        .into_iter()
        .map(|e| octofhir_api::RawIncludedEntry {
            resource: octofhir_api::RawJson::from_string(e.resource_json),
            resource_type: e.resource_type,
            id: e.id,
        })
        .collect();

    let bundle = octofhir_api::bundle_from_search_raw(
        total,
        resources,
        ids,
        included,
        &state.base_url,
        &resource_type,
        offset,
        count,
        suffix.as_deref(),
    );

    // Apply _summary and _elements filters if present
    let has_result_params = params.contains_key("_summary") || params.contains_key("_elements");
    if has_result_params {
        let bundle_value = apply_result_params(bundle, &params)?;
        return Ok((StatusCode::OK, Json(bundle_value)).into_response());
    }

    // Return Bundle directly - RawJson entries serialize efficiently via RawValue
    Ok((StatusCode::OK, Json(bundle)).into_response())
}

/// POST /[type]/_search - Search via POST with form-encoded parameters
#[tracing::instrument(name = "fhir.search.post", skip_all, fields(resource_type = %resource_type))]
pub async fn search_resource_post(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    axum::Form(params): axum::Form<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        )));
    }

    // Convert params to query string format
    let raw_q: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let cfg = &state.search_cfg;

    // Parse query string to SearchParams
    let search_params =
        octofhir_search::parse_query_string(&raw_q, cfg.default_count as u32, cfg.max_count as u32);

    // Strip result params from query suffix for pagination links
    let suffix = build_query_suffix_for_links(&raw_q);
    let offset = search_params.offset.unwrap_or(0) as usize;
    let count = search_params.count.unwrap_or(10) as usize;

    // Execute search with raw JSON optimization
    let result = octofhir_db_postgres::queries::execute_search_raw(
        &state.db_pool,
        &resource_type,
        &search_params,
        Some(&cfg.registry),
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let total = result.total.map(|t| t as usize).unwrap_or(result.entries.len());

    // Convert main entries to RawJson
    let (resources, ids): (Vec<_>, Vec<_>) = result
        .entries
        .into_iter()
        .map(|e| (octofhir_api::RawJson::from_string(e.resource_json), e.id))
        .unzip();

    // Convert included entries
    let included: Vec<octofhir_api::RawIncludedEntry> = result
        .included
        .into_iter()
        .map(|e| octofhir_api::RawIncludedEntry {
            resource: octofhir_api::RawJson::from_string(e.resource_json),
            resource_type: e.resource_type,
            id: e.id,
        })
        .collect();

    let bundle = octofhir_api::bundle_from_search_raw(
        total,
        resources,
        ids,
        included,
        &state.base_url,
        &resource_type,
        offset,
        count,
        suffix.as_deref(),
    );

    // Apply _summary and _elements filters if present
    let has_result_params = params.contains_key("_summary") || params.contains_key("_elements");
    if has_result_params {
        let bundle_value = apply_result_params(bundle, &params)?;
        return Ok((StatusCode::OK, Json(bundle_value)).into_response());
    }

    // Return Bundle directly - RawJson entries serialize efficiently via RawValue
    Ok((StatusCode::OK, Json(bundle)).into_response())
}

/// GET / or GET /?_type=Patient,Observation - System-level search
#[tracing::instrument(name = "fhir.search.system", skip_all)]
pub async fn system_search(
    State(state): State<crate::server::AppState>,
    Query(params): Query<HashMap<String, String>>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    // Get _type parameter - required for system search
    let types_param = params.get("_type").ok_or_else(|| {
        ApiError::bad_request("System search requires _type parameter to specify resource types")
    })?;

    let types: Vec<&str> = types_param.split(',').map(|s| s.trim()).collect();

    if types.is_empty() {
        return Err(ApiError::bad_request(
            "_type parameter must specify at least one resource type",
        ));
    }

    let raw_q = raw.unwrap_or_default();
    let cfg = &state.search_cfg;

    let mut all_resources: Vec<Value> = Vec::new();
    let mut total_count: usize = 0;

    // Parse search params once
    let search_params =
        octofhir_search::parse_query_string(&raw_q, cfg.default_count as u32, cfg.max_count as u32);

    // Search each resource type using modern storage API
    for type_name in &types {
        if type_name.parse::<ResourceType>().is_err() {
            tracing::warn!("Skipping unknown resource type: {}", type_name);
            continue;
        }

        match state.storage.search(type_name, &search_params).await {
            Ok(result) => {
                total_count += result.total.unwrap_or(result.entries.len() as u32) as usize;
                for entry in result.entries {
                    all_resources.push(entry.resource);
                }
            }
            Err(e) => {
                tracing::warn!("Search failed for type {}: {}", type_name, e);
                // Continue with other types
            }
        }
    }

    // Apply _count limit to combined results
    let count = params
        .get("_count")
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(cfg.default_count)
        .min(cfg.max_count);

    let offset = params
        .get("_offset")
        .and_then(|o| o.parse::<usize>().ok())
        .unwrap_or(0);

    // Paginate combined results
    let paginated: Vec<Value> = all_resources.into_iter().skip(offset).take(count).collect();

    // Build system search bundle (use first type for link building)
    let primary_type = types.first().unwrap_or(&"Resource");
    let suffix = build_query_suffix_for_links(&raw_q);

    let bundle = octofhir_api::bundle_from_search(
        total_count,
        paginated,
        &state.base_url,
        primary_type,
        offset,
        count,
        suffix.as_deref(),
    );

    // Apply _summary and _elements filters
    let bundle_value = apply_result_params(bundle, &params)?;

    Ok((StatusCode::OK, Json(bundle_value)))
}

/// Build query suffix for pagination links, stripping result params
fn build_query_suffix_for_links(raw_q: &str) -> Option<String> {
    if raw_q.is_empty() {
        return None;
    }
    let filtered: Vec<_> = raw_q
        .split('&')
        .filter(|kv| {
            !kv.starts_with("_count=")
                && !kv.starts_with("_offset=")
                && !kv.starts_with("_summary=")
                && !kv.starts_with("_elements=")
        })
        .collect();
    let s = filtered.join("&");
    if s.is_empty() { None } else { Some(s) }
}

/// Resolve _include and _revinclude for search results
///
/// Note: This is currently unused as execute_search handles includes internally.
/// Kept for potential future use with custom include logic.
#[allow(dead_code)]
async fn resolve_includes_for_search(
    storage: &octofhir_storage::DynStorage,
    resources: &[octofhir_storage::StoredResource],
    resource_type: &str,
    params: &HashMap<String, String>,
    _search_cfg: &octofhir_search::SearchConfig,
) -> Vec<octofhir_api::IncludedResourceEntry> {
    let mut included = Vec::new();
    let mut included_keys: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    // Process _include parameters
    if let Some(include_values) = params.get("_include") {
        for include_spec in include_values.split(',') {
            let parts: Vec<&str> = include_spec.split(':').collect();
            if parts.len() >= 2 {
                let source_type = parts[0];
                let param_name = parts[1];
                let target_type = parts.get(2).copied();

                // Only process includes for matching source type
                if source_type == resource_type || source_type == "*" {
                    // Extract references from resources and fetch included resources
                    for res in resources {
                        if let Some(refs) =
                            extract_references_from_resource(&res.resource, param_name)
                        {
                            for (ref_type, ref_id) in refs {
                                // Skip if target type filter doesn't match
                                if let Some(tt) = target_type {
                                    if ref_type != tt {
                                        continue;
                                    }
                                }

                                // Skip duplicates
                                let key = (ref_type.clone(), ref_id.clone());
                                if included_keys.contains(&key) {
                                    continue;
                                }

                                // Fetch the referenced resource using modern storage API
                                if ref_type.parse::<ResourceType>().is_ok() {
                                    if let Ok(Some(stored)) = storage.read(&ref_type, &ref_id).await
                                    {
                                        included_keys.insert(key);
                                        included.push(octofhir_api::IncludedResourceEntry {
                                            resource: stored.resource.into(),
                                            resource_type: ref_type,
                                            id: ref_id,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Process _revinclude parameters
    if let Some(revinclude_values) = params.get("_revinclude") {
        for revinclude_spec in revinclude_values.split(',') {
            let parts: Vec<&str> = revinclude_spec.split(':').collect();
            if parts.len() >= 2 {
                let source_type = parts[0];
                let param_name = parts[1];

                // For revinclude, find resources of source_type that reference our results
                if source_type.parse::<ResourceType>().is_ok() {
                    for res in resources {
                        // Build search params to find referencing resources
                        let ref_value = format!("{}/{}", resource_type, res.id);
                        let search_params = octofhir_storage::SearchParams::new()
                            .with_count(100)
                            .with_param(param_name, &ref_value);

                        // Execute search for referencing resources
                        if let Ok(result) = storage.search(source_type, &search_params).await {
                            for entry in result.entries {
                                let key = (entry.resource_type.clone(), entry.id.clone());
                                if !included_keys.contains(&key) {
                                    included_keys.insert(key);
                                    included.push(octofhir_api::IncludedResourceEntry {
                                        resource: entry.resource.into(),
                                        resource_type: entry.resource_type,
                                        id: entry.id,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    included
}

/// Extract reference values from a resource for a given search parameter
fn extract_references_from_resource(
    resource: &Value,
    param_name: &str,
) -> Option<Vec<(String, String)>> {
    let mut refs = Vec::new();

    // Common reference field mappings
    let field_name = match param_name {
        "patient" | "subject" => "subject",
        "encounter" => "encounter",
        "performer" => "performer",
        "author" => "author",
        "organization" => "managingOrganization",
        "practitioner" => "practitioner",
        "location" => "location",
        _ => param_name,
    };

    // Try to get the field
    if let Some(field_value) = resource.get(field_name) {
        extract_refs_from_value(field_value, &mut refs);
    }

    // Also try direct param name
    if let Some(field_value) = resource.get(param_name) {
        extract_refs_from_value(field_value, &mut refs);
    }

    if refs.is_empty() { None } else { Some(refs) }
}

/// Extract references from a JSON value (handles Reference objects and arrays)
fn extract_refs_from_value(value: &Value, refs: &mut Vec<(String, String)>) {
    match value {
        Value::Object(obj) => {
            // Check if this is a Reference
            if let Some(ref_str) = obj.get("reference").and_then(|v| v.as_str())
                && let Some((rtype, rid)) = parse_reference_string(ref_str)
            {
                refs.push((rtype, rid));
            }
        }
        Value::Array(arr) => {
            for item in arr {
                extract_refs_from_value(item, refs);
            }
        }
        _ => {}
    }
}

/// Parse a FHIR reference string into (type, id)
fn parse_reference_string(reference: &str) -> Option<(String, String)> {
    // For backwards compatibility, handle absolute URLs by extracting the last two path segments.
    // This matches the old behavior where all absolute URLs were treated as potentially local.
    if reference.contains("://") {
        let parts: Vec<&str> = reference.rsplitn(3, '/').collect();
        if parts.len() >= 2 {
            let rtype = parts[1];
            let rid = parts[0];
            if !rtype.is_empty() && !rid.is_empty() {
                // Validate resource type starts with uppercase
                if rtype.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
                    return Some((rtype.to_string(), rid.to_string()));
                }
            }
        }
        return None;
    }

    // For relative references, use the shared implementation
    parse_reference_simple(reference, None).ok()
}

/// Apply _summary and _elements result parameters to a bundle
fn apply_result_params(
    bundle: octofhir_api::Bundle,
    params: &HashMap<String, String>,
) -> Result<Value, ApiError> {
    let mut bundle_value =
        serde_json::to_value(bundle).map_err(|e| ApiError::internal(e.to_string()))?;

    let summary = params.get("_summary").map(|s| s.as_str());
    let elements = params.get("_elements");

    // Handle _summary=count - remove entries
    if summary == Some("count") {
        if let Some(obj) = bundle_value.as_object_mut() {
            obj.remove("entry");
        }
        return Ok(bundle_value);
    }

    // Apply summary or elements to each entry's resource
    if let Some(entries) = bundle_value.get_mut("entry").and_then(|e| e.as_array_mut()) {
        for entry in entries {
            if let Some(resource) = entry.get_mut("resource") {
                // Apply _summary
                if let Some(sum) = summary {
                    *resource = apply_summary(resource, sum);
                }

                // Apply _elements
                if let Some(elems) = elements {
                    *resource = apply_elements_filter(resource, elems);
                }
            }
        }
    }

    Ok(bundle_value)
}

/// Apply _summary parameter to a resource
fn apply_summary(resource: &Value, summary: &str) -> Value {
    match summary {
        "true" => {
            // Return only summary elements (id, meta, text, and type-specific summary fields)
            let mut result = json!({
                "resourceType": resource.get("resourceType").cloned().unwrap_or(Value::Null),
                "id": resource.get("id").cloned().unwrap_or(Value::Null),
            });
            if let Some(meta) = resource.get("meta") {
                result["meta"] = meta.clone();
            }
            if let Some(text) = resource.get("text") {
                result["text"] = text.clone();
            }
            result
        }
        "text" => {
            // Return only text narrative
            json!({
                "resourceType": resource.get("resourceType").cloned().unwrap_or(Value::Null),
                "id": resource.get("id").cloned().unwrap_or(Value::Null),
                "text": resource.get("text").cloned().unwrap_or(Value::Null),
            })
        }
        "data" => {
            // Return everything except text
            let mut result = resource.clone();
            if let Some(obj) = result.as_object_mut() {
                obj.remove("text");
            }
            result
        }
        _ => resource.clone(),
    }
}

/// Apply _elements filter to a resource
fn apply_elements_filter(resource: &Value, elements: &str) -> Value {
    let element_list: Vec<&str> = elements.split(',').map(|s| s.trim()).collect();

    let mut result = json!({
        "resourceType": resource.get("resourceType").cloned().unwrap_or(Value::Null),
        "id": resource.get("id").cloned().unwrap_or(Value::Null),
    });

    // Always include meta
    if let Some(meta) = resource.get("meta") {
        result["meta"] = meta.clone();
    }

    // Include requested elements
    for elem in element_list {
        if let Some(value) = resource.get(elem) {
            result[elem] = value.clone();
        }
    }

    result
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

    // Check storage connectivity by trying a simple search
    let search_params = octofhir_storage::SearchParams::new().with_count(1);
    if let Err(e) = state.storage.search("Patient", &search_params).await {
        tracing::warn!("Storage health check failed: {}", e);
        // Storage might still be initializing or empty, don't fail the health check
    }

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

/// Server settings response for UI feature detection
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettingsResponse {
    /// FHIR version configured
    pub fhir_version: String,
    /// Feature flags
    pub features: ServerFeatures,
}

/// Feature flags for the server
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerFeatures {
    /// SQL on FHIR (ViewDefinition editor, $run operation)
    pub sql_on_fhir: bool,
    /// GraphQL API
    pub graphql: bool,
    /// Bulk Data Export ($export)
    pub bulk_export: bool,
    /// DB Console (SQL execution)
    pub db_console: bool,
    /// Authentication enabled
    pub auth: bool,
}

/// GET /api/settings - Server settings and feature flags for UI
pub async fn api_settings(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    let config = &state.config;

    let response = ServerSettingsResponse {
        fhir_version: state.fhir_version.clone(),
        features: ServerFeatures {
            sql_on_fhir: config.sql_on_fhir.enabled,
            graphql: config.graphql.enabled,
            bulk_export: config.bulk_export.enabled,
            db_console: config.db_console.enabled,
            auth: true,
        },
    };

    (StatusCode::OK, Json(response))
}

/// GET /api/resource-types - List available FHIR resource types
pub async fn api_resource_types(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    match state.model_provider.get_resource_types().await {
        Ok(mut resource_types) => {
            resource_types.sort();
            (StatusCode::OK, Json(resource_types))
        }
        Err(err) => {
            tracing::error!("Failed to load resource types: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(Vec::<String>::new()),
            )
        }
    }
}

/// Categorized resource type for UI grouping
#[derive(Debug, Clone, Serialize)]
pub struct CategorizedResourceType {
    pub name: String,
    pub category: String,
    pub url: Option<String>,
    pub package: String,
}

/// Counts per category for UI segmented control
#[derive(Debug, Clone, Serialize)]
pub struct CategoryCounts {
    pub all: usize,
    pub fhir: usize,
    pub system: usize,
    pub custom: usize,
}

/// Response for categorized resource types
#[derive(Debug, Clone, Serialize)]
pub struct CategorizedResourceTypesResponse {
    pub types: Vec<CategorizedResourceType>,
    pub counts: CategoryCounts,
}

/// GET /api/resource-types-categorized - List resource types with category info
pub async fn api_resource_types_categorized(
    State(state): State<crate::server::AppState>,
) -> impl IntoResponse {
    use octofhir_db_postgres::PostgresPackageStore;

    let store = PostgresPackageStore::new(state.db_pool.as_ref().clone());
    let fhir_version = state.model_provider.fhir_version_str();

    match store
        .list_fhirschema_names_with_package(&["resource", "logical"], fhir_version)
        .await
    {
        Ok(schema_infos) => {
            let mut types: Vec<CategorizedResourceType> = schema_infos
                .into_iter()
                .map(|info| {
                    let category = if info.package_name.starts_with("hl7.fhir.") {
                        "fhir"
                    } else if info.package_name.starts_with("octofhir-")
                        || info.package_name.starts_with("octofhir.")
                    {
                        "system"
                    } else {
                        "custom"
                    };
                    let package = format!("{}@{}", info.package_name, info.package_version);
                    CategorizedResourceType {
                        name: info.name,
                        category: category.to_string(),
                        url: info.url,
                        package,
                    }
                })
                .collect();
            types.sort_by(|a, b| a.name.cmp(&b.name));

            // Calculate counts per category
            let counts = CategoryCounts {
                all: types.len(),
                fhir: types.iter().filter(|t| t.category == "fhir").count(),
                system: types.iter().filter(|t| t.category == "system").count(),
                custom: types.iter().filter(|t| t.category == "custom").count(),
            };

            (
                StatusCode::OK,
                Json(CategorizedResourceTypesResponse { types, counts }),
            )
        }
        Err(err) => {
            tracing::error!("Failed to load categorized resource types: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CategorizedResourceTypesResponse {
                    types: vec![],
                    counts: CategoryCounts {
                        all: 0,
                        fhir: 0,
                        system: 0,
                        custom: 0,
                    },
                }),
            )
        }
    }
}

/// GET /api/json-schema/{resource_type} - Get JSON Schema for a resource type
pub async fn api_json_schema(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Check cache first
    if let Some(cached) = state.json_schema_cache.get(&resource_type) {
        return Ok(Json(cached.clone()));
    }

    // Get FhirSchema from model provider
    let fhir_schema = state
        .model_provider
        .get_schema(&resource_type)
        .await
        .ok_or_else(|| {
            ApiError::NotFound(format!("Schema not found for resource type: {}", resource_type))
        })?;

    // Convert to JSON Schema
    let json_schema = crate::json_schema::convert_fhir_to_json_schema(&fhir_schema);

    // Cache the result
    state
        .json_schema_cache
        .insert(resource_type, json_schema.clone());

    Ok(Json(json_schema))
}

/// Query parameters for operations list
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct OperationsQueryParams {
    /// Filter by category
    pub category: Option<String>,
    /// Filter by module
    pub module: Option<String>,
    /// Filter by public flag
    pub public: Option<bool>,
}

/// GET /api/operations - List all server operations
pub async fn api_operations(
    State(state): State<crate::server::AppState>,
    Query(params): Query<OperationsQueryParams>,
) -> impl IntoResponse {
    use crate::operation_registry::{OperationStorage, PostgresOperationStorage};
    use octofhir_core::OperationDefinition;

    let op_storage = PostgresOperationStorage::new(state.db_pool.as_ref().clone());

    // Get all operations from the operations registry table
    let mut operations: Vec<OperationDefinition> =
        match (&params.category, &params.module, params.public) {
            (Some(category), _, _) => op_storage.list_by_category(category).await,
            (_, Some(module), _) => op_storage.list_by_module(module).await,
            (_, _, Some(true)) => op_storage.list_public().await,
            _ => op_storage.list_all().await,
        }
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to load operations from registry");
            Vec::new()
        });

    // Load Gateway CustomOperations dynamically (to avoid duplication in database)
    let gateway_ops = load_gateway_operations(&state.storage).await;
    operations.extend(gateway_ops);

    // Apply filters to gateway operations
    let operations: Vec<OperationDefinition> = operations
        .into_iter()
        .filter(|op| {
            // Category filter
            if let Some(ref cat) = params.category {
                if &op.category != cat {
                    return false;
                }
            }
            // Module filter
            if let Some(ref module) = params.module {
                if &op.module != module {
                    return false;
                }
            }
            // Public filter
            if let Some(public) = params.public {
                if op.public != public {
                    return false;
                }
            }
            true
        })
        .collect();

    let response = serde_json::json!({
        "operations": operations,
        "total": operations.len()
    });
    (StatusCode::OK, Json(response))
}

/// Load Gateway CustomOperations and convert them to OperationDefinitions.
///
/// This function is public to allow the gateway reload listener to update
/// the public paths cache when CustomOperations change.
pub async fn load_gateway_operations(
    storage: &octofhir_storage::DynStorage,
) -> Vec<octofhir_core::OperationDefinition> {
    use crate::gateway::types::{App, CustomOperation};
    use octofhir_core::{OperationDefinition, categories};
    use std::collections::HashMap;

    // Load all active Apps using modern storage API
    let search_params = octofhir_storage::SearchParams::new().with_count(1000);
    let apps_result = storage.search("App", &search_params).await;

    let apps: Vec<App> = match apps_result {
        Ok(result) => result
            .entries
            .into_iter()
            .filter_map(|stored| serde_json::from_value(stored.resource).ok())
            .filter(|app: &App| app.active)
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load Apps for gateway operations");
            return Vec::new();
        }
    };

    // Build a map of app ID -> App for quick lookup
    let app_map: HashMap<String, App> = apps
        .into_iter()
        .filter_map(|app| app.id.clone().map(|id| (id, app)))
        .collect();

    // Load all active CustomOperations using modern storage API
    let ops_result = storage.search("CustomOperation", &search_params).await;

    let custom_operations: Vec<CustomOperation> = match ops_result {
        Ok(result) => result
            .entries
            .into_iter()
            .filter_map(|stored| serde_json::from_value(stored.resource).ok())
            .filter(|op: &CustomOperation| op.active)
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load CustomOperations");
            return Vec::new();
        }
    };

    // Convert CustomOperations to OperationDefinitions
    let mut operations = Vec::new();

    for custom_op in custom_operations {
        // Extract app reference
        let app_ref = match custom_op.app.reference.as_ref() {
            Some(r) => r,
            None => continue,
        };

        // Extract app ID from reference (e.g., "App/123" -> "123")
        let app_id = match app_ref.split('/').next_back() {
            Some(id) => id,
            None => continue,
        };

        // Find the app
        let app = match app_map.get(app_id) {
            Some(a) => a,
            None => continue,
        };

        // Build full path by combining app base path and operation path
        let full_path = format!("{}{}", app.base_path, custom_op.path);

        // Create operation ID from app name and operation ID
        let operation_id = format!(
            "gateway.{}.{}",
            app.name.to_lowercase().replace(' ', "_"),
            custom_op
                .id
                .as_ref()
                .unwrap_or(&"unknown".to_string())
                .to_lowercase()
        );

        // Build description
        let description = format!(
            "Custom {} operation: {} (Type: {})",
            custom_op.method, full_path, custom_op.operation_type
        );

        // Create OperationDefinition
        let op_def = OperationDefinition::new(
            operation_id,
            format!(
                "{} {}",
                custom_op.method,
                custom_op.id.as_deref().unwrap_or("Unknown")
            ),
            categories::API,
            vec![custom_op.method.clone()],
            full_path,
            app.id.clone().unwrap_or_else(|| "gateway".to_string()),
        )
        .with_description(description)
        .with_public(custom_op.public);

        operations.push(op_def);
    }

    operations
}

/// GET /api/operations/{id} - Get a specific operation by ID
pub async fn api_operation_get(
    State(state): State<crate::server::AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::operation_registry::{OperationStorage, PostgresOperationStorage};

    let storage = PostgresOperationStorage::new(state.db_pool.as_ref().clone());

    match storage.get(&id).await {
        Ok(Some(op)) => (StatusCode::OK, Json(serde_json::to_value(op).unwrap())),
        Ok(None) => {
            let error_response = serde_json::json!({
                "error": format!("Operation '{}' not found", id)
            });
            (StatusCode::NOT_FOUND, Json(error_response))
        }
        Err(e) => {
            let error_response = serde_json::json!({
                "error": format!("Failed to get operation: {}", e)
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response))
        }
    }
}

/// Request body for PATCH /api/operations/{id}
#[derive(Debug, serde::Deserialize)]
pub struct OperationPatchRequest {
    /// Update the public flag (whether operation requires authentication)
    #[serde(default)]
    pub public: Option<bool>,
    /// Update the description
    #[serde(default)]
    pub description: Option<String>,
}

/// PATCH /api/operations/{id} - Update a specific operation
///
/// Allows updating mutable operation properties like `public` flag.
/// This is useful for administrators to make operations public/private
/// without restarting the server.
///
/// OperationRegistryService automatically updates in-memory indexes when public flag changes.
pub async fn api_operation_patch(
    State(state): State<crate::server::AppState>,
    Path(id): Path<String>,
    Json(body): Json<OperationPatchRequest>,
) -> impl IntoResponse {
    use crate::operation_registry::{OperationStorage, OperationUpdate, PostgresOperationStorage};

    // If only updating public flag, use the registry service which also updates in-memory indexes
    if body.description.is_none() && body.public.is_some() {
        match state.operation_registry.set_operation_public(&id, body.public.unwrap()).await {
            Ok(Some(op)) => {
                tracing::info!(operation_id = %id, public = body.public.unwrap(), "Operation public flag updated");
                return (StatusCode::OK, Json(serde_json::to_value(op).unwrap()));
            }
            Ok(None) => {
                let error_response = serde_json::json!({
                    "error": format!("Operation '{}' not found", id)
                });
                return (StatusCode::NOT_FOUND, Json(error_response));
            }
            Err(e) => {
                let error_response = serde_json::json!({
                    "error": format!("Failed to update operation: {}", e)
                });
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response));
            }
        }
    }

    // For other updates (description), use direct storage update
    let storage = PostgresOperationStorage::new(state.db_pool.as_ref().clone());

    let update = OperationUpdate {
        public: body.public,
        description: body.description,
    };

    match storage.update(&id, update).await {
        Ok(Some(op)) => {
            // If public flag was also changed, re-sync registry to update in-memory indexes
            if body.public.is_some() {
                if let Err(e) = state.operation_registry.sync_operations(false).await {
                    tracing::warn!(error = %e, "Failed to sync operation registry after update");
                }
            }
            (StatusCode::OK, Json(serde_json::to_value(op).unwrap()))
        }
        Ok(None) => {
            let error_response = serde_json::json!({
                "error": format!("Operation '{}' not found", id)
            });
            (StatusCode::NOT_FOUND, Json(error_response))
        }
        Err(e) => {
            let error_response = serde_json::json!({
                "error": format!("Failed to update operation: {}", e)
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response))
        }
    }
}

// ---- Error mapping helpers ----
fn map_storage_error(e: StorageError) -> ApiError {
    match e {
        StorageError::NotFound { resource_type, id } => {
            ApiError::not_found(format!("{resource_type} with id '{id}' not found"))
        }
        StorageError::AlreadyExists { resource_type, id } => {
            ApiError::conflict(format!("{resource_type} with id '{id}' already exists"))
        }
        StorageError::Deleted { resource_type, id } => {
            ApiError::gone(format!("{resource_type} with id '{id}' has been deleted"))
        }
        StorageError::VersionConflict { expected, actual } => ApiError::precondition_failed(
            &format!("Version conflict: expected {expected}, got {actual}"),
        ),
        StorageError::InvalidResource { message } => ApiError::bad_request(message),
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

// ============================================================================
// Transaction/Batch Bundle Processing (Task 0020)
// ============================================================================

/// POST / - Process transaction or batch bundle
#[tracing::instrument(name = "fhir.bundle", skip_all)]
pub async fn transaction_handler(
    State(state): State<crate::server::AppState>,
    headers: HeaderMap,
    Json(bundle): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    // Check for Prefer: respond-async header
    let prefer_async = headers
        .get("prefer")
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("respond-async"))
        })
        .unwrap_or(false);

    // Validate bundle structure
    let resource_type = bundle["resourceType"].as_str();
    if resource_type != Some("Bundle") {
        return Err(ApiError::bad_request(
            "Expected Bundle resource at root endpoint",
        ));
    }

    let bundle_type = bundle["type"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing bundle type"))?;

    // If async requested, submit job
    if prefer_async {
        let request = crate::async_jobs::AsyncJobRequest {
            request_type: format!("bundle-{}", bundle_type),
            method: "POST".to_string(),
            url: "/".to_string(),
            body: Some(bundle),
            headers: None,
            client_id: None,
        };

        let job_id = state
            .async_job_manager
            .submit_job(request)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to submit async job: {}", e)))?;

        return Ok(create_async_accepted_response(job_id, &state.base_url));
    }

    // Otherwise, process synchronously
    match bundle_type {
        "transaction" => {
            let (status, json) = process_transaction(&state, &bundle).await?;
            Ok((status, HeaderMap::new(), json))
        }
        "batch" => {
            let (status, json) = process_batch(&state, &bundle).await?;
            Ok((status, HeaderMap::new(), json))
        }
        _ => Err(ApiError::bad_request(format!(
            "Bundle type '{}' not supported at root endpoint. Use 'transaction' or 'batch'.",
            bundle_type
        ))),
    }
}

/// Process a transaction bundle atomically - all entries succeed or all fail
async fn process_transaction(
    state: &crate::server::AppState,
    bundle: &Value,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let entries = bundle["entry"]
        .as_array()
        .ok_or_else(|| ApiError::bad_request("Missing or invalid bundle entries"))?;

    if entries.is_empty() {
        // Empty transaction is valid - return empty response
        let response_bundle = json!({
            "resourceType": "Bundle",
            "type": "transaction-response",
            "entry": []
        });
        return Ok((StatusCode::OK, Json(response_bundle)));
    }

    // Sort entries by HTTP method for proper processing order
    let sorted_entries = sort_transaction_entries(entries);

    // Build reference map for fullUrl -> actual reference resolution
    let mut reference_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // Begin native PostgreSQL database transaction
    tracing::debug!("Beginning native PostgreSQL transaction for Bundle processing");
    // Access the DB pool directly from AppState for native transactions
    let mut tx = octofhir_db_postgres::PostgresStorage::from_pool((*state.db_pool).clone())
        .begin_transaction()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to begin transaction: {}", e)))?;

    let mut response_entries: Vec<Value> = Vec::new();

    // Process each entry within the transaction
    for entry in &sorted_entries {
        let result =
            process_transaction_entry_with_tx(&mut *tx, state, entry, &mut reference_map).await;

        match result {
            Ok(response_entry) => {
                response_entries.push(response_entry);
            }
            Err(e) => {
                // Transaction failed - automatic rollback via Drop impl
                tracing::warn!("Transaction failed, will auto-rollback: {}", e);
                // No need to explicitly rollback - PostgresTransaction Drop handles it
                return Err(e);
            }
        }
    }

    // Commit transaction
    tx.commit()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to commit transaction: {}", e)))?;

    tracing::debug!(
        "Transaction committed successfully with {} entries",
        response_entries.len()
    );

    // Build response bundle
    let response_bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction-response",
        "entry": response_entries
    });

    Ok((StatusCode::OK, Json(response_bundle)))
}

/// Process a single transaction entry using a transaction object.
///
/// This version works with native database transactions for ACID guarantees.
async fn process_transaction_entry_with_tx(
    tx: &mut dyn octofhir_storage::Transaction,
    _state: &crate::server::AppState,
    entry: &Value,
    reference_map: &mut std::collections::HashMap<String, String>,
) -> Result<Value, ApiError> {
    let request = &entry["request"];
    let method = request["method"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.method in bundle entry"))?;
    let url = request["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.url in bundle entry"))?;

    let full_url = entry["fullUrl"].as_str();

    // Resolve references in the resource if present
    let resource = if let Some(res) = entry.get("resource") {
        Some(resolve_bundle_references(res, reference_map)?)
    } else {
        None
    };

    match method.to_uppercase().as_str() {
        "POST" => {
            // Create operation
            let resource =
                resource.ok_or_else(|| ApiError::bad_request("POST entry requires a resource"))?;

            let (resource_type, _condition) = if let Some(idx) = url.find('?') {
                (&url[..idx], Some(&url[idx + 1..]))
            } else {
                (url, None)
            };

            let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
            })?;

            // Create using transaction
            let stored = tx
                .create(&resource)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to create resource: {}", e)))?;

            // Update reference map
            if let Some(fu) = full_url {
                reference_map.insert(fu.to_string(), format!("{}/{}", resource_type, stored.id));
            }

            // Build response - convert StoredResource to ResourceEnvelope
            let envelope: octofhir_core::ResourceEnvelope =
                serde_json::from_value(stored.resource.clone()).map_err(|e| {
                    ApiError::internal(format!("Failed to deserialize resource: {}", e))
                })?;
            let response_json = json_from_envelope(&envelope);
            Ok(build_transaction_response_entry(
                Some(&response_json),
                "201 Created",
                Some(resource_type),
                Some(&stored.id),
                Some(&stored.version_id),
            ))
        }
        "PUT" => {
            // Update operation
            let resource =
                resource.ok_or_else(|| ApiError::bad_request("PUT entry requires a resource"))?;

            // Update using transaction
            let stored = tx
                .update(&resource)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to update resource: {}", e)))?;

            // Convert StoredResource to ResourceEnvelope
            let envelope: octofhir_core::ResourceEnvelope =
                serde_json::from_value(stored.resource.clone()).map_err(|e| {
                    ApiError::internal(format!("Failed to deserialize resource: {}", e))
                })?;
            let response_json = json_from_envelope(&envelope);
            let resource_type = stored.resource_type.clone();
            Ok(build_transaction_response_entry(
                Some(&response_json),
                "200 OK",
                Some(&resource_type),
                Some(&stored.id),
                Some(&stored.version_id),
            ))
        }
        "DELETE" => {
            // Delete operation
            let parts: Vec<&str> = url.split('/').collect();
            if parts.len() != 2 {
                return Err(ApiError::bad_request(format!(
                    "Invalid DELETE url format: {}",
                    url
                )));
            }

            let resource_type = parts[0];
            let id = parts[1];

            let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
            })?;

            // Delete using transaction
            tx.delete(resource_type, id).await.map_err(|e| {
                if e.to_string().contains("not found") || e.to_string().contains("deleted") {
                    ApiError::not_found(format!("{}/{}", resource_type, id))
                } else {
                    ApiError::internal(format!("Failed to delete resource: {}", e))
                }
            })?;

            Ok(build_transaction_response_entry(
                None,
                "204 No Content",
                Some(resource_type),
                Some(id),
                None,
            ))
        }
        "GET" => {
            // Read operation
            let parts: Vec<&str> = url.split('/').collect();
            if parts.len() != 2 {
                return Err(ApiError::bad_request(format!(
                    "Invalid GET url format: {}",
                    url
                )));
            }

            let resource_type = parts[0];
            let id = parts[1];

            let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
            })?;

            // Read using transaction (sees uncommitted changes in same transaction)
            let stored = tx
                .read(resource_type, id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to read resource: {}", e)))?
                .ok_or_else(|| ApiError::not_found(format!("{}/{}", resource_type, id)))?;

            // Convert StoredResource to ResourceEnvelope
            let envelope: octofhir_core::ResourceEnvelope =
                serde_json::from_value(stored.resource.clone()).map_err(|e| {
                    ApiError::internal(format!("Failed to deserialize resource: {}", e))
                })?;
            let response_json = json_from_envelope(&envelope);
            Ok(build_transaction_response_entry(
                Some(&response_json),
                "200 OK",
                Some(resource_type),
                Some(&stored.id),
                Some(&stored.version_id),
            ))
        }
        "PATCH" => {
            // Patch is typically converted to PUT in FHIR
            Err(ApiError::bad_request(
                "PATCH not yet supported in transaction bundles with native transactions",
            ))
        }
        _ => Err(ApiError::bad_request(format!(
            "Unknown HTTP method in bundle entry: {}",
            method
        ))),
    }
}

/// Process a batch bundle non-atomically - each entry is independent
async fn process_batch(
    state: &crate::server::AppState,
    bundle: &Value,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let entries = bundle["entry"]
        .as_array()
        .ok_or_else(|| ApiError::bad_request("Missing or invalid bundle entries"))?;

    if entries.is_empty() {
        let response_bundle = json!({
            "resourceType": "Bundle",
            "type": "batch-response",
            "entry": []
        });
        return Ok((StatusCode::OK, Json(response_bundle)));
    }

    let mut reference_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut response_entries: Vec<Value> = Vec::new();

    // Process each entry independently (no rollback on failure)
    for entry in entries {
        let result = process_transaction_entry(state, entry, &mut reference_map).await;

        match result {
            Ok((response_entry, _)) => {
                response_entries.push(response_entry);
            }
            Err(e) => {
                // For batch, return error response for this entry and continue
                response_entries.push(json!({
                    "response": {
                        "status": format!("{} {}", e.status_code().as_u16(), e.status_code().canonical_reason().unwrap_or("Error")),
                        "outcome": e.to_operation_outcome()
                    }
                }));
            }
        }
    }

    let response_bundle = json!({
        "resourceType": "Bundle",
        "type": "batch-response",
        "entry": response_entries
    });

    Ok((StatusCode::OK, Json(response_bundle)))
}

/// Sort transaction entries by HTTP method order per FHIR spec
/// Order: DELETE, POST, PUT, PATCH, GET, HEAD
fn sort_transaction_entries(entries: &[Value]) -> Vec<&Value> {
    let mut sorted: Vec<_> = entries.iter().collect();

    sorted.sort_by(|a, b| {
        let method_a = a["request"]["method"].as_str().unwrap_or("");
        let method_b = b["request"]["method"].as_str().unwrap_or("");

        let order_a = transaction_method_order(method_a);
        let order_b = transaction_method_order(method_b);

        order_a.cmp(&order_b)
    });

    sorted
}

/// Get processing order for HTTP methods in transactions
fn transaction_method_order(method: &str) -> u8 {
    match method.to_uppercase().as_str() {
        "DELETE" => 0, // Delete first
        "POST" => 1,   // Create second (may be referenced by later entries)
        "PUT" => 2,    // Update third
        "PATCH" => 3,  // Patch fourth
        "GET" => 4,    // Read last
        "HEAD" => 5,   // Head last
        _ => 6,
    }
}

/// Process a single transaction entry
/// Returns (response_entry, Option<(resource_type, id)>) where the tuple indicates a created resource for rollback
async fn process_transaction_entry(
    state: &crate::server::AppState,
    entry: &Value,
    reference_map: &mut std::collections::HashMap<String, String>,
) -> Result<(Value, Option<(String, String)>), ApiError> {
    let request = &entry["request"];
    let method = request["method"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.method in bundle entry"))?;
    let url = request["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.url in bundle entry"))?;

    let full_url = entry["fullUrl"].as_str();

    // Resolve references in the resource if present
    let resource = if let Some(res) = entry.get("resource") {
        Some(resolve_bundle_references(res, reference_map)?)
    } else {
        None
    };

    match method.to_uppercase().as_str() {
        "POST" => process_post_entry(state, url, resource, full_url, reference_map).await,
        "PUT" => process_put_entry(state, url, resource, request).await,
        "DELETE" => process_delete_entry(state, url).await,
        "GET" => process_get_entry(state, url).await,
        "PATCH" => process_patch_entry(state, url, resource, request).await,
        _ => Err(ApiError::bad_request(format!(
            "Unknown HTTP method in bundle entry: {}",
            method
        ))),
    }
}

/// Process POST (create) entry
async fn process_post_entry(
    state: &crate::server::AppState,
    url: &str,
    resource: Option<Value>,
    full_url: Option<&str>,
    reference_map: &mut std::collections::HashMap<String, String>,
) -> Result<(Value, Option<(String, String)>), ApiError> {
    let mut resource =
        resource.ok_or_else(|| ApiError::bad_request("POST entry requires a resource"))?;

    // Parse URL - can be "Type" or "Type?condition"
    let (resource_type, condition) = if let Some(idx) = url.find('?') {
        (&url[..idx], Some(&url[idx + 1..]))
    } else {
        (url, None)
    };

    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resource type: {}",
            resource_type
        )));
    }

    // Handle conditional create using modern storage API
    if let Some(cond) = condition {
        let search_params = octofhir_search::parse_query_string(cond, 2, 10);

        if let Ok(result) = state.storage.search(resource_type, &search_params).await {
            match result.entries.len() {
                0 => {
                    // No match - proceed with create
                }
                1 => {
                    // Match found - return existing resource
                    let existing = &result.entries[0];

                    if let Some(fu) = full_url {
                        reference_map
                            .insert(fu.to_string(), format!("{}/{}", resource_type, existing.id));
                    }

                    let response_entry = build_transaction_response_entry(
                        Some(&existing.resource),
                        "200 OK",
                        Some(resource_type),
                        Some(&existing.id),
                        Some(&existing.version_id),
                    );

                    return Ok((response_entry, None));
                }
                _ => {
                    return Err(ApiError::precondition_failed(
                        "Multiple matches for conditional create",
                    ));
                }
            }
        }
    }

    // Ensure resourceType is set
    resource["resourceType"] = json!(resource_type);

    // Create the resource using modern storage API
    let stored = state
        .storage
        .create(&resource)
        .await
        .map_err(map_storage_error)?;

    // Update reference map
    if let Some(fu) = full_url {
        reference_map.insert(fu.to_string(), format!("{}/{}", resource_type, stored.id));
    }

    let response_entry = build_transaction_response_entry(
        Some(&stored.resource),
        "201 Created",
        Some(resource_type),
        Some(&stored.id),
        Some(&stored.version_id),
    );

    Ok((response_entry, Some((resource_type.to_string(), stored.id))))
}

/// Process PUT (update or conditional update) entry
async fn process_put_entry(
    state: &crate::server::AppState,
    url: &str,
    resource: Option<Value>,
    request: &Value,
) -> Result<(Value, Option<(String, String)>), ApiError> {
    let mut resource =
        resource.ok_or_else(|| ApiError::bad_request("PUT entry requires a resource"))?;

    // Parse URL: "Type/id" or "Type?condition"
    let parts: Vec<&str> = url.split('/').collect();

    if parts.len() == 2 && !parts[1].contains('?') {
        // Direct update: PUT Type/id
        let resource_type = parts[0];
        let id = parts[1];

        // Validate resource type
        if resource_type.parse::<ResourceType>().is_err() {
            return Err(ApiError::bad_request(format!(
                "Unknown resource type: {}",
                resource_type
            )));
        }

        // Protect the default admin access policy from modification
        if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
            return Err(ApiError::forbidden(
                "The default admin access policy cannot be modified",
            ));
        }

        // Check if resource exists using modern storage API
        let existing = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(map_storage_error)?;

        // Extract If-Match version for optimistic locking
        let if_match = request["ifMatch"].as_str().map(|im| {
            im.trim_start_matches("W/\"")
                .trim_end_matches('"')
                .trim_start_matches('"')
                .to_string()
        });

        // Handle If-Match version check
        if let (Some(expected_version), Some(existing_stored)) = (&if_match, &existing) {
            if expected_version != &existing_stored.version_id {
                return Err(ApiError::conflict(format!(
                    "Version mismatch: expected {}, found {}",
                    expected_version, existing_stored.version_id
                )));
            }
        }

        // Ensure id and resourceType are set
        resource["id"] = json!(id);
        resource["resourceType"] = json!(resource_type);

        let (status, stored) = if existing.is_some() {
            let stored = state
                .storage
                .update(&resource, if_match.as_deref())
                .await
                .map_err(map_storage_error)?;
            ("200 OK", stored)
        } else {
            let stored = state
                .storage
                .create(&resource)
                .await
                .map_err(map_storage_error)?;
            ("201 Created", stored)
        };

        let response_entry = build_transaction_response_entry(
            Some(&stored.resource),
            status,
            Some(resource_type),
            Some(id),
            Some(&stored.version_id),
        );

        let created = if status == "201 Created" {
            Some((resource_type.to_string(), id.to_string()))
        } else {
            None
        };

        Ok((response_entry, created))
    } else if url.contains('?') {
        // Conditional update: PUT Type?condition
        let (resource_type, condition) = if let Some(idx) = url.find('?') {
            (&url[..idx], &url[idx + 1..])
        } else {
            return Err(ApiError::bad_request(format!("Invalid PUT URL: {}", url)));
        };

        // Validate resource type
        if resource_type.parse::<ResourceType>().is_err() {
            return Err(ApiError::bad_request(format!(
                "Unknown resource type: {}",
                resource_type
            )));
        }

        // Search using modern storage API
        let search_params = octofhir_search::parse_query_string(condition, 2, 10);

        let result = state
            .storage
            .search(resource_type, &search_params)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;

        match result.entries.len() {
            0 => {
                // No match - create
                resource["resourceType"] = json!(resource_type);

                let stored = state
                    .storage
                    .create(&resource)
                    .await
                    .map_err(map_storage_error)?;

                let response_entry = build_transaction_response_entry(
                    Some(&stored.resource),
                    "201 Created",
                    Some(resource_type),
                    Some(&stored.id),
                    Some(&stored.version_id),
                );

                Ok((response_entry, Some((resource_type.to_string(), stored.id))))
            }
            1 => {
                // Update existing
                let existing = &result.entries[0];

                // Protect the default admin access policy from modification
                if resource_type == "AccessPolicy" && existing.id == ADMIN_ACCESS_POLICY_ID {
                    return Err(ApiError::forbidden(
                        "The default admin access policy cannot be modified",
                    ));
                }

                // Ensure id and resourceType are set
                resource["id"] = json!(existing.id.clone());
                resource["resourceType"] = json!(resource_type);

                let stored = state
                    .storage
                    .update(&resource, None)
                    .await
                    .map_err(map_storage_error)?;

                let response_entry = build_transaction_response_entry(
                    Some(&stored.resource),
                    "200 OK",
                    Some(resource_type),
                    Some(&existing.id),
                    Some(&stored.version_id),
                );

                Ok((response_entry, None))
            }
            _ => Err(ApiError::precondition_failed(
                "Multiple matches for conditional update",
            )),
        }
    } else {
        Err(ApiError::bad_request(format!("Invalid PUT URL: {}", url)))
    }
}

/// Process DELETE entry
async fn process_delete_entry(
    state: &crate::server::AppState,
    url: &str,
) -> Result<(Value, Option<(String, String)>), ApiError> {
    let parts: Vec<&str> = url.split('/').collect();

    if parts.len() == 2 && !parts[1].contains('?') {
        // Direct delete: DELETE Type/id
        let resource_type = parts[0];
        let id = parts[1];

        // Validate resource type
        if resource_type.parse::<ResourceType>().is_err() {
            return Err(ApiError::bad_request(format!(
                "Unknown resource type: {}",
                resource_type
            )));
        }

        // Protect the default admin access policy from deletion
        if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
            return Err(ApiError::forbidden(
                "The default admin access policy cannot be deleted",
            ));
        }

        // Delete using modern storage API
        let _ = state
            .storage
            .delete(resource_type, id)
            .await
            .map_err(map_storage_error)?;

        let response_entry = build_transaction_response_entry(
            None,
            "204 No Content",
            Some(resource_type),
            None,
            None,
        );

        Ok((response_entry, None))
    } else if url.contains('?') {
        // Conditional delete: DELETE Type?condition
        let (resource_type, condition) = if let Some(idx) = url.find('?') {
            (&url[..idx], &url[idx + 1..])
        } else {
            return Err(ApiError::bad_request(format!(
                "Invalid DELETE URL: {}",
                url
            )));
        };

        // Validate resource type
        if resource_type.parse::<ResourceType>().is_err() {
            return Err(ApiError::bad_request(format!(
                "Unknown resource type: {}",
                resource_type
            )));
        }

        // Search using modern storage API
        let search_params = octofhir_search::parse_query_string(condition, 2, 10);

        let result = state
            .storage
            .search(resource_type, &search_params)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;

        match result.entries.len() {
            0 => {
                // Nothing to delete - success
                let response_entry = build_transaction_response_entry(
                    None,
                    "204 No Content",
                    Some(resource_type),
                    None,
                    None,
                );
                Ok((response_entry, None))
            }
            1 => {
                let id = &result.entries[0].id;

                // Protect the default admin access policy from deletion
                if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
                    return Err(ApiError::forbidden(
                        "The default admin access policy cannot be deleted",
                    ));
                }

                let _ = state
                    .storage
                    .delete(resource_type, id)
                    .await
                    .map_err(map_storage_error)?;

                let response_entry = build_transaction_response_entry(
                    None,
                    "204 No Content",
                    Some(resource_type),
                    None,
                    None,
                );
                Ok((response_entry, None))
            }
            _ => Err(ApiError::precondition_failed(
                "Multiple matches for conditional delete",
            )),
        }
    } else {
        Err(ApiError::bad_request(format!(
            "Invalid DELETE URL: {}",
            url
        )))
    }
}

/// Process GET (read, vread, search) entry
async fn process_get_entry(
    state: &crate::server::AppState,
    url: &str,
) -> Result<(Value, Option<(String, String)>), ApiError> {
    // Check for search: Type?params
    if url.contains('?') {
        return process_get_search_entry(state, url).await;
    }

    let parts: Vec<&str> = url.split('/').collect();

    // Check for vread: Type/id/_history/version
    if parts.len() == 4 && parts[2] == "_history" {
        let resource_type = parts[0];
        let id = parts[1];
        let version = parts[3];

        // Validate resource type
        if resource_type.parse::<ResourceType>().is_err() {
            return Err(ApiError::bad_request(format!(
                "Unknown resource type: {}",
                resource_type
            )));
        }

        // Use vread for versioned access
        let stored = state
            .storage
            .vread(resource_type, id, version)
            .await
            .map_err(map_storage_error)?
            .ok_or_else(|| {
                ApiError::not_found(format!(
                    "{}/{}/_history/{} not found",
                    resource_type, id, version
                ))
            })?;

        let response_entry = build_transaction_response_entry(
            Some(&stored.resource),
            "200 OK",
            Some(resource_type),
            Some(id),
            Some(&stored.version_id),
        );

        return Ok((response_entry, None));
    }

    // Direct read: Type/id
    if parts.len() == 2 {
        let resource_type = parts[0];
        let id = parts[1];

        // Validate resource type
        if resource_type.parse::<ResourceType>().is_err() {
            return Err(ApiError::bad_request(format!(
                "Unknown resource type: {}",
                resource_type
            )));
        }

        let stored = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(map_storage_error)?
            .ok_or_else(|| ApiError::not_found(format!("{}/{} not found", resource_type, id)))?;

        let response_entry = build_transaction_response_entry(
            Some(&stored.resource),
            "200 OK",
            Some(resource_type),
            Some(id),
            Some(&stored.version_id),
        );

        Ok((response_entry, None))
    } else {
        Err(ApiError::bad_request(format!("Invalid GET URL: {}", url)))
    }
}

/// Process GET with search params in batch: Type?params
async fn process_get_search_entry(
    state: &crate::server::AppState,
    url: &str,
) -> Result<(Value, Option<(String, String)>), ApiError> {
    let (resource_type, query) = if let Some(idx) = url.find('?') {
        (&url[..idx], &url[idx + 1..])
    } else {
        return Err(ApiError::bad_request("Invalid search URL"));
    };

    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resource type: {}",
            resource_type
        )));
    }

    // Search using modern storage API
    let cfg = &state.search_cfg;
    let search_params =
        octofhir_search::parse_query_string(query, cfg.default_count as u32, cfg.max_count as u32);

    let result = state
        .storage
        .search(resource_type, &search_params)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let resources_json: Vec<Value> = result.entries.iter().map(|e| e.resource.clone()).collect();

    let offset = search_params.offset.unwrap_or(0) as usize;
    let count = search_params.count.unwrap_or(10) as usize;
    let total = result
        .total
        .map(|t| t as usize)
        .unwrap_or(resources_json.len());

    // Build a searchset bundle for the response
    let search_bundle = octofhir_api::bundle_from_search(
        total,
        resources_json,
        &state.base_url,
        resource_type,
        offset,
        count,
        None,
    );

    // Return the bundle as the response resource
    let search_bundle_json =
        serde_json::to_value(search_bundle).map_err(|e| ApiError::internal(e.to_string()))?;

    let response_entry = json!({
        "response": {
            "status": "200 OK"
        },
        "resource": search_bundle_json
    });

    Ok((response_entry, None))
}

/// Process PATCH entry
async fn process_patch_entry(
    state: &crate::server::AppState,
    url: &str,
    resource: Option<Value>,
    _request: &Value,
) -> Result<(Value, Option<(String, String)>), ApiError> {
    let patch = resource.ok_or_else(|| ApiError::bad_request("PATCH entry requires a resource"))?;

    let parts: Vec<&str> = url.split('/').collect();

    if parts.len() == 2 {
        let resource_type = parts[0];
        let id = parts[1];

        // Validate resource type
        if resource_type.parse::<ResourceType>().is_err() {
            return Err(ApiError::bad_request(format!(
                "Unknown resource type: {}",
                resource_type
            )));
        }

        // Protect the default admin access policy from modification
        if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
            return Err(ApiError::forbidden(
                "The default admin access policy cannot be modified",
            ));
        }

        // Get existing resource using modern storage API
        let existing = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(map_storage_error)?
            .ok_or_else(|| ApiError::not_found(format!("{}/{} not found", resource_type, id)))?;

        // Apply JSON Patch
        let mut patched_json = existing.resource.clone();
        let patch_ops: Vec<json_patch::PatchOperation> = serde_json::from_value(patch)
            .map_err(|e| ApiError::bad_request(format!("Invalid JSON Patch: {}", e)))?;

        json_patch::patch(&mut patched_json, &patch_ops)
            .map_err(|e| ApiError::bad_request(format!("Patch failed: {}", e)))?;

        // Ensure id and resourceType are set
        patched_json["id"] = json!(id);
        patched_json["resourceType"] = json!(resource_type);

        // Update the resource using modern storage API
        let stored = state
            .storage
            .update(&patched_json, None)
            .await
            .map_err(map_storage_error)?;

        let response_entry = build_transaction_response_entry(
            Some(&stored.resource),
            "200 OK",
            Some(resource_type),
            Some(id),
            Some(&stored.version_id),
        );

        Ok((response_entry, None))
    } else {
        Err(ApiError::bad_request(format!("Invalid PATCH URL: {}", url)))
    }
}

/// Resolve bundle references (fullUrl and urn:uuid) in a resource
fn resolve_bundle_references(
    resource: &Value,
    reference_map: &std::collections::HashMap<String, String>,
) -> Result<Value, ApiError> {
    let mut resolved = resource.clone();
    resolve_references_recursive(&mut resolved, reference_map)?;
    Ok(resolved)
}

/// Recursively resolve references in a JSON value
fn resolve_references_recursive(
    value: &mut Value,
    reference_map: &std::collections::HashMap<String, String>,
) -> Result<(), ApiError> {
    match value {
        Value::Object(map) => {
            // Check for reference field
            if let Some(ref_value) = map.get_mut("reference")
                && let Some(ref_str) = ref_value.as_str()
            {
                if let Some(resolved) = reference_map.get(ref_str) {
                    *ref_value = json!(resolved);
                } else if ref_str.starts_with("urn:uuid:") {
                    // Unresolved UUID reference - this is an error
                    return Err(ApiError::bad_request(format!(
                        "Unresolved reference: {}",
                        ref_str
                    )));
                }
                // If it's a regular reference (Type/id), leave it as-is
            }

            // Recurse into object values
            for (_, v) in map.iter_mut() {
                resolve_references_recursive(v, reference_map)?;
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                resolve_references_recursive(item, reference_map)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Build a transaction response entry
fn build_transaction_response_entry(
    resource: Option<&Value>,
    status: &str,
    resource_type: Option<&str>,
    id: Option<&str>,
    version_id: Option<&str>,
) -> Value {
    let mut entry = json!({
        "response": {
            "status": status
        }
    });

    if let Some(res) = resource {
        entry["resource"] = res.clone();
    }

    if let (Some(rt), Some(id)) = (resource_type, id) {
        let version = version_id.unwrap_or("1");
        entry["response"]["location"] = json!(format!("{}/{}/_history/{}", rt, id, version));
        entry["response"]["etag"] = json!(format!("W/\"{}\"", version));
        entry["fullUrl"] = json!(format!("{}/{}/{}", &"", rt, id)); // base_url would go here
    }

    entry
}

// ============================================================================
// Compartment Search Handlers
// ============================================================================

/// Handler for compartment search: GET /{CompartmentType}/{id}/{ResourceType}
///
/// Examples:
/// - GET /Patient/123/Observation - All observations for patient 123
/// - GET /Patient/123/Condition?clinical-status=active - Active conditions for patient 123
/// - GET /Encounter/456/Procedure - All procedures for encounter 456
pub async fn compartment_search(
    Path((compartment_type, compartment_id, resource_type)): Path<(String, String, String)>,
    RawQuery(query_string): RawQuery,
    State(state): State<crate::server::AppState>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        compartment_type = %compartment_type,
        compartment_id = %compartment_id,
        resource_type = %resource_type,
        query = ?query_string,
        "Compartment search request"
    );

    // Parse resource type
    let _rt = resource_type
        .parse::<ResourceType>()
        .map_err(|_| ApiError::BadRequest(format!("Invalid resource type: {}", resource_type)))?;

    // Get compartment definition
    let compartment_def = state
        .compartment_registry
        .get(&compartment_type)
        .map_err(|e| ApiError::NotFound(format!("Compartment not found: {}", e)))?;

    // Get inclusion parameters for this resource type
    let inclusion_params = compartment_def
        .get_inclusion_params(&resource_type)
        .ok_or_else(|| {
            ApiError::BadRequest(format!(
                "Resource type '{}' is not in {} compartment",
                resource_type, compartment_type
            ))
        })?;

    // Check if the compartment resource exists
    match state.storage.read(&compartment_type, &compartment_id).await {
        Ok(Some(_)) => {} // Resource exists
        Ok(None) => {
            return Err(ApiError::NotFound(format!(
                "Compartment resource {}/{} not found",
                compartment_type, compartment_id
            )));
        }
        Err(e) => {
            return Err(ApiError::Internal(format!("Storage error: {}", e)));
        }
    }

    // Build compartment constraint
    // The compartment constraint is: (param1=CompartmentType/id OR param2=CompartmentType/id OR ...)
    let compartment_ref = format!("{}/{}", compartment_type, compartment_id);

    // FHIR spec requires OR semantics: match if ANY param references the compartment
    // For multiple inclusion params, we need to execute searches for each param and merge results
    let base_query: String = query_string.unwrap_or_default();

    tracing::debug!(
        resource_type = %resource_type,
        compartment = %compartment_ref,
        inclusion_params = ?inclusion_params,
        base_query = %base_query,
        "Executing compartment search with OR semantics"
    );

    // Execute searches for all inclusion parameters and collect unique resources
    let mut all_resources = std::collections::HashMap::new(); // Use HashMap to deduplicate by ID
    let mut total_count = 0usize;

    for param in inclusion_params {
        // Build query with this inclusion parameter
        let mut query = base_query.clone();
        if !query.is_empty() {
            query.push('&');
        }
        query.push_str(&format!("{}={}", param, compartment_ref));

        tracing::debug!(
            param = %param,
            query = %query,
            "Searching with inclusion parameter"
        );

        // Execute search for this parameter using modern storage API
        let search_params = octofhir_search::parse_query_string(&query, 1000, 1000);
        match state.storage.search(&resource_type, &search_params).await {
            Ok(result) => {
                total_count = total_count.max(result.total.unwrap_or(0) as usize);

                // Add resources, deduplicating by ID
                for stored in result.entries {
                    if let Some(id) = stored.resource.get("id").and_then(|v| v.as_str()) {
                        all_resources.insert(id.to_string(), stored.resource);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    param = %param,
                    error = %e,
                    "Search failed for inclusion parameter, continuing with others"
                );
                // Continue with other parameters even if one fails
            }
        }
    }

    // Convert deduplicated resources to Vec
    let resources_json: Vec<Value> = all_resources.into_values().collect();
    let actual_count = resources_json.len();

    tracing::info!(
        compartment = %compartment_ref,
        resource_type = %resource_type,
        inclusion_params_count = inclusion_params.len(),
        unique_resources = actual_count,
        "Compartment search completed with OR semantics"
    );

    // Build Bundle with deduplicated results
    let bundle = octofhir_api::bundle_from_search(
        actual_count,
        resources_json,
        &state.base_url,
        &resource_type,
        0,
        actual_count,
        None,
    );

    Ok((StatusCode::OK, Json(bundle)))
}

/// Handler for wildcard compartment search: GET /{CompartmentType}/{id}/*
///
/// Returns all resources in the specified compartment.
///
/// Examples:
/// - GET /Patient/123/* - All resources in patient 123's compartment
/// - GET /Encounter/456/* - All resources in encounter 456's compartment
pub async fn compartment_search_all(
    Path((compartment_type, compartment_id)): Path<(String, String)>,
    State(state): State<crate::server::AppState>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        compartment_type = %compartment_type,
        compartment_id = %compartment_id,
        "Wildcard compartment search request"
    );

    // Get compartment definition
    let compartment_def = state
        .compartment_registry
        .get(&compartment_type)
        .map_err(|e| ApiError::NotFound(format!("Compartment not found: {}", e)))?;

    // Verify compartment resource exists
    match state.storage.read(&compartment_type, &compartment_id).await {
        Ok(Some(_)) => {} // Resource exists
        Ok(None) => {
            return Err(ApiError::NotFound(format!(
                "Compartment resource {}/{} not found",
                compartment_type, compartment_id
            )));
        }
        Err(e) => {
            return Err(ApiError::Internal(format!("Storage error: {}", e)));
        }
    }

    // Get all resource types in this compartment
    let resource_types = compartment_def.resource_types();
    let resource_types_count = resource_types.len();

    tracing::info!(
        compartment = %format!("{}/{}", compartment_type, compartment_id),
        resource_types = ?resource_types,
        count = resource_types_count,
        "Searching all resource types in compartment"
    );

    // Build searchset Bundle
    let mut all_entries = Vec::new();
    let mut total = 0usize;

    // Execute search for each resource type
    for resource_type_str in resource_types {
        if resource_type_str.parse::<ResourceType>().is_err() {
            tracing::warn!(
                resource_type = %resource_type_str,
                "Skipping unknown resource type"
            );
            continue;
        }

        // Get inclusion parameters for this resource type
        let inclusion_params = match compartment_def.get_inclusion_params(resource_type_str) {
            Some(params) => params,
            None => {
                tracing::debug!(
                    resource_type = %resource_type_str,
                    "Resource type not in compartment definition"
                );
                continue;
            }
        };

        // Build search query for this resource type
        let compartment_ref = format!("{}/{}", compartment_type, compartment_id);
        let mut query = String::new();

        // Use the first (primary) inclusion parameter
        if !inclusion_params.is_empty() {
            let param = &inclusion_params[0];
            query.push_str(&format!("{}={}", param, compartment_ref));
        }

        // Execute search using modern storage API
        let search_params = octofhir_search::parse_query_string(&query, 1000, 1000);
        match state
            .storage
            .search(resource_type_str, &search_params)
            .await
        {
            Ok(result) => {
                total += result.entries.len();

                // Add each resource as an entry in the bundle
                for stored in result.entries {
                    all_entries.push(json!({
                        "resource": stored.resource,
                        "search": {
                            "mode": "match"
                        }
                    }));
                }
            }
            Err(e) => {
                tracing::warn!(
                    resource_type = %resource_type_str,
                    error = %e,
                    "Failed to search resource type in compartment"
                );
                // Continue with other resource types
            }
        }
    }

    // Build searchset Bundle with all entries
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "searchset",
        "total": total,
        "entry": all_entries,
    });

    tracing::info!(
        total = total,
        resource_types_searched = resource_types_count,
        "Wildcard compartment search completed"
    );

    Ok((StatusCode::OK, Json(bundle)))
}

// ============================================================================
// Async Job Handlers (FHIR Asynchronous Request Pattern)
// ============================================================================

/// GET /_async-status/{job-id}
///
/// Retrieve status and progress of an asynchronous job.
/// Returns 202 Accepted if job is still in progress, 200 OK if completed.
pub async fn async_job_status(
    Path(job_id): Path<uuid::Uuid>,
    State(state): State<crate::server::AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let job = state
        .async_job_manager
        .get_job(job_id)
        .await
        .map_err(|e| match e {
            crate::async_jobs::AsyncJobError::NotFound(_) => {
                ApiError::NotFound(format!("Async job not found: {}", job_id))
            }
            _ => ApiError::Internal(format!("Failed to get job status: {}", e)),
        })?;

    // For bulk export jobs that are completed, return the manifest directly
    if job.request_type == "bulk_export"
        && job.status == crate::async_jobs::AsyncJobStatus::Completed
    {
        if let Some(result) = job.result {
            return Ok((StatusCode::OK, Json(result)).into_response());
        }
    }

    // Build response based on job status
    let response = json!({
        "jobId": job.id,
        "status": job.status,
        "progress": job.progress,
        "createdAt": job.created_at.to_rfc3339(),
        "updatedAt": job.updated_at.to_rfc3339(),
        "completedAt": job.completed_at.map(|dt| dt.to_rfc3339()),
        "expiresAt": job.expires_at.to_rfc3339(),
        "requestType": job.request_type,
        "errorMessage": job.error_message,
    });

    let status_code = match job.status {
        crate::async_jobs::AsyncJobStatus::Queued
        | crate::async_jobs::AsyncJobStatus::InProgress => StatusCode::ACCEPTED,
        crate::async_jobs::AsyncJobStatus::Completed => StatusCode::OK,
        crate::async_jobs::AsyncJobStatus::Failed => StatusCode::INTERNAL_SERVER_ERROR,
        crate::async_jobs::AsyncJobStatus::Cancelled => StatusCode::GONE,
    };

    // Add X-Progress header for in-progress jobs (FHIR Bulk Data spec)
    let mut headers = axum::http::HeaderMap::new();
    if job.status == crate::async_jobs::AsyncJobStatus::InProgress
        || job.status == crate::async_jobs::AsyncJobStatus::Queued
    {
        let progress_pct = (job.progress * 100.0).round() as u32;
        if let Ok(progress_value) = axum::http::HeaderValue::from_str(&format!(
            "Processing: {}% complete",
            progress_pct
        )) {
            headers.insert("X-Progress", progress_value);
        }
        // Also add Retry-After header to suggest when to poll again (in seconds)
        if let Ok(retry_value) = axum::http::HeaderValue::from_str("10") {
            headers.insert(axum::http::header::RETRY_AFTER, retry_value);
        }
    }

    Ok((status_code, headers, Json(response)).into_response())
}

/// GET /_async-status/{job-id}/result
///
/// Retrieve the result of a completed asynchronous job.
/// Returns 404 if job not found, 425 if job not yet completed, 200 with result if completed.
pub async fn async_job_result(
    Path(job_id): Path<uuid::Uuid>,
    State(state): State<crate::server::AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let job = state
        .async_job_manager
        .get_job(job_id)
        .await
        .map_err(|e| match e {
            crate::async_jobs::AsyncJobError::NotFound(_) => {
                ApiError::NotFound(format!("Async job not found: {}", job_id))
            }
            _ => ApiError::Internal(format!("Failed to get job: {}", e)),
        })?;

    // Check if job is completed
    match job.status {
        crate::async_jobs::AsyncJobStatus::Completed => {
            if let Some(result) = job.result {
                Ok((StatusCode::OK, Json(result)))
            } else {
                Err(ApiError::Internal(
                    "Job marked as completed but result is missing".to_string(),
                ))
            }
        }
        crate::async_jobs::AsyncJobStatus::Failed => {
            let error_msg = job
                .error_message
                .unwrap_or_else(|| "Job failed without error message".to_string());
            Err(ApiError::Internal(error_msg))
        }
        crate::async_jobs::AsyncJobStatus::Cancelled => {
            Err(ApiError::BadRequest("Job was cancelled".to_string()))
        }
        crate::async_jobs::AsyncJobStatus::Queued
        | crate::async_jobs::AsyncJobStatus::InProgress => {
            // 425 Too Early - job not yet ready
            let response = json!({
                "resourceType": "OperationOutcome",
                "issue": [{
                    "severity": "information",
                    "code": "processing",
                    "diagnostics": format!("Job is still {} (progress: {:.1}%)", job.status, job.progress * 100.0),
                }]
            });
            Ok((StatusCode::from_u16(425).unwrap(), Json(response)))
        }
    }
}

/// DELETE /_async-status/{job-id}
///
/// Cancel a queued or in-progress asynchronous job.
/// Returns 204 No Content if successfully cancelled.
pub async fn async_job_cancel(
    Path(job_id): Path<uuid::Uuid>,
    State(state): State<crate::server::AppState>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .async_job_manager
        .cancel_job(job_id)
        .await
        .map_err(|e| match e {
            crate::async_jobs::AsyncJobError::NotFound(_) => {
                ApiError::NotFound(format!("Async job not found: {}", job_id))
            }
            _ => ApiError::Internal(format!("Failed to cancel job: {}", e)),
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Helper: Create 202 Accepted response for async job submission
///
/// Returns 202 Accepted with Content-Location header pointing to the status endpoint
pub fn create_async_accepted_response(
    job_id: uuid::Uuid,
    base_url: &str,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let status_url = format!("{}/_async-status/{}", base_url, job_id);

    let response = json!({
        "resourceType": "OperationOutcome",
        "issue": [{
            "severity": "information",
            "code": "informational",
            "diagnostics": format!("Request accepted for asynchronous processing. Job ID: {}", job_id),
        }]
    });

    let mut headers = HeaderMap::new();
    if let Ok(header_value) = header::HeaderValue::from_str(&status_url) {
        headers.insert(header::CONTENT_LOCATION, header_value);
    }

    (StatusCode::ACCEPTED, headers, Json(response))
}

// ==================== Bulk Export File Serving ====================

/// GET /fhir/_bulk-files/{job_id}/{filename}
///
/// Serve NDJSON files from bulk export jobs. Returns the file content
/// with appropriate content type.
pub async fn bulk_export_file(
    State(state): State<crate::server::AppState>,
    Path((job_id, filename)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    use std::path::PathBuf;
    use tokio::fs;

    // Validate job_id is a valid UUID to prevent path traversal
    let _uuid = uuid::Uuid::parse_str(&job_id)
        .map_err(|_| ApiError::BadRequest("Invalid job ID format".to_string()))?;

    // Validate filename doesn't contain path traversal attempts
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(ApiError::BadRequest("Invalid filename".to_string()));
    }

    // Construct the file path
    let export_path = &state.config.bulk_export.export_path;
    let file_path = PathBuf::from(export_path).join(&job_id).join(&filename);

    // Check if file exists and is within the export directory
    if !file_path.exists() {
        return Err(ApiError::NotFound(format!(
            "Export file not found: {}",
            filename
        )));
    }

    // Read the file
    let contents = fs::read(&file_path)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to read export file: {}", e)))?;

    // Build response with NDJSON content type
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/fhir+ndjson"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename))
            .unwrap_or_else(|_| header::HeaderValue::from_static("attachment")),
    );

    Ok((StatusCode::OK, headers, contents))
}

// ==================== Package Management API ====================

/// Response type for package list
#[derive(Serialize)]
pub struct PackageListResponse {
    pub packages: Vec<PackageInfo>,
    #[serde(rename = "serverFhirVersion")]
    pub server_fhir_version: String,
}

/// Package information summary
#[derive(Serialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    #[serde(rename = "fhirVersion")]
    pub fhir_version: Option<String>,
    #[serde(rename = "resourceCount")]
    pub resource_count: usize,
    #[serde(rename = "installedAt")]
    pub installed_at: Option<String>,
}

/// Package detail response
#[derive(Serialize)]
pub struct PackageDetailResponse {
    pub name: String,
    pub version: String,
    #[serde(rename = "fhirVersion")]
    pub fhir_version: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "resourceCount")]
    pub resource_count: usize,
    #[serde(rename = "installedAt")]
    pub installed_at: Option<String>,
    #[serde(rename = "isCompatible")]
    pub is_compatible: bool,
    #[serde(rename = "resourceTypes")]
    pub resource_types: Vec<ResourceTypeSummary>,
}

/// Summary of resources by type
#[derive(Serialize)]
pub struct ResourceTypeSummary {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub count: usize,
}

/// Package resource summary
#[derive(Serialize)]
pub struct PackageResourceSummary {
    pub id: Option<String>,
    pub url: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(rename = "resourceType")]
    pub resource_type: String,
}

/// Package resources list response
#[derive(Serialize)]
pub struct PackageResourcesResponse {
    pub resources: Vec<PackageResourceSummary>,
    pub total: usize,
}

/// Query parameters for package resources
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PackageResourcesQuery {
    /// Filter by resource type
    #[serde(rename = "resourceType")]
    pub resource_type: Option<String>,
    /// Limit results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

/// GET /api/packages - List all installed packages
pub async fn api_packages_list(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    use octofhir_canonical_manager::traits::PackageStore;
    use octofhir_db_postgres::PostgresPackageStore;

    let store = PostgresPackageStore::new(state.db_pool.as_ref().clone());

    match store.list_packages().await {
        Ok(packages) => {
            let package_infos: Vec<PackageInfo> = packages
                .into_iter()
                .map(|p| PackageInfo {
                    name: p.name,
                    version: p.version,
                    fhir_version: Some(p.fhir_version),
                    resource_count: p.resource_count,
                    installed_at: Some(p.installed_at.to_rfc3339()),
                })
                .collect();

            (
                StatusCode::OK,
                Json(PackageListResponse {
                    packages: package_infos,
                    server_fhir_version: state.fhir_version.clone(),
                }),
            )
        }
        Err(e) => {
            tracing::error!("Failed to list packages: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(PackageListResponse {
                    packages: Vec::new(),
                    server_fhir_version: state.fhir_version.clone(),
                }),
            )
        }
    }
}

/// GET /api/packages/:name/:version - Get package details
pub async fn api_packages_get(
    State(state): State<crate::server::AppState>,
    Path((name, version)): Path<(String, String)>,
) -> Result<Json<PackageDetailResponse>, ApiError> {
    use sqlx_core::query::query;
    use sqlx_core::row::Row;

    // Get package info
    let row = query(
        r#"
        SELECT name, version, fhir_version, resource_count, installed_at
        FROM fcm.packages
        WHERE name = $1 AND version = $2
        "#,
    )
    .bind(&name)
    .bind(&version)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::Internal(format!("Database error: {}", e)))?;

    let Some(pkg_row) = row else {
        return Err(ApiError::NotFound(format!(
            "Package not found: {}@{}",
            name, version
        )));
    };

    let fhir_version: Option<String> = pkg_row.get("fhir_version");
    let resource_count: i32 = pkg_row.get("resource_count");
    let installed_at: chrono::DateTime<chrono::Utc> = pkg_row.get("installed_at");

    // Get resource type summary
    let type_rows = query(
        r#"
        SELECT resource_type, COUNT(*) as count
        FROM fcm.resources
        WHERE package_name = $1 AND package_version = $2
        GROUP BY resource_type
        ORDER BY resource_type
        "#,
    )
    .bind(&name)
    .bind(&version)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::Internal(format!("Database error: {}", e)))?;

    let resource_types: Vec<ResourceTypeSummary> = type_rows
        .iter()
        .map(|row| {
            let count: i64 = row.get("count");
            ResourceTypeSummary {
                resource_type: row.get("resource_type"),
                count: count as usize,
            }
        })
        .collect();

    // Check FHIR version compatibility
    let is_compatible = fhir_version
        .as_ref()
        .map(|fv| {
            let server_major = extract_fhir_major_version(&state.fhir_version);
            let pkg_major = extract_fhir_major_version(fv);
            server_major == pkg_major
        })
        .unwrap_or(true);

    Ok(Json(PackageDetailResponse {
        name,
        version,
        fhir_version,
        description: None, // TODO: Add description to package manifest
        resource_count: resource_count as usize,
        installed_at: Some(installed_at.to_rfc3339()),
        is_compatible,
        resource_types,
    }))
}

/// Extract major FHIR version (e.g., "4.0.1" -> "R4", "5.0.0" -> "R5")
fn extract_fhir_major_version(version: &str) -> &str {
    match version {
        v if v.starts_with("4.0") => "R4",
        v if v.starts_with("4.3") => "R4B",
        v if v.starts_with("5.") => "R5",
        v if v.starts_with("6.") => "R6",
        "R4" | "R4B" | "R5" | "R6" => version,
        _ => "unknown",
    }
}

/// GET /api/packages/:name/:version/resources - List package resources
pub async fn api_packages_resources(
    State(state): State<crate::server::AppState>,
    Path((name, version)): Path<(String, String)>,
    Query(params): Query<PackageResourcesQuery>,
) -> Result<Json<PackageResourcesResponse>, ApiError> {
    use sqlx_core::query::query;
    use sqlx_core::row::Row;

    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.offset.unwrap_or(0);

    // Build query with optional filter
    let (sql, bind_type) = if let Some(ref rt) = params.resource_type {
        (
            r#"
            SELECT resource_type, resource_id, url, name, version
            FROM fcm.resources
            WHERE package_name = $1 AND package_version = $2 AND resource_type = $3
            ORDER BY resource_type, name, url
            LIMIT $4 OFFSET $5
            "#,
            Some(rt.clone()),
        )
    } else {
        (
            r#"
            SELECT resource_type, resource_id, url, name, version
            FROM fcm.resources
            WHERE package_name = $1 AND package_version = $2
            ORDER BY resource_type, name, url
            LIMIT $3 OFFSET $4
            "#,
            None,
        )
    };

    let rows = if let Some(rt) = bind_type {
        query(sql)
            .bind(&name)
            .bind(&version)
            .bind(rt)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(state.db_pool.as_ref())
            .await
    } else {
        query(sql)
            .bind(&name)
            .bind(&version)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(state.db_pool.as_ref())
            .await
    }
    .map_err(|e| ApiError::Internal(format!("Database error: {}", e)))?;

    let resources: Vec<PackageResourceSummary> = rows
        .iter()
        .map(|row| PackageResourceSummary {
            id: row.get("resource_id"),
            url: row.get("url"),
            name: row.get("name"),
            version: row.get("version"),
            resource_type: row.get("resource_type"),
        })
        .collect();

    // Get total count
    let count_sql = if params.resource_type.is_some() {
        "SELECT COUNT(*) FROM fcm.resources WHERE package_name = $1 AND package_version = $2 AND resource_type = $3"
    } else {
        "SELECT COUNT(*) FROM fcm.resources WHERE package_name = $1 AND package_version = $2"
    };

    let total: i64 = if let Some(ref rt) = params.resource_type {
        sqlx_core::query_scalar::query_scalar(count_sql)
            .bind(&name)
            .bind(&version)
            .bind(rt)
            .fetch_one(state.db_pool.as_ref())
            .await
            .unwrap_or(0)
    } else {
        sqlx_core::query_scalar::query_scalar(count_sql)
            .bind(&name)
            .bind(&version)
            .fetch_one(state.db_pool.as_ref())
            .await
            .unwrap_or(0)
    };

    Ok(Json(PackageResourcesResponse {
        resources,
        total: total as usize,
    }))
}

/// GET /api/packages/:name/:version/resources/:url - Get resource content by URL
pub async fn api_packages_resource_content(
    State(state): State<crate::server::AppState>,
    Path((name, version, url)): Path<(String, String, String)>,
) -> Result<Json<Value>, ApiError> {
    use sqlx_core::query::query;
    use sqlx_core::row::Row;

    // URL is encoded in the path, decode it
    let decoded_url = urlencoding::decode(&url)
        .map_err(|e| ApiError::BadRequest(format!("Invalid URL encoding: {}", e)))?;

    let row = query(
        r#"
        SELECT content
        FROM fcm.resources
        WHERE package_name = $1 AND package_version = $2 AND url = $3
        "#,
    )
    .bind(&name)
    .bind(&version)
    .bind(decoded_url.as_ref())
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::Internal(format!("Database error: {}", e)))?;

    match row {
        Some(r) => {
            let content: Value = r.get("content");
            Ok(Json(content))
        }
        None => Err(ApiError::NotFound(format!(
            "Resource not found: {} in {}@{}",
            decoded_url, name, version
        ))),
    }
}

/// GET /api/packages/:name/:version/fhirschema/:url - Get FHIRSchema for a resource
pub async fn api_packages_fhirschema(
    State(state): State<crate::server::AppState>,
    Path((name, version, url)): Path<(String, String, String)>,
) -> Result<Json<Value>, ApiError> {
    use octofhir_db_postgres::PostgresPackageStore;

    let store = PostgresPackageStore::new(state.db_pool.as_ref().clone());

    // URL is encoded in the path, decode it
    let decoded_url = urlencoding::decode(&url)
        .map_err(|e| ApiError::BadRequest(format!("Invalid URL encoding: {}", e)))?;

    // Try to get the FHIRSchema from the database
    match store
        .get_fhirschema_from_package(&decoded_url, &name, &version)
        .await
    {
        Ok(Some(schema)) => Ok(Json(schema.content)),
        Ok(None) => {
            // Schema not found - could convert on-demand here, but for now return 404
            Err(ApiError::NotFound(format!(
                "FHIRSchema not found for {} in {}@{}. Schema may need to be converted.",
                decoded_url, name, version
            )))
        }
        Err(e) => Err(ApiError::Internal(format!("Database error: {}", e))),
    }
}

// ============================================================================
// Package Installation API
// ============================================================================

/// Request body for package installation
#[derive(Debug, Deserialize)]
pub struct PackageInstallRequest {
    /// Package name (e.g., "hl7.fhir.us.core")
    pub name: String,
    /// Package version (e.g., "6.1.0")
    pub version: String,
}

async fn refresh_resource_type_cache(state: &crate::server::AppState) {
    match state.model_provider.get_resource_types().await {
        Ok(resource_types) => {
            let count = resource_types.len();
            let new_set: HashSet<String> = resource_types.into_iter().collect();
            state.resource_type_set.store(Arc::new(new_set));
            state.model_provider.invalidate_schema_caches();
            tracing::info!(resource_types = count, "Resource type cache refreshed");
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to refresh resource type cache after package install"
            );
        }
    }
}

/// Information about an installed dependency package
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledDependencyInfo {
    pub name: String,
    pub version: String,
    pub fhir_version: String,
    pub resource_count: usize,
}

/// Response for successful package installation
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInstallResponse {
    pub success: bool,
    pub name: String,
    pub version: String,
    pub fhir_version: String,
    pub resource_count: usize,
    /// Dependencies that were also installed
    pub dependencies_installed: Vec<InstalledDependencyInfo>,
    pub message: String,
}

/// POST /api/packages/install - Install a package at runtime
///
/// This endpoint allows installing FHIR packages without restarting the server.
/// The package will be downloaded from the FHIR package registry, installed,
/// and the search registry will be rebuilt.
///
/// # Request Body
/// ```json
/// {
///   "name": "hl7.fhir.us.core",
///   "version": "6.1.0"
/// }
/// ```
///
/// # Response
/// Returns installation status and package details on success.
pub async fn api_packages_install(
    State(state): State<crate::server::AppState>,
    Json(request): Json<PackageInstallRequest>,
) -> Result<Json<PackageInstallResponse>, ApiError> {
    use crate::canonical::install_package_parallel_runtime;

    // Validate input
    if request.name.trim().is_empty() {
        return Err(ApiError::BadRequest("Package name is required".to_string()));
    }
    if request.version.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "Package version is required".to_string(),
        ));
    }

    // Get server FHIR version from config
    let server_fhir_version = &state.config.fhir.version;

    tracing::info!(
        package = %request.name,
        version = %request.version,
        server_fhir_version = %server_fhir_version,
        "API request to install package"
    );

    // Install the package with parallel download/extraction
    match install_package_parallel_runtime(&request.name, &request.version, server_fhir_version)
        .await
    {
        Ok(result) => {
            let deps_count = result.dependencies_installed.len();
            tracing::info!(
                package = %result.name,
                version = %result.version,
                fhir_version = %result.fhir_version,
                resource_count = result.resource_count,
                dependencies = deps_count,
                "Package installed successfully via API"
            );

            refresh_resource_type_cache(&state).await;

            let deps_info: Vec<InstalledDependencyInfo> = result
                .dependencies_installed
                .into_iter()
                .map(|d| InstalledDependencyInfo {
                    name: d.name,
                    version: d.version,
                    fhir_version: d.fhir_version,
                    resource_count: d.resource_count,
                })
                .collect();

            let message = if deps_count > 0 {
                format!(
                    "Package installed with {} resources and {} dependencies.",
                    result.resource_count, deps_count
                )
            } else {
                format!(
                    "Package installed with {} resources.",
                    result.resource_count
                )
            };

            Ok(Json(PackageInstallResponse {
                success: true,
                name: result.name,
                version: result.version,
                fhir_version: result.fhir_version,
                resource_count: result.resource_count,
                dependencies_installed: deps_info,
                message,
            }))
        }
        Err(e) => {
            tracing::error!(
                package = %request.name,
                version = %request.version,
                error = %e,
                "Failed to install package via API"
            );

            // Check if it's a FHIR version mismatch (user error) or internal error
            if e.contains("FHIR version mismatch") || e.contains("failed to fetch package metadata")
            {
                Err(ApiError::BadRequest(e))
            } else {
                Err(ApiError::Internal(e))
            }
        }
    }
}

// ============================================================================
// Package Install with SSE Progress
// ============================================================================

/// POST /api/packages/install/stream - Install package with SSE progress streaming
///
/// This endpoint installs a FHIR package and streams progress events via Server-Sent Events (SSE).
/// The client receives real-time updates about the installation progress including:
/// - Dependency resolution
/// - Download progress for each package
/// - Extraction and indexing progress
/// - Final completion or error status
///
/// # Request Body
/// ```json
/// {
///   "name": "hl7.fhir.us.core",
///   "version": "6.1.0"
/// }
/// ```
///
/// # Response
/// Server-Sent Events stream with JSON payloads:
/// ```
/// data: {"type":"started","total_packages":3}
/// data: {"type":"download_started","package":"hl7.fhir.r4.core","version":"4.0.1","current":1,"total":3}
/// data: {"type":"download_progress","package":"hl7.fhir.r4.core","version":"4.0.1","downloaded_bytes":1024,"total_bytes":5000,"percent":20}
/// data: {"type":"completed","total_installed":3,"total_resources":1500,"duration_ms":5000}
/// ```
pub async fn api_packages_install_stream(
    State(state): State<crate::server::AppState>,
    Json(request): Json<PackageInstallRequest>,
) -> Result<
    axum::response::sse::Sse<
        impl futures::stream::Stream<
            Item = Result<axum::response::sse::Event, std::convert::Infallible>,
        >,
    >,
    ApiError,
> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::stream::StreamExt;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    // Validate input
    if request.name.trim().is_empty() {
        return Err(ApiError::BadRequest("Package name is required".to_string()));
    }
    if request.version.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "Package version is required".to_string(),
        ));
    }

    tracing::info!(
        package = %request.name,
        version = %request.version,
        "API request to install package with SSE progress"
    );

    // Start installation with progress
    let receiver =
        crate::canonical::install_package_runtime_with_progress(&request.name, &request.version)
            .await
            .map_err(|e| ApiError::Internal(e))?;

    // Convert receiver to SSE stream
    let refresh_state = state.clone();
    let stream = UnboundedReceiverStream::new(receiver).map(move |event| {
        if matches!(
            event,
            octofhir_canonical_manager::InstallEvent::Completed { .. }
        ) {
            let refresh_state = refresh_state.clone();
            tokio::spawn(async move {
                refresh_resource_type_cache(&refresh_state).await;
            });
        }
        let json = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Ok::<_, std::convert::Infallible>(Event::default().data(json))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ============================================================================
// Package Registry Lookup API
// ============================================================================

/// Response for package version lookup
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageLookupResponse {
    /// Package name
    pub name: String,
    /// Available versions (sorted by semver, newest first)
    pub versions: Vec<String>,
    /// Whether this package is already installed (any version)
    pub installed_versions: Vec<String>,
}

/// GET /api/packages/lookup/:name - Lookup available versions for a package
///
/// This endpoint queries the FHIR package registry to find available versions
/// of a specific package. It also indicates which versions are already installed.
///
/// # Path Parameters
/// * `name` - Package name (e.g., "hl7.fhir.us.core")
///
/// # Response
/// Returns available versions and installation status.
pub async fn api_packages_lookup(
    State(state): State<crate::server::AppState>,
    Path(name): Path<String>,
) -> Result<Json<PackageLookupResponse>, ApiError> {
    use crate::canonical::lookup_package_versions;
    use octofhir_canonical_manager::traits::PackageStore;
    use octofhir_db_postgres::PostgresPackageStore;

    tracing::info!(package = %name, "API request to lookup package versions");

    // Lookup available versions from registry
    let versions = lookup_package_versions(&name).await.map_err(|e| {
        if e.contains("not found") || e.contains("PackageNotFound") {
            ApiError::NotFound(format!("Package '{}' not found in registry", name))
        } else {
            ApiError::Internal(e)
        }
    })?;

    // Check which versions are already installed
    let store = PostgresPackageStore::new(state.db_pool.as_ref().clone());
    let installed_versions = match store.list_packages().await {
        Ok(packages) => packages
            .into_iter()
            .filter(|p| p.name == name)
            .map(|p| p.version)
            .collect(),
        Err(_) => vec![],
    };

    Ok(Json(PackageLookupResponse {
        name,
        versions,
        installed_versions,
    }))
}

// ============================================================================
// Package Registry Search API
// ============================================================================

/// Search result for a package from the registry
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageSearchResult {
    /// Package name
    pub name: String,
    /// Available versions (sorted by semver, newest first)
    pub versions: Vec<String>,
    /// Package description
    pub description: Option<String>,
    /// Latest version
    pub latest_version: String,
}

/// Response for package search
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageSearchResponse {
    /// Search query that was used
    pub query: String,
    /// Matching packages
    pub packages: Vec<PackageSearchResult>,
    /// Total number of results
    pub total: usize,
}

/// GET /api/packages/search?q=... - Search for packages in the registry
///
/// This endpoint searches the FHIR package registry (fs.get-ig.org) for packages
/// matching the query string. The search supports partial matching (ILIKE) -
/// spaces in the query are treated as wildcards for fuzzy matching.
///
/// # Query Parameters
/// * `q` - Search query string (e.g., "us core", "hl7.fhir")
///
/// # Response
/// Returns list of matching packages with their versions and descriptions.
pub async fn api_packages_search(
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<PackageSearchResponse>, ApiError> {
    use crate::canonical::search_registry_packages;

    let query = params
        .get("q")
        .map(|s| s.as_str())
        .unwrap_or("")
        .to_string();

    if query.len() < 2 {
        return Err(ApiError::BadRequest(
            "Search query must be at least 2 characters".to_string(),
        ));
    }

    tracing::info!(query = %query, "API request to search packages");

    let results = search_registry_packages(&query).await.map_err(|e| {
        tracing::error!(query = %query, error = %e, "Failed to search packages");
        ApiError::Internal(e)
    })?;

    let total = results.len();
    let packages: Vec<PackageSearchResult> = results
        .into_iter()
        .map(|r| PackageSearchResult {
            name: r.name,
            versions: r.versions,
            description: r.description,
            latest_version: r.latest_version,
        })
        .collect();

    Ok(Json(PackageSearchResponse {
        query,
        packages,
        total,
    }))
}

// =============================================================================
// Internal Resource Handlers
// =============================================================================

/// Internal resource types that are served at the root level (not under /fhir).
/// These are administrative resources defined in the octofhir-auth IG.
const INTERNAL_RESOURCE_TYPES: &[&str] = &[
    "User",
    "Role",
    "Client",
    "AccessPolicy",
    "IdentityProvider",
    "CustomOperation",
    "App",
];

/// Check if a resource type is an internal resource type.
fn is_internal_resource_type(resource_type: &str) -> bool {
    INTERNAL_RESOURCE_TYPES.contains(&resource_type)
}

/// GET /{resource_type} - Search internal resources.
///
/// This handler validates that the resource type is an internal resource type
/// before delegating to the standard search handler.
pub async fn internal_search_resource(
    state: State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    query: Query<HashMap<String, String>>,
    raw: RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    if !is_internal_resource_type(&resource_type) {
        return Err(ApiError::not_found(format!(
            "Resource type '{}' is not available at this path",
            resource_type
        )));
    }

    search_resource(state, Path(resource_type), query, raw).await
}

/// POST /{resource_type} - Create internal resource.
///
/// This handler validates that the resource type is an internal resource type
/// before delegating to the standard create handler.
pub async fn internal_create_resource(
    state: State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    headers: HeaderMap,
    payload: Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_internal_resource_type(&resource_type) {
        return Err(ApiError::not_found(format!(
            "Resource type '{}' is not available at this path",
            resource_type
        )));
    }

    create_resource(state, Path(resource_type), headers, payload).await
}

/// GET /{resource_type}/{id} - Read internal resource.
///
/// This handler validates that the resource type is an internal resource type
/// before delegating to the standard read handler.
pub async fn internal_read_resource(
    state: State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    if !is_internal_resource_type(&resource_type) {
        return Err(ApiError::not_found(format!(
            "Resource type '{}' is not available at this path",
            resource_type
        )));
    }

    read_resource(state, Path((resource_type, id)), headers).await
}

/// PUT /{resource_type}/{id} - Update internal resource.
///
/// This handler validates that the resource type is an internal resource type
/// before delegating to the standard update handler.
pub async fn internal_update_resource(
    state: State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
    payload: Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_internal_resource_type(&resource_type) {
        return Err(ApiError::not_found(format!(
            "Resource type '{}' is not available at this path",
            resource_type
        )));
    }

    update_resource(state, Path((resource_type, id)), headers, payload).await
}

/// DELETE /{resource_type}/{id} - Delete internal resource.
///
/// This handler validates that the resource type is an internal resource type
/// before delegating to the standard delete handler.
pub async fn internal_delete_resource(
    state: State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_internal_resource_type(&resource_type) {
        return Err(ApiError::not_found(format!(
            "Resource type '{}' is not available at this path",
            resource_type
        )));
    }

    delete_resource(state, Path((resource_type, id))).await
}
