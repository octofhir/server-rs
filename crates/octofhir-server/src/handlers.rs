use crate::bootstrap::ADMIN_ACCESS_POLICY_ID;
use crate::gateway::{App, PathValidationError, validate_app_operations};
use crate::mapping::{IdPolicy, validate_payload_structure};
use crate::patch::{apply_fhirpath_patch, apply_json_patch};
use crate::server::SharedModelProvider;
use axum::body::{Body, Bytes};
use axum::http::Request;
use axum::response::Response;
use axum::{
    Extension, Json,
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use include_dir::{Dir, include_dir};
use mime_guess::MimeGuess;
use octofhir_api::ApiError;
use octofhir_core::ResourceType;
use octofhir_fhir_model::ModelProvider;
use octofhir_storage::StorageError;
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

/// Apply the same identity contract before any Bundle path normalizes a body.
fn validate_bundle_entry_identity(entry: &Value) -> Result<(), ApiError> {
    if let Some(full_url) = entry.get("fullUrl") {
        let full_url = full_url
            .as_str()
            .filter(|url| !url.is_empty() && !url.chars().any(char::is_whitespace))
            .ok_or_else(|| ApiError::bad_request("Bundle fullUrl must be an absolute URI"))?;
        let parsed = url::Url::parse(full_url)
            .map_err(|_| ApiError::bad_request("Bundle fullUrl must be an absolute URI"))?;
        if full_url.contains("/_history/") || parsed.fragment().is_some() {
            return Err(ApiError::bad_request(
                "Bundle fullUrl must be version independent and have no fragment",
            ));
        }
        if matches!(parsed.scheme(), "http" | "https") {
            let segments: Vec<_> = parsed
                .path()
                .trim_end_matches('/')
                .rsplit('/')
                .take(2)
                .collect();
            if let [id, resource_type] = segments.as_slice()
                && resource_type.parse::<ResourceType>().is_ok()
                && let Some(body) = entry.get("resource")
                && (body["resourceType"].as_str() != Some(*resource_type)
                    || body
                        .get("id")
                        .is_some_and(|body_id| body_id.as_str() != Some(*id)))
            {
                return Err(ApiError::bad_request(
                    "Bundle fullUrl disagrees with the resource identity",
                ));
            }
        }
    }
    let method = entry["request"]["method"]
        .as_str()
        .unwrap_or("")
        .to_uppercase();
    if !matches!(method.as_str(), "POST" | "PUT") {
        return Ok(());
    }
    let resource = entry
        .get("resource")
        .ok_or_else(|| ApiError::bad_request("POST/PUT entry requires a resource"))?;
    let url = entry["request"]["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.url"))?;
    let path = url.split('?').next().unwrap_or(url);
    let segments: Vec<_> = path.split('/').collect();
    let resource_type = segments.first().copied().unwrap_or("");
    if method == "PUT" && url.contains('?') {
        match resource.get("id") {
            None | Some(Value::Null) => {}
            Some(Value::String(id)) => octofhir_core::validate_id(id)
                .map_err(|error| ApiError::bad_request(error.to_string()))?,
            Some(_) => {
                return Err(ApiError::bad_request(
                    "Conditional PUT id must be a FHIR id",
                ));
            }
        }
    }
    let policy = if method == "PUT" && !url.contains('?') && segments.len() == 2 {
        IdPolicy::Update {
            path_id: segments[1].to_string(),
        }
    } else {
        IdPolicy::Create
    };
    validate_payload_structure(resource_type, resource, &policy).map_err(ApiError::bad_request)
}

async fn validate_bundle_writes(
    state: &crate::server::AppState,
    resources: &[&Value],
    candidates: Arc<HashMap<String, Arc<Value>>>,
) -> Result<LocalReferenceSet, ApiError> {
    let resolver = Arc::new(
        crate::operations::validation_reference::ValidationReferenceResolver::for_write_graph(
            state,
            candidates.clone(),
        ),
    );
    let mut references = LocalReferenceSet::default();
    let mut groups: HashMap<String, HashSet<String>> = HashMap::new();
    for resource in resources {
        let local = local_reference_set(resource, state)?;
        let profiles = crate::validation::ValidationService::schemas_for(
            resource,
            resource["resourceType"].as_str().unwrap_or_default(),
        );
        let outcome = state
            .validation_service
            .validate_profiles_with_resolver(
                resource,
                &profiles,
                Some(&local.aliases),
                resolver.clone(),
            )
            .await;
        if !outcome.valid {
            return Err(validation_error(outcome));
        }
        // Targets written by this same transaction exist at commit by
        // construction; only references leaving the bundle need a lookup.
        references.all.extend(
            local
                .all
                .into_iter()
                .filter(|reference| !candidates.contains_key(reference)),
        );
        references.versions.extend(
            local
                .versions
                .into_iter()
                .filter(|reference| !candidates.contains_key(&reference.to_versioned())),
        );
        references.aliases.extend(local.aliases);
        for (resource_type, ids) in local.groups {
            let external: Vec<String> = ids
                .into_iter()
                .filter(|id| !candidates.contains_key(&format!("{resource_type}/{id}")))
                .collect();
            groups.entry(resource_type).or_default().extend(external);
        }
    }
    groups.retain(|_, ids| !ids.is_empty());
    if resolver.take_error().is_some() {
        return Err(ApiError::internal("Reference integrity lookup failed"));
    }
    for (reference, body) in resolver.write_snapshots(&Value::Null).await {
        if reference.version.is_none() && candidates.contains_key(&reference.to_relative()) {
            continue;
        }
        references.all.insert(reference.to_versioned());
        if reference.version.is_some() {
            references.versions.push(reference.clone());
        } else {
            groups
                .entry(reference.resource_type.clone())
                .or_default()
                .insert(reference.id.clone());
        }
        references.snapshots.push((reference, body));
    }
    references.groups = groups
        .into_iter()
        .map(|(resource_type, ids)| {
            let mut ids: Vec<_> = ids.into_iter().collect();
            ids.sort();
            (resource_type, ids)
        })
        .collect();
    references
        .groups
        .sort_by(|left, right| left.0.cmp(&right.0));
    references
        .versions
        .sort_by_key(octofhir_core::FhirReference::to_versioned);
    references.versions.dedup();
    Ok(references)
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

/// GET /fhir or GET /fhir?_type=Patient,Observation - Combined root and system search handler
///
/// Searches the specified `_type` values, or all supported FHIR types when omitted.
#[tracing::instrument(name = "fhir.root", skip_all)]
pub async fn fhir_root(
    state: State<crate::server::AppState>,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
    params: Query<HashMap<String, String>>,
    raw: RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    system_search(state, visibility, headers, params, raw)
        .await
        .map(|r| r.into_response())
}

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" }))
}

pub async fn readyz(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    // Check database connectivity with a lightweight query
    let db_ok = sqlx_core::executor::Executor::execute(
        &*state.db_pool,
        sqlx_core::query::query("SELECT 1"),
    )
    .await
    .is_ok();

    // Check canonical manager is loaded
    let canonical_ok = crate::canonical::get_manager().is_some();

    if db_ok && canonical_ok {
        (
            StatusCode::OK,
            Json(json!({
                "status": "ready",
                "checks": { "database": "ok", "canonical_manager": "ok" }
            })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "not_ready",
                "checks": {
                    "database": if db_ok { "ok" } else { "failed" },
                    "canonical_manager": if canonical_ok { "ok" } else { "not_loaded" }
                }
            })),
        )
    }
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
        None => (StatusCode::SERVICE_UNAVAILABLE, "Metrics not initialized").into_response(),
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
    let mut body = (*state.capability_statement).clone();
    // Registry reloads must be reflected immediately, without rebuilding profiles
    // or querying canonical storage for every metadata request.
    refresh_capability_search(&mut body, &state.search_config.config().registry);

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
///
/// Reuses the already-loaded `SearchParameterRegistry` from `ReloadableSearchConfig`
/// instead of re-querying the canonical manager (single source of truth).
pub async fn build_capability_statement(
    fhir_version: octofhir_core::fhir::FhirVersion,
    base_url: &str,
    resource_types: &[String],
    search_registry: &octofhir_search::SearchParameterRegistry,
    operations: &crate::operations::OperationRegistry,
    handlers: &HashMap<String, crate::operations::DynOperationHandler>,
    graphql_enabled: bool,
) -> Result<Value, serde_json::Error> {
    use octofhir_api::CapabilityStatementBuilder;

    // Build base CapabilityStatement per spec
    let mut builder = CapabilityStatementBuilder::new_json_r4b()
        .status("active")
        .kind("instance")
        .date(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .add_format("application/fhir+json")
        .add_format("application/json")
        .with_interactions(&["transaction", "batch", "search-system", "history-system"])
        .add_patch_format("application/json-patch+json")
        .add_patch_format("application/fhir+json");

    // Apply FHIR version field
    builder = builder.fhir_version(fhir_version.version());

    // Installed profiles are validation inputs, not a conformance declaration.
    // Advertise the base resource contract only; IG conformance needs explicit
    // configuration and validation of the corresponding server behavior.

    // Build resources using pre-fetched data
    // Filter out non-standard resource types that are not part of the FHIR spec
    // (e.g., AccessPolicy, App, Client, etc.) — these fail CapabilityStatement validation (cpb-12)
    let standard_resource_types: Vec<&str> = resource_types
        .iter()
        .filter(|rt| {
            // Only include standard FHIR resource types in the CapabilityStatement.
            // Custom types (AccessPolicy, App, Client, etc.) are excluded to pass
            // FHIR validation against the ResourceType value set.
            octofhir_api::is_standard_fhir_resource(rt)
        })
        .map(|s| s.as_str())
        .collect();

    for rt in &standard_resource_types {
        let mapped = capability_search_params(search_registry, rt);

        let resource = octofhir_api::CapabilityStatementRestResource::new(*rt)
            .with_interactions(
                &[
                    "read",
                    "vread",
                    "search-type",
                    "create",
                    "update",
                    "patch",
                    "delete",
                    "history-instance",
                    "history-type",
                ][..],
            )
            .with_versioning("versioned-update")
            .with_read_history(true)
            .with_update_create(true)
            .with_conditional_create(true)
            .with_conditional_read("full-support")
            .with_conditional_update(true)
            .with_conditional_delete("single")
            .with_search_params(mapped)
            .with_profile(Some(format!(
                "http://hl7.org/fhir/StructureDefinition/{rt}"
            )));
        builder = builder.add_resource_struct(resource);
    }

    let cs = builder.build();

    // Add software section consistent with spec
    let mut body = serde_json::to_value(&cs)?;
    body["software"] = json!({ "name": "OctoFHIR", "version": env!("CARGO_PKG_VERSION") });
    body["name"] = json!("OctoFHIR");
    body["version"] = json!(env!("CARGO_PKG_VERSION"));
    body["url"] = json!(format!("{}/fhir/metadata", base_url.trim_end_matches('/')));
    apply_capability_operations(&mut body, operations, handlers, graphql_enabled);

    // Add implementation section
    body["implementation"] = json!({
        "description": "OctoFHIR FHIR Server",
        "url": format!("{}/fhir", base_url.trim_end_matches('/'))
    });

    // Note: SMART security extensions (oauth-uris) are added at startup
    // via octofhir_auth::add_smart_security() in server.rs

    // Note: loaded canonical packages are available via /api/packages endpoint.
    // Custom extensions (urn:abyxon:loaded-package) were removed from the
    // CapabilityStatement to avoid FHIR validation errors for unknown extensions.

    Ok(body)
}

fn capability_search_params(
    registry: &octofhir_search::SearchParameterRegistry,
    resource_type: &str,
) -> Vec<octofhir_api::SearchParam> {
    let mut mapped: Vec<_> = registry
        .get_all_for_type(resource_type)
        .iter()
        .map(|p| octofhir_api::SearchParam {
            name: p.code.clone(),
            type_: format!("{:?}", p.param_type).to_lowercase(),
            documentation: (!p.description.is_empty()).then(|| p.description.clone()),
        })
        .collect();
    for common in octofhir_api::common_search_params() {
        if !mapped.iter().any(|p| p.name == common.name) {
            mapped.push(common);
        }
    }
    mapped.sort_by(|a, b| a.name.cmp(&b.name));
    mapped.dedup_by(|a, b| a.name == b.name);
    mapped
}

fn refresh_capability_search(
    body: &mut Value,
    registry: &octofhir_search::SearchParameterRegistry,
) {
    for rest in body["rest"].as_array_mut().into_iter().flatten() {
        for resource in rest["resource"].as_array_mut().into_iter().flatten() {
            if let Some(resource_type) = resource["type"].as_str() {
                let params = capability_search_params(registry, resource_type);
                resource["searchParam"] =
                    serde_json::to_value(params).expect("search parameter serialization");
            }
        }
    }
}

fn apply_capability_operations(
    body: &mut Value,
    registry: &crate::operations::OperationRegistry,
    handlers: &HashMap<String, crate::operations::DynOperationHandler>,
    graphql_enabled: bool,
) {
    use crate::operations::handler::OperationLevel;
    fn declared(
        registry: &crate::operations::OperationRegistry,
        handlers: &HashMap<String, crate::operations::DynOperationHandler>,
        resource_type: Option<&str>,
    ) -> Vec<Value> {
        use std::collections::BTreeMap;
        let levels: &[OperationLevel] = if resource_type.is_some() {
            &[OperationLevel::Type, OperationLevel::Instance]
        } else {
            &[OperationLevel::System]
        };
        let mut found = BTreeMap::new();
        for &level in levels {
            let definitions = match level {
                OperationLevel::System => registry.system_operations().to_vec(),
                OperationLevel::Type => registry.type_operations(resource_type.unwrap()),
                OperationLevel::Instance => registry.instance_operations(resource_type.unwrap()),
            };
            for definition in definitions {
                let routed = match level {
                    OperationLevel::System => registry.get_system_operation(&definition.code),
                    OperationLevel::Type => {
                        registry.get_type_operation(resource_type.unwrap(), &definition.code)
                    }
                    OperationLevel::Instance => {
                        registry.get_instance_operation(resource_type.unwrap(), &definition.code)
                    }
                };
                if routed.is_some_and(|routed| routed.url == definition.url)
                    && !definition.url.is_empty()
                    && handlers
                        .get(&definition.code)
                        .is_some_and(|handler| handler.supports_level(level, resource_type))
                {
                    found
                        .entry(definition.code.clone())
                        .or_insert_with(|| definition.url.clone());
                }
            }
        }
        found
            .into_iter()
            .map(|(name, definition)| json!({"name":name,"definition":definition}))
            .collect()
    }
    fn install(target: &mut Value, mut operations: Vec<Value>, graphql_enabled: bool) {
        if graphql_enabled {
            operations.push(json!({"name":"graphql","definition":"http://hl7.org/fhir/OperationDefinition/Resource-graphql"}));
        }
        if operations.is_empty() {
            target.as_object_mut().unwrap().remove("operation");
        } else {
            operations.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            target["operation"] = json!(operations);
        }
    }
    for rest in body["rest"].as_array_mut().into_iter().flatten() {
        install(rest, declared(registry, handlers, None), graphql_enabled);
        for resource in rest["resource"].as_array_mut().into_iter().flatten() {
            let resource_type = resource["type"].as_str().unwrap_or_default();
            let operations = declared(registry, handlers, Some(resource_type));
            install(resource, operations, graphql_enabled);
        }
    }
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
pub(crate) async fn preprocess_payload(
    resource_type: &str,
    payload: &mut Value,
) -> Result<(), ApiError> {
    // Registry constraints remain mandatory even when general validation is
    // skipped: a failed registry installation must not leave a committed row.
    let issues = crate::validation::resource_constraints(payload);
    if !issues.is_empty() {
        return Err(ApiError::unprocessable_entity(
            "Resource constraints failed",
            Some(
                crate::validation::ValidationOutcome {
                    valid: false,
                    issues,
                }
                .to_operation_outcome(),
            ),
        ));
    }
    if resource_type == "AccessPolicy" {
        let policy: octofhir_auth::policy::resources::AccessPolicy =
            serde_json::from_value(payload.clone())
                .map_err(|error| ApiError::bad_request(format!("Invalid AccessPolicy: {error}")))?;
        policy
            .validate()
            .map_err(|error| ApiError::bad_request(format!("Invalid AccessPolicy: {error}")))?;
        policy
            .to_internal_policy()
            .map_err(|error| ApiError::bad_request(format!("Invalid AccessPolicy: {error}")))?;
    }
    if resource_type == "User" {
        if let Some(obj) = payload.as_object_mut()
            && let Some(password) = obj.get("password").and_then(|v| v.as_str())
        {
            let hashed = octofhir_auth::hash_password(password)
                .map_err(|e| ApiError::internal(format!("Failed to hash password: {}", e)))?;
            obj.insert("passwordHash".to_string(), Value::String(hashed));
            obj.remove("password");
        }
    } else if resource_type == "Client" {
        if let Some(obj) = payload.as_object_mut()
            && let Some(secret) = obj.get("clientSecret").and_then(|v| v.as_str())
        {
            // Only hash if it doesn't look like an Argon2 hash already
            if !secret.starts_with("$argon2id$") {
                let hashed = octofhir_auth::hash_password(secret).map_err(|e| {
                    ApiError::internal(format!("Failed to hash client secret: {}", e))
                })?;
                obj.insert("clientSecret".to_string(), Value::String(hashed));
            }
        }
    } else if resource_type == "App"
        && let Some(obj) = payload.as_object_mut()
        && let Some(secret) = obj.get("secret").and_then(|v| v.as_str())
    {
        // Only hash if it doesn't look like an Argon2 hash already
        if !secret.starts_with("$argon2id$") {
            let hashed = octofhir_auth::hash_app_secret(secret)
                .map_err(|e| ApiError::internal(format!("Failed to hash app secret: {}", e)))?;
            obj.insert("secret".to_string(), Value::String(hashed));
        }
    }
    Ok(())
}

/// Validate a FHIRPath expression for syntax correctness.
fn validate_fhirpath_expression(expression: &str) -> Result<(), ApiError> {
    use octofhir_fhirpath::parse;

    let result = parse(expression);
    if !result.success {
        let error_msg = result
            .error_message
            .unwrap_or_else(|| "Unknown parse error".to_string());
        return Err(ApiError::bad_request(format!(
            "Invalid FHIRPath expression: {}",
            error_msg
        )));
    }

    Ok(())
}

/// Postprocess resource after successful storage (e.g., trigger registry updates).
///
/// This function is called after a resource has been successfully created or updated.
/// It performs resource-type-specific actions such as:
/// - SearchParameter: Validate FHIRPath and reload search parameter registry
/// - Future: Other resource types may trigger specific actions
async fn postprocess_resource(
    resource_type: &str,
    resource_id: &str,
    payload: &Value,
    state: &crate::server::AppState,
) -> Result<(), ApiError> {
    // Invalidate resource cache on create/update
    if let Some(cache) = &state.resource_cache {
        cache.invalidate(resource_type, resource_id).await;
    }

    // Handle SearchParameter resource type
    if resource_type == "SearchParameter" {
        // 1. Validate FHIRPath expression if present
        if let Some(expression) = payload.get("expression").and_then(|v| v.as_str()) {
            validate_fhirpath_expression(expression)?;
        }

        // 2. Parse and incrementally update registry (NO FULL RELOAD!)
        match octofhir_search::parse_search_parameter(payload) {
            Ok(param) => {
                // Get current search config and update registry
                let config = state.search_config.config();
                let version = payload["meta"]["versionId"]
                    .as_str()
                    .and_then(|version| version.parse().ok())
                    .ok_or_else(|| {
                        ApiError::internal("Committed SearchParameter has no revision")
                    })?;
                config
                    .registry
                    .apply_resource(resource_id, version, Some(param));

                // Clear query cache for this specific parameter
                if let Some(cache) = &config.cache {
                    cache.clear();
                }

                tracing::info!(
                    code = payload.get("code").and_then(|v| v.as_str()),
                    "Search parameter registered incrementally"
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to parse SearchParameter for incremental update");
                // Don't fail the request - resource is already saved
            }
        }
    }

    Ok(())
}

// ---- CRUD & Search placeholders ----

#[derive(Default)]
pub(crate) struct LocalReferenceSet {
    pub(crate) groups: Vec<(String, Vec<String>)>,
    pub(crate) versions: Vec<octofhir_core::FhirReference>,
    pub(crate) all: HashSet<String>,
    pub(crate) aliases: HashSet<String>,
    pub(crate) snapshots: Vec<(octofhir_core::FhirReference, Option<std::sync::Arc<Value>>)>,
}

fn collect_reference_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "reference"
                    && let Some(reference) = value.as_str()
                {
                    out.push(reference.to_string());
                } else {
                    collect_reference_strings(value, out);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_reference_strings(value, out);
            }
        }
        _ => {}
    }
}

pub(crate) fn local_reference_set(
    payload: &Value,
    state: &crate::server::AppState,
) -> Result<LocalReferenceSet, ApiError> {
    if state.config.validation.skip_reference_validation {
        return Ok(LocalReferenceSet::default());
    }

    let mut references = Vec::new();
    collect_reference_strings(payload, &mut references);

    let fhir_base = format!("{}/fhir", state.base_url.trim_end_matches('/'));
    let mut by_type: HashMap<String, Vec<String>> = HashMap::new();
    let mut versions = Vec::new();
    let mut aliases = HashSet::new();
    for reference in references {
        let parsed =
            match octofhir_core::fhir_reference::parse_reference(&reference, Some(&fhir_base)) {
                Ok(parsed) => parsed,
                Err(octofhir_core::UnresolvableReference::Invalid(_)) => {
                    return Err(ApiError::unprocessable_entity(
                        "Malformed local FHIR reference",
                        None,
                    ));
                }
                Err(_) => continue,
            };
        if !state
            .resource_type_set
            .load()
            .contains(&parsed.resource_type)
        {
            return Err(ApiError::unprocessable_entity(
                "Unsupported local reference resource type",
                None,
            ));
        }
        aliases.insert(reference);
        if parsed.version.is_some() {
            versions.push(parsed);
        } else {
            by_type
                .entry(parsed.resource_type)
                .or_default()
                .push(parsed.id);
        }
    }

    let mut groups: Vec<(String, Vec<String>)> = by_type
        .into_iter()
        .map(|(resource_type, mut ids)| {
            ids.sort();
            ids.dedup();
            (resource_type, ids)
        })
        .collect();
    groups.sort_by(|left, right| left.0.cmp(&right.0));

    versions.sort_by_key(octofhir_core::FhirReference::to_versioned);
    versions.dedup();
    let all = groups
        .iter()
        .flat_map(|(resource_type, ids)| ids.iter().map(move |id| format!("{resource_type}/{id}")))
        .chain(
            versions
                .iter()
                .map(octofhir_core::FhirReference::to_versioned),
        )
        .collect();

    Ok(LocalReferenceSet {
        groups,
        versions,
        all,
        aliases,
        snapshots: Vec::new(),
    })
}

fn validation_error(validation_outcome: crate::validation::ValidationOutcome) -> ApiError {
    ApiError::UnprocessableEntity {
        message: "Resource validation failed".to_string(),
        operation_outcome: Some(validation_outcome.to_operation_outcome()),
    }
}

async fn validate_for_checked_write(
    payload: &Value,
    state: &crate::server::AppState,
) -> Result<LocalReferenceSet, ApiError> {
    let resource_type = payload["resourceType"].as_str().unwrap_or_default();
    let profiles = crate::validation::ValidationService::schemas_for(payload, resource_type);
    let (validation_outcome, references) =
        validate_write_content(payload, state, &profiles).await?;
    if !validation_outcome.valid {
        return Err(validation_error(validation_outcome));
    }

    Ok(references)
}

/// Shared primary-write/$validate content checks. References are provisionally
/// present for CPU validation; exact existence and body snapshots are checked
/// on the same connection as the eventual commit or speculative rollback.
pub(crate) async fn validate_write_content(
    payload: &Value,
    state: &crate::server::AppState,
    profiles: &[String],
) -> Result<(crate::validation::ValidationOutcome, LocalReferenceSet), ApiError> {
    let resolver = Arc::new(
        crate::operations::validation_reference::ValidationReferenceResolver::for_write(
            state, payload,
        ),
    );
    validate_write_content_with_resolver(payload, state, profiles, resolver).await
}

async fn validate_write_content_with_resolver(
    payload: &Value,
    state: &crate::server::AppState,
    profiles: &[String],
    resolver: Arc<crate::operations::validation_reference::ValidationReferenceResolver>,
) -> Result<(crate::validation::ValidationOutcome, LocalReferenceSet), ApiError> {
    let mut references = local_reference_set(payload, state)?;
    if state.config.validation.skip_reference_validation {
        let outcome = state
            .validation_service
            .validate_against_profiles(payload, profiles)
            .await;
        return Ok((outcome, references));
    }
    let outcome = state
        .validation_service
        .validate_profiles_with_resolver(
            payload,
            profiles,
            Some(&references.aliases),
            resolver.clone(),
        )
        .await;
    if resolver.take_error().is_some() {
        return Err(ApiError::internal("Reference integrity lookup failed"));
    }
    for (reference, body) in resolver.write_snapshots(payload).await {
        if references.all.insert(reference.to_versioned()) {
            if reference.version.is_some() {
                references.versions.push(reference.clone());
            } else if let Some((_, ids)) = references
                .groups
                .iter_mut()
                .find(|(rt, _)| rt == &reference.resource_type)
            {
                ids.push(reference.id.clone());
            } else {
                references
                    .groups
                    .push((reference.resource_type.clone(), vec![reference.id.clone()]));
            }
        }
        references.snapshots.push((reference, body));
    }
    references
        .snapshots
        .sort_by_key(|(reference, _)| reference.to_versioned());
    Ok((outcome, references))
}

async fn validate_missing_references(
    payload: &Value,
    state: &crate::server::AppState,
    found: &HashSet<String>,
) -> ApiError {
    let references = match local_reference_set(payload, state) {
        Ok(references) => references,
        Err(error) => return error,
    };
    let fhir_base = format!("{}/fhir", state.base_url.trim_end_matches('/'));
    let available_aliases = references
        .aliases
        .into_iter()
        .filter(|alias| {
            octofhir_core::parse_reference(alias, Some(&fhir_base))
                .ok()
                .is_some_and(|reference| found.contains(&reference.to_versioned()))
        })
        .collect();
    let validation_outcome = state
        .validation_service
        .validate_with_known_refs(payload, &available_aliases)
        .await;
    if validation_outcome.valid {
        // Transactional existence is authoritative even if a validator cannot
        // independently resolve this exact version or a target appeared later.
        let message = "A required local reference is unavailable for this write";
        return ApiError::unprocessable_entity(
            message,
            Some(json!({
                "resourceType":"OperationOutcome", "issue":[{
                    "severity":"error", "code":"not-found", "diagnostics":message
                }]
            })),
        );
    }
    validation_error(validation_outcome)
}

/// Pool-side existence check for the lock-free write path. Targets may vanish
/// between this check and the write; that race is accepted in exchange for not
/// holding row locks on every reference target.
async fn check_references_exist(
    state: &crate::server::AppState,
    payload: &Value,
    references: &LocalReferenceSet,
) -> Result<(), ApiError> {
    if references.groups.is_empty() {
        return Ok(());
    }
    let found = state
        .storage
        .exists_many_grouped(&references.groups)
        .await
        .map_err(map_storage_error)?;
    if found == references.all {
        return Ok(());
    }
    Err(validate_missing_references(payload, state, &found).await)
}

/// `postprocess_resource` only inspects the body for SearchParameter, where it
/// needs the committed meta; every other type can use the request payload and
/// skip re-parsing the stored JSON.
async fn postprocess_stored(
    resource_type: &str,
    stored: &octofhir_storage::RawStoredResource,
    payload: &Value,
    state: &crate::server::AppState,
) -> Result<(), ApiError> {
    let committed = if resource_type == "SearchParameter" {
        Some(
            serde_json::from_str::<Value>(&stored.resource_json)
                .map_err(|error| ApiError::internal(error.to_string()))?,
        )
    } else {
        None
    };
    postprocess_resource(
        resource_type,
        &stored.id,
        committed.as_ref().unwrap_or(payload),
        state,
    )
    .await
}

async fn map_write_error(
    error: crate::write::WriteError,
    payload: &Value,
    state: &crate::server::AppState,
) -> ApiError {
    match error {
        crate::write::WriteError::Api(error) => error,
        crate::write::WriteError::Storage(error) => map_storage_error(error),
        crate::write::WriteError::MissingReferences(found) => {
            validate_missing_references(payload, state, &found).await
        }
    }
}

fn map_transaction_write_error(error: crate::write::WriteError) -> ApiError {
    match error {
        crate::write::WriteError::Api(error) => error,
        crate::write::WriteError::Storage(error) => map_storage_error(error),
        crate::write::WriteError::MissingReferences(_) => ApiError::unprocessable_entity(
            "A required local reference is unavailable for this write",
            None,
        ),
    }
}

#[tracing::instrument(name = "fhir.create", skip_all, fields(resource_type = %resource_type))]
pub async fn create_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    authorization: Option<Extension<crate::write::AuthorizedWrite>>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Result<Response, ApiError> {
    // Create ignores client identity before content validation as well as storage.
    if let Some(object) = payload.as_object_mut() {
        object.remove("id");
    }
    // Basic structural validation (resourceType match)
    let resource_types = state.resource_type_set.load();
    if let Err(e) =
        crate::validation::validate_resource(&resource_type, &payload, resource_types.as_ref())
    {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    let mut conditions = headers.get_all("If-None-Exist").iter();
    let condition = conditions
        .next()
        .map(|value| {
            value
                .to_str()
                .map_err(|_| ApiError::bad_request("Invalid If-None-Exist header"))
        })
        .transpose()?;
    if conditions.next().is_some() {
        return Err(ApiError::bad_request(
            "If-None-Exist requires a single search criterion",
        ));
    }
    // Without a policy context or If-None-Exist nothing needs row locks or a
    // multi-statement transaction: one pool statement checks references, one
    // inserts. The locking protocol below pins a connection for 5+ round trips
    // per write, which under load turns the pool into the bottleneck.
    let fast_path = authorization.is_none() && condition.is_none();
    let authorization = authorization.map(|Extension(authorization)| authorization.0);
    let write = if fast_path {
        None
    } else {
        Some(
            crate::write::PreparedWrite::prepare(
                &state,
                crate::write::WriteMode::Create,
                &resource_type,
                None,
                authorization,
            )
            .await?,
        )
    };
    let conditional = match (&write, condition) {
        (Some(write), Some(condition)) => Some(write.conditional(&state, condition).await?),
        _ => None,
    };
    // A match ignores the POST content. Preview under the same coordination
    // used for the final write, without holding a connection during validation.
    let matched = match &conditional {
        Some(conditional) => match conditional.execute(&state, &payload, None).await {
            Ok(result) => result,
            Err(error) => return Err(map_write_error(error, &payload, &state).await),
        },
        None => None,
    };
    let skip_validation = should_skip_validation(&headers, &state.config.validation);
    let (stored, created) = if let Some(matched) = matched {
        let created = matched.created;
        (matched.into_raw(), created)
    } else {
        let local_references = if skip_validation {
            tracing::warn!(resource_type = %resource_type, operation = "create",
                "Validation skipped via X-Skip-Validation header");
            LocalReferenceSet::default()
        } else {
            validate_for_checked_write(&payload, &state).await?
        };
        preprocess_payload(&resource_type, &mut payload).await?;
        validate_payload_structure(&resource_type, &payload, &IdPolicy::Create)
            .map_err(ApiError::bad_request)?;
        if let Some(conditional) = &conditional {
            match conditional
                .execute(&state, &payload, Some(&local_references))
                .await
            {
                Ok(result) => {
                    let result = result.expect("validated conditional create returns a resource");
                    let created = result.created;
                    (result.into_raw(), created)
                }
                Err(error) => return Err(map_write_error(error, &payload, &state).await),
            }
        } else if fast_path && local_references.versions.is_empty() {
            check_references_exist(&state, &payload, &local_references).await?;
            match state.storage.create_raw(&payload).await {
                Ok(stored) => (stored, true),
                Err(error) => return Err(map_storage_error(error)),
            }
        } else {
            let write = match write {
                Some(write) => write,
                None => {
                    crate::write::PreparedWrite::prepare(
                        &state,
                        crate::write::WriteMode::Create,
                        &resource_type,
                        None,
                        None,
                    )
                    .await?
                }
            };
            match write
                .run(&state, Some(&payload), &local_references, None, true)
                .await
            {
                Ok(result) => {
                    let created = result.created;
                    (result.into_raw(), created)
                }
                Err(error) => return Err(map_write_error(error, &payload, &state).await),
            }
        }
    };
    if created {
        postprocess_stored(&resource_type, &stored, &payload, &state).await?;
    }
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let mut response_headers = HeaderMap::new();
    insert_header_if_valid(
        &mut response_headers,
        if created {
            header::LOCATION
        } else {
            header::CONTENT_LOCATION
        },
        fhir_versioned_resource_url(
            &state.fhir_base_url(),
            &resource_type,
            &stored.id,
            &stored.version_id,
        ),
    );
    insert_header_if_valid(
        &mut response_headers,
        header::ETAG,
        format!("W/\"{}\"", stored.version_id),
    );
    insert_header_if_valid(
        &mut response_headers,
        header::LAST_MODIFIED,
        httpdate::fmt_http_date(
            std::time::UNIX_EPOCH
                + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64),
        ),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
    );
    if created && skip_validation {
        response_headers.insert(
            "X-Validation-Skipped",
            header::HeaderValue::from_static("true"),
        );
    }
    let prefer_return = headers
        .get("Prefer")
        .and_then(|value| value.to_str().ok())
        .map(parse_prefer_return);
    match prefer_return {
        Some(PreferReturn::Minimal) => Ok((status, response_headers, "").into_response()),
        Some(PreferReturn::OperationOutcome) => Ok((
            status,
            response_headers,
            Json(json!({
                "resourceType": "OperationOutcome",
                "issue": [{"severity": "information", "code": "informational",
                    "diagnostics": format!("Create completed: {}/{}", resource_type, stored.id)}]
            })),
        )
            .into_response()),
        _ => Ok((status, response_headers, stored.resource_json).into_response()),
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
    let normalized = header.to_ascii_lowercase();
    if normalized.contains("return=minimal") {
        PreferReturn::Minimal
    } else if normalized.contains("return=operationoutcome") {
        PreferReturn::OperationOutcome
    } else {
        PreferReturn::Representation
    }
}

fn fhir_versioned_resource_url(
    base_url: &str,
    resource_type: &str,
    id: &str,
    version_id: &str,
) -> String {
    format!(
        "{}/{}/{}/_history/{}",
        base_url.trim_end_matches('/'),
        resource_type,
        id,
        version_id
    )
}

/// Extract the resource id from a FHIR Location header of the form
/// `{base}/{ResourceType}/{id}/_history/{version}`.
fn extract_id_from_location(location: &str) -> Option<String> {
    let before_history = location.split("/_history/").next()?;
    let id = before_history.trim_end_matches('/').rsplit('/').next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn insert_header_if_valid(headers: &mut HeaderMap, name: header::HeaderName, value: String) {
    if let Ok(val) = header::HeaderValue::from_str(&value) {
        headers.insert(name, val);
    }
}

fn rebase_internal_write_headers(state: &crate::server::AppState, response: &mut Response) {
    let fhir_prefix = format!("{}/", state.fhir_base_url());
    for name in [header::LOCATION, header::CONTENT_LOCATION] {
        if let Some(location) = response
            .headers()
            .get(&name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix(&fhir_prefix))
            .map(|relative| format!("{}/{relative}", state.base_url.trim_end_matches('/')))
        {
            insert_header_if_valid(response.headers_mut(), name, location);
        }
    }
}

fn add_subsetted_tag(resource: &mut Value) {
    let Some(obj) = resource.as_object_mut() else {
        return;
    };

    let meta = obj.entry("meta".to_string()).or_insert_with(|| json!({}));
    let Some(meta_obj) = meta.as_object_mut() else {
        return;
    };

    let tags = meta_obj
        .entry("tag".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(tags_arr) = tags.as_array_mut() else {
        return;
    };

    let has_subsetted = tags_arr.iter().any(|tag| {
        tag["system"].as_str() == Some("http://terminology.hl7.org/CodeSystem/v3-ObservationValue")
            && tag["code"].as_str() == Some("SUBSETTED")
    });

    if !has_subsetted {
        tags_arr.push(json!({
            "system": "http://terminology.hl7.org/CodeSystem/v3-ObservationValue",
            "code": "SUBSETTED",
        }));
    }
}

fn should_tag_subsetted(params: &HashMap<String, String>) -> bool {
    if params.contains_key("_elements") {
        return true;
    }

    match params.get("_summary").map(|s| s.as_str()) {
        Some("true" | "text" | "data") => true,
        Some("false" | "count") | None => false,
        Some(_) => true,
    }
}

fn apply_result_params_to_resource(
    resource_json: &str,
    params: &HashMap<String, String>,
) -> Result<Option<Vec<u8>>, ApiError> {
    if !params.contains_key("_summary") && !params.contains_key("_elements") {
        return Ok(None);
    }

    let mut resource: Value =
        serde_json::from_str(resource_json).map_err(|e| ApiError::internal(e.to_string()))?;

    if let Some(summary) = params.get("_summary") {
        resource = apply_summary(&resource, summary);
    }
    if let Some(elements) = params.get("_elements") {
        resource = apply_elements_filter(&resource, elements);
    }
    if should_tag_subsetted(params) {
        add_subsetted_tag(&mut resource);
    }

    serde_json::to_vec(&resource)
        .map(Some)
        .map_err(|e| ApiError::internal(e.to_string()))
}

// ---- History Query Parameters ----

/// Parse a FHIR `_total` query value into a [`TotalMode`]. Unknown/absent → None.
fn parse_total_mode(s: Option<&str>) -> Option<octofhir_storage::TotalMode> {
    match s.map(str::to_ascii_lowercase).as_deref() {
        Some("accurate") => Some(octofhir_storage::TotalMode::Accurate),
        Some("estimate") => Some(octofhir_storage::TotalMode::Estimate),
        _ => None,
    }
}

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
    #[serde(rename = "_offset", alias = "__offset")]
    pub offset: Option<u32>,
    /// Total-count mode: `accurate` computes `Bundle.total`. Omitted by default
    /// so history does not scan the full current + history set on every request.
    #[serde(rename = "_total")]
    pub total: Option<String>,
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

async fn authorize_read_error(
    state: &crate::server::AppState,
    authorization: Option<&crate::authorization::AuthorizedRead>,
    resource_type: &str,
    id: &str,
    error: StorageError,
) -> ApiError {
    if error.is_deleted()
        && let Some(authorization) = authorization
    {
        let result = async {
            let snapshot = state
                .storage
                .current_snapshot(resource_type, id)
                .await
                .map_err(map_storage_error)?
                .ok_or_else(|| ApiError::not_found("Resource is unavailable"))?;
            let body = serde_json::from_str(&snapshot.resource.resource_json)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            authorization
                .clone()
                .prepare(state)
                .await?
                .authorize(state, &body)
                .await
        }
        .await;
        if let Err(error) = result {
            return error;
        }
    }
    map_storage_error(error)
}

pub(crate) fn stored_snapshot(
    raw: &octofhir_storage::RawStoredResource,
) -> Result<octofhir_storage::StoredResource, ApiError> {
    Ok(octofhir_storage::StoredResource {
        id: raw.id.clone(),
        version_id: raw.version_id.clone(),
        resource_type: raw.resource_type.clone(),
        resource: serde_json::from_str(&raw.resource_json)
            .map_err(|error| ApiError::internal(error.to_string()))?,
        last_updated: raw.last_updated,
        created_at: raw.created_at,
    })
}

#[tracing::instrument(name = "fhir.read", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn read_resource(
    State(state): State<crate::server::AppState>,
    authorization: Option<Extension<crate::authorization::AuthorizedRead>>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    // Try resource cache first
    let stored: Option<std::sync::Arc<octofhir_storage::RawStoredResource>> =
        if let Some(cache) = &state.resource_cache {
            let fill_stamp = cache.begin_fill(&resource_type, &id);
            if let Some(cached) = cache.get(&resource_type, &id).await {
                Some(cached)
            } else {
                match state.storage.read_raw(&resource_type, &id).await {
                    Ok(Some(s)) => {
                        cache.set(&s, fill_stamp).await;
                        Some(std::sync::Arc::new(s))
                    }
                    Ok(None) => None,
                    Err(e) => {
                        return Err(authorize_read_error(
                            &state,
                            authorization.as_ref().map(|Extension(auth)| auth),
                            &resource_type,
                            &id,
                            e,
                        )
                        .await);
                    }
                }
            }
        } else {
            match state.storage.read_raw(&resource_type, &id).await {
                Ok(Some(s)) => Some(std::sync::Arc::new(s)),
                Ok(None) => None,
                Err(e) => {
                    return Err(authorize_read_error(
                        &state,
                        authorization.as_ref().map(|Extension(auth)| auth),
                        &resource_type,
                        &id,
                        e,
                    )
                    .await);
                }
            }
        };

    match stored {
        Some(stored) => {
            if let Some(Extension(authorization)) = authorization {
                let body: Value = serde_json::from_str(&stored.resource_json)
                    .map_err(|error| ApiError::internal(error.to_string()))?;
                authorization
                    .prepare(&state)
                    .await?
                    .authorize(&state, &body)
                    .await?;
            }
            let version_id = &stored.version_id;

            if snapshot_not_modified(&headers, version_id, stored.last_updated) {
                return Ok(not_modified_response(version_id, stored.last_updated));
            }

            // Build response with raw JSON body (no serde_json::Value round-trip)
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/fhir+json; charset=utf-8");

            // ETag: W/"version_id"
            let etag = format!("W/\"{}\"", version_id);
            builder = builder.header(header::ETAG, etag);

            // Last-Modified: HTTP date format
            let last_modified = httpdate::fmt_http_date(
                std::time::UNIX_EPOCH
                    + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64),
            );
            builder = builder.header(header::LAST_MODIFIED, last_modified);

            let body = match apply_result_params_to_resource(&stored.resource_json, &params)? {
                Some(filtered) => Body::from(filtered),
                None => Body::from(stored.resource_json.clone()),
            };

            Ok(builder.body(body).unwrap())
        }
        None => Err(ApiError::not_found(format!(
            "{resource_type} with id '{id}' not found"
        ))),
    }
}

/// GET /[type]/[id]/_history/[vid] - Read a specific version of a resource
#[tracing::instrument(name = "fhir.vread", skip_all, fields(resource_type = %resource_type, id = %id, version_id = %version_id))]
pub async fn vread_resource(
    State(state): State<crate::server::AppState>,
    authorization: Option<Extension<crate::authorization::AuthorizedRead>>,
    Path((resource_type, id, version_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    // Use vread_raw to get the specific version as raw JSON (no Value round-trip)
    match state
        .storage
        .version_snapshot(&resource_type, &id, &version_id)
        .await
    {
        Ok(Some(snapshot)) => {
            let stored = snapshot.resource;
            if let Some(Extension(authorization)) = authorization {
                let body: Value = serde_json::from_str(&stored.resource_json)
                    .map_err(|error| ApiError::internal(error.to_string()))?;
                authorization
                    .prepare(&state)
                    .await?
                    .authorize(&state, &body)
                    .await?;
            }
            if snapshot.method == octofhir_storage::HistoryMethod::Delete {
                return Err(ApiError::gone("Requested revision records a deletion"));
            }
            if snapshot_not_modified(&headers, &stored.version_id, stored.last_updated) {
                return Ok(not_modified_response(
                    &stored.version_id,
                    stored.last_updated,
                ));
            }
            // Build response with raw JSON body
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/fhir+json; charset=utf-8");

            // ETag: W/"version_id"
            let etag = format!("W/\"{}\"", stored.version_id);
            builder = builder.header(header::ETAG, etag);

            // Last-Modified: HTTP date format
            let last_modified = httpdate::fmt_http_date(
                std::time::UNIX_EPOCH
                    + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64),
            );
            builder = builder.header(header::LAST_MODIFIED, last_modified);

            let body = match apply_result_params_to_resource(&stored.resource_json, &params)? {
                Some(filtered) => Body::from(filtered),
                None => Body::from(stored.resource_json),
            };

            Ok(builder.body(body).unwrap())
        }
        Ok(None) => Err(ApiError::not_found(format!(
            "{resource_type}/{id}/_history/{version_id} not found"
        ))),
        Err(e) => Err(map_storage_error(e)),
    }
}

fn finalize_history_links(
    bundle: &mut octofhir_api::Bundle,
    params: &HistoryQueryParams,
    offset: u32,
    count: u32,
    has_more: bool,
) -> Result<(), ApiError> {
    if bundle.total.is_none() && count > 0 {
        let base = bundle
            .link
            .iter()
            .find(|link| link.relation == "self")
            .map(|link| link.url.split('?').next().unwrap_or(&link.url).to_owned())
            .ok_or_else(|| ApiError::internal("History lacks its self link"))?;
        bundle.link.push(octofhir_api::BundleLink {
            relation: "first".into(),
            url: format!("{base}?_count={count}&_offset=0"),
        });
        if offset > 0 {
            bundle.link.push(octofhir_api::BundleLink {
                relation: "previous".into(),
                url: format!(
                    "{base}?_count={count}&_offset={}",
                    offset.saturating_sub(count)
                ),
            });
        }
        if has_more {
            bundle.link.push(octofhir_api::BundleLink {
                relation: "next".into(),
                url: format!(
                    "{base}?_count={count}&_offset={}",
                    offset
                        .checked_add(count)
                        .ok_or_else(|| ApiError::bad_request("History offset overflow"))?
                ),
            });
        }
    }
    for link in &mut bundle.link {
        let mut url =
            url::Url::parse(&link.url).map_err(|error| ApiError::internal(error.to_string()))?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(since) = &params.since {
                query.append_pair("_since", since);
            }
            if let Some(at) = &params.at {
                query.append_pair("_at", at);
            }
            if let Some(total) = &params.total {
                query.append_pair("_total", total);
            }
        }
        link.url = url.to_string();
    }
    Ok(())
}

// ---- History Handlers ----

/// Instance history: GET /{type}/{id}/_history
#[tracing::instrument(name = "fhir.history.instance", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn instance_history(
    State(state): State<crate::server::AppState>,
    authorization: Option<Extension<crate::authorization::AuthorizedHistory>>,
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
    match state.storage.read_raw(&resource_type, &id).await {
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
        let range = octofhir_search::types::parse_date_range(at)
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        history_params.at = Some(range.start);
        history_params.at_end = Some(range.end);
    }
    let count = params
        .count
        .unwrap_or(100)
        .min(u32::try_from(state.config.search.max_count).unwrap_or(u32::MAX - 1));
    let offset = params.offset.unwrap_or(0);
    history_params.count = Some(if count == 0 {
        0
    } else {
        count.saturating_add(1)
    });
    history_params.offset = Some(offset);
    history_params.total = parse_total_mode(params.total.as_deref());

    // Get history from storage (raw path: skips JSONB → Value round-trip)
    let mut result = if let Some(Extension(authorization)) = authorization {
        authorization
            .read(
                &state,
                std::slice::from_ref(&resource_type),
                Some(&id),
                &history_params,
            )
            .await?
    } else {
        state
            .storage
            .history_raw(&resource_type, Some(&id), &history_params)
            .await
            .map_err(map_storage_error)?
    };

    let has_more = result.entries.len() > count as usize;
    result.entries.truncate(count as usize);

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
                resource: octofhir_api::RawJson::from_string(entry.resource.resource_json),
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

    let mut bundle = bundle_from_history(
        entries,
        &state.fhir_base_url(),
        &resource_type,
        Some(&id),
        offset as usize,
        count as usize,
        result.total,
    );

    finalize_history_links(&mut bundle, &params, offset, count, has_more)?;

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
    authorization: Option<Extension<crate::authorization::AuthorizedHistory>>,
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
        let range = octofhir_search::types::parse_date_range(at)
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        history_params.at = Some(range.start);
        history_params.at_end = Some(range.end);
    }
    let count = params
        .count
        .unwrap_or(100)
        .min(u32::try_from(state.config.search.max_count).unwrap_or(u32::MAX - 1));
    let offset = params.offset.unwrap_or(0);
    history_params.count = Some(if count == 0 {
        0
    } else {
        count.saturating_add(1)
    });
    history_params.offset = Some(offset);
    history_params.total = parse_total_mode(params.total.as_deref());

    // Get history from storage (raw path, None for id = type-level history)
    let mut result = if let Some(Extension(authorization)) = authorization {
        authorization
            .read(
                &state,
                std::slice::from_ref(&resource_type),
                None,
                &history_params,
            )
            .await?
    } else {
        state
            .storage
            .history_raw(&resource_type, None, &history_params)
            .await
            .map_err(map_storage_error)?
    };

    let has_more = result.entries.len() > count as usize;
    result.entries.truncate(count as usize);

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
                resource: octofhir_api::RawJson::from_string(entry.resource.resource_json),
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

    let mut bundle = bundle_from_history(
        entries,
        &state.fhir_base_url(),
        &resource_type,
        None,
        offset as usize,
        count as usize,
        result.total,
    );

    finalize_history_links(&mut bundle, &params, offset, count, has_more)?;

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
    authorization: Option<Extension<crate::authorization::AuthorizedHistory>>,
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
        let range = octofhir_search::types::parse_date_range(at)
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        history_params.at = Some(range.start);
        history_params.at_end = Some(range.end);
    }
    let count = params
        .count
        .unwrap_or(100)
        .min(u32::try_from(state.config.search.max_count).unwrap_or(u32::MAX - 1));
    let offset = params.offset.unwrap_or(0);
    history_params.count = Some(if count == 0 {
        0
    } else {
        count.saturating_add(1)
    });
    history_params.offset = Some(offset);
    history_params.total = parse_total_mode(params.total.as_deref());

    // Get system-level history from storage (raw path)
    let mut result = if let Some(Extension(authorization)) = authorization {
        authorization
            .read(
                &state,
                &system_search_types(&state, None),
                None,
                &history_params,
            )
            .await?
    } else {
        state
            .storage
            .system_history_raw(&history_params)
            .await
            .map_err(map_storage_error)?
    };

    let has_more = result.entries.len() > count as usize;
    result.entries.truncate(count as usize);

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
                resource: octofhir_api::RawJson::from_string(entry.resource.resource_json),
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

    let mut bundle = bundle_from_system_history(
        entries,
        &state.fhir_base_url(),
        offset as usize,
        count as usize,
        result.total,
    );

    finalize_history_links(&mut bundle, &params, offset, count, has_more)?;

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
    authorization: Option<Extension<crate::write::AuthorizedWrite>>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Result<Response, ApiError> {
    // Basic structural validation (resourceType match)
    let resource_types = state.resource_type_set.load();
    if let Err(e) =
        crate::validation::validate_resource(&resource_type, &payload, resource_types.as_ref())
    {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    validate_payload_structure(
        &resource_type,
        &payload,
        &IdPolicy::Update {
            path_id: id.clone(),
        },
    )
    .map_err(ApiError::bad_request)?;
    if let Some(object) = payload.as_object_mut() {
        object.insert("id".into(), Value::String(id.clone()));
    }

    // Full schema + FHIRPath constraint validation using ValidationService
    let skip_validation = should_skip_validation(&headers, &state.config.validation);
    let mut local_references = LocalReferenceSet::default();

    if !skip_validation {
        local_references = validate_for_checked_write(&payload, &state).await?;
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

    let if_match = crate::write::if_match_header(&headers)?;

    // Parse Prefer header for return preference
    let prefer_return = headers
        .get("Prefer")
        .and_then(|h| h.to_str().ok())
        .map(parse_prefer_return);

    let mut payload = payload;
    preprocess_payload(&resource_type, &mut payload).await?;

    // Validate payload structure (zero-allocation check)
    if let Err(err) = validate_payload_structure(
        &resource_type,
        &payload,
        &IdPolicy::Update {
            path_id: id.clone(),
        },
    ) {
        return Err(ApiError::bad_request(err));
    }

    // Inject id from URL path into payload (storage layer requires id in JSON)
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("id".to_string(), Value::String(id.clone()));
    }

    // Same fast path as create: no policy context means a single-statement
    // UPDATE (If-Match folded into its WHERE), falling back to an INSERT when
    // the id does not exist yet (PUT-as-create).
    let (stored, created) = if authorization.is_none() && local_references.versions.is_empty() {
        check_references_exist(&state, &payload, &local_references).await?;
        match state
            .storage
            .update_raw(&payload, if_match.as_deref())
            .await
        {
            Ok(stored) => (stored, false),
            Err(octofhir_storage::StorageError::NotFound { .. }) if if_match.is_none() => {
                match state.storage.create_raw(&payload).await {
                    Ok(stored) => (stored, true),
                    Err(error) => return Err(map_storage_error(error)),
                }
            }
            Err(error) => return Err(map_storage_error(error)),
        }
    } else {
        let write = crate::write::PreparedWrite::prepare(
            &state,
            crate::write::WriteMode::Update,
            &resource_type,
            Some(&id),
            authorization.map(|Extension(authorization)| authorization.0),
        )
        .await?;
        match write
            .run(
                &state,
                Some(&payload),
                &local_references,
                if_match.as_deref(),
                true,
            )
            .await
        {
            Ok(result) => {
                let created = result.created;
                (result.into_raw(), created)
            }
            Err(error) => return Err(map_write_error(error, &payload, &state).await),
        }
    };
    postprocess_stored(&resource_type, &stored, &payload, &state).await?;

    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let mut response_headers = HeaderMap::new();
    let location = fhir_versioned_resource_url(
        &state.fhir_base_url(),
        &resource_type,
        &id,
        &stored.version_id,
    );
    insert_header_if_valid(
        &mut response_headers,
        header::CONTENT_LOCATION,
        location.clone(),
    );
    if created {
        insert_header_if_valid(&mut response_headers, header::LOCATION, location);
    }
    insert_header_if_valid(
        &mut response_headers,
        header::ETAG,
        format!("W/\"{}\"", stored.version_id),
    );
    let last_modified = httpdate::fmt_http_date(
        std::time::UNIX_EPOCH
            + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64),
    );
    insert_header_if_valid(&mut response_headers, header::LAST_MODIFIED, last_modified);
    response_headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
    );
    if skip_validation {
        response_headers.insert(
            "X-Validation-Skipped",
            header::HeaderValue::from_static("true"),
        );
    }
    match prefer_return {
        Some(PreferReturn::Minimal) => Ok((status, response_headers).into_response()),
        Some(PreferReturn::OperationOutcome) => {
            let action = if created { "created" } else { "updated" };
            let outcome = json!({"resourceType":"OperationOutcome", "issue":[{
                "severity":"information", "code":"informational",
                "diagnostics":format!("Resource {action}: {resource_type}/{id}")
            }]});
            Ok((status, response_headers, Json(outcome)).into_response())
        }
        _ => Ok((status, response_headers, stored.resource_json).into_response()),
    }
}

/// Render the actual committed conditional mutation, including empty minimal
/// responses. Registry/cache changes run only after its transaction committed.
async fn conditional_write_response(
    state: &crate::server::AppState,
    resource_type: &str,
    result: crate::write::WriteResult,
    headers: &HeaderMap,
    action: &str,
    skip_validation: bool,
) -> Result<Response, ApiError> {
    let stored = result
        .stored
        .as_ref()
        .expect("write returns a stored resource");
    postprocess_resource(resource_type, &stored.id, &stored.resource, state).await?;
    let created = result.created;
    let stored = result.into_raw();
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let mut response_headers = HeaderMap::new();
    let location = fhir_versioned_resource_url(
        &state.fhir_base_url(),
        resource_type,
        &stored.id,
        &stored.version_id,
    );
    insert_header_if_valid(
        &mut response_headers,
        header::CONTENT_LOCATION,
        location.clone(),
    );
    if created {
        insert_header_if_valid(&mut response_headers, header::LOCATION, location);
    }
    insert_header_if_valid(
        &mut response_headers,
        header::ETAG,
        format!("W/\"{}\"", stored.version_id),
    );
    insert_header_if_valid(
        &mut response_headers,
        header::LAST_MODIFIED,
        httpdate::fmt_http_date(
            std::time::UNIX_EPOCH
                + std::time::Duration::from_secs(stored.last_updated.unix_timestamp() as u64),
        ),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/fhir+json; charset=utf-8"),
    );
    if skip_validation {
        response_headers.insert(
            "X-Validation-Skipped",
            header::HeaderValue::from_static("true"),
        );
    }
    let preference = headers
        .get("Prefer")
        .and_then(|value| value.to_str().ok())
        .map(parse_prefer_return);
    Ok(match preference {
        Some(PreferReturn::Minimal) => (status, response_headers).into_response(),
        Some(PreferReturn::OperationOutcome) => {
            let action = if created { "created" } else { action };
            let outcome = json!({"resourceType":"OperationOutcome","issue":[{
                "severity":"information","code":"informational",
                "diagnostics":format!("Resource {action}: {resource_type}/{}", stored.id)
            }]});
            (status, response_headers, Json(outcome)).into_response()
        }
        _ => (status, response_headers, stored.resource_json).into_response(),
    })
}

/// PUT /[type]?[search params] - Conditional update based on search criteria
#[tracing::instrument(name = "fhir.conditional_update", skip_all, fields(resource_type = %resource_type))]
pub async fn conditional_update_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    authorization: Option<Extension<crate::write::AuthorizedWrite>>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let query = raw
        .filter(|query| !query.is_empty())
        .ok_or_else(|| ApiError::bad_request("Conditional update requires search parameters"))?;
    crate::validation::validate_resource(
        &resource_type,
        &payload,
        state.resource_type_set.load().as_ref(),
    )
    .map_err(ApiError::bad_request)?;
    let supplied_id = match payload.get("id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => {
            octofhir_core::validate_id(id)
                .map_err(|error| ApiError::bad_request(error.to_string()))?;
            Some(id.as_str())
        }
        Some(_) => {
            return Err(ApiError::bad_request(
                "Conditional PUT id must be a FHIR id",
            ));
        }
    };
    let if_match = crate::write::if_match_header(&headers)?;
    let write = crate::write::PreparedWrite::prepare(
        &state,
        crate::write::WriteMode::Update,
        &resource_type,
        None,
        authorization.map(|Extension(authorization)| authorization.0),
    )
    .await?;
    let conditional = write.conditional(&state, &query).await?;
    let skip_validation = should_skip_validation(&headers, &state.config.validation);
    // Revalidation after a changed selection preserves self-reference identity
    // and targetProfile checks without holding the pool connection during CPU work.
    for _ in 0..4 {
        let previous = match conditional.snapshot(&state, Some(&payload)).await {
            Ok(previous) => previous,
            Err(error) => return Err(map_write_error(error, &payload, &state).await),
        };
        let preview_id = previous.as_ref().map(|row| row.id.as_str());
        if let (Some(supplied), Some(selected)) = (supplied_id, preview_id)
            && supplied != selected
        {
            return Err(ApiError::bad_request(
                "Conditional PUT body id does not match the selected resource",
            ));
        }
        let id = preview_id
            .or(supplied_id)
            .map(str::to_owned)
            .unwrap_or_else(octofhir_core::generate_id);
        let mut candidate = payload.clone();
        candidate["id"] = Value::String(id.clone());
        validate_payload_structure(
            &resource_type,
            &candidate,
            &IdPolicy::Update { path_id: id },
        )
        .map_err(ApiError::bad_request)?;
        let references = if skip_validation {
            LocalReferenceSet::default()
        } else {
            validate_for_checked_write(&candidate, &state).await?
        };
        preprocess_payload(&resource_type, &mut candidate).await?;
        let result = match conditional
            .update(
                &state,
                &candidate,
                &references,
                supplied_id,
                preview_id,
                if_match.as_deref(),
            )
            .await
        {
            Ok(Some(result)) => result,
            Ok(None) => continue,
            Err(error) => return Err(map_write_error(error, &candidate, &state).await),
        };
        return conditional_write_response(
            &state,
            &resource_type,
            result,
            &headers,
            "updated",
            skip_validation,
        )
        .await;
    }
    Err(ApiError::conflict(
        "Conditional selection kept changing; retry the write",
    ))
}

fn deletion_headers(
    deletion: Option<&octofhir_storage::DeletedResource>,
) -> Result<HeaderMap, ApiError> {
    let mut headers = HeaderMap::new();
    if let Some(deletion) = deletion {
        headers.insert(
            header::ETAG,
            header::HeaderValue::from_str(&format!("W/\"{}\"", deletion.version_id))
                .map_err(|error| ApiError::internal(error.to_string()))?,
        );
        let timestamp = std::time::UNIX_EPOCH
            + std::time::Duration::from_secs(
                deletion
                    .last_updated
                    .unix_timestamp()
                    .try_into()
                    .map_err(|_| ApiError::internal("Invalid deletion timestamp"))?,
            );
        headers.insert(
            header::LAST_MODIFIED,
            header::HeaderValue::from_str(&httpdate::fmt_http_date(timestamp))
                .map_err(|error| ApiError::internal(error.to_string()))?,
        );
    }
    Ok(headers)
}

#[tracing::instrument(name = "fhir.delete", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn delete_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    authorization: Option<Extension<crate::write::AuthorizedWrite>>,
    headers: HeaderMap,
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

    let if_match = crate::write::if_match_header(&headers)?;
    // Fast path: one pool DELETE; the trigger archives the body and tombstone.
    // SearchParameter needs the deletion revision for its registry, so it keeps
    // the snapshot-returning path.
    if authorization.is_none() && if_match.is_none() && resource_type != "SearchParameter" {
        state
            .storage
            .delete(&resource_type, &id)
            .await
            .map_err(map_storage_error)?;
        if let Some(cache) = &state.resource_cache {
            cache.invalidate(&resource_type, &id).await;
        }
        return Ok((StatusCode::NO_CONTENT, HeaderMap::new()));
    }
    let write = crate::write::PreparedWrite::prepare(
        &state,
        crate::write::WriteMode::Delete,
        &resource_type,
        Some(&id),
        authorization.map(|Extension(authorization)| authorization.0),
    )
    .await?;
    let result = match write
        .run(
            &state,
            None,
            &LocalReferenceSet::default(),
            if_match.as_deref(),
            true,
        )
        .await
    {
        Ok(result) => result,
        Err(error) => return Err(map_write_error(error, &Value::Null, &state).await),
    };
    if let Some(cache) = &state.resource_cache {
        cache.invalidate(&resource_type, &id).await;
    }
    if resource_type == "SearchParameter"
        && let Some(deletion) = &result.deletion
    {
        let config = state.search_config.config();
        let version = deletion
            .version_id
            .parse()
            .map_err(|error: std::num::ParseIntError| ApiError::internal(error.to_string()))?;
        if config.registry.apply_resource(&id, version, None)
            && let Some(cache) = &config.cache
        {
            cache.clear();
        }
    }
    Ok((
        StatusCode::NO_CONTENT,
        deletion_headers(result.deletion.as_ref())?,
    ))
}

/// DELETE /[type]?[search params] - Conditional single-resource delete.
#[tracing::instrument(name = "fhir.conditional_delete", skip_all, fields(resource_type = %resource_type))]
pub async fn conditional_delete_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    authorization: Option<Extension<crate::write::AuthorizedWrite>>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request("Unknown resource type"));
    }
    let query = raw
        .filter(|query| !query.is_empty())
        .ok_or_else(|| ApiError::bad_request("Conditional delete requires search parameters"))?;
    let if_match = crate::write::if_match_header(&headers)?;
    let write = crate::write::PreparedWrite::prepare(
        &state,
        crate::write::WriteMode::Delete,
        &resource_type,
        None,
        authorization.map(|Extension(authorization)| authorization.0),
    )
    .await?;
    let conditional = write.conditional(&state, &query).await?;
    let result = match conditional.delete(&state, if_match.as_deref()).await {
        Ok(result) => result,
        Err(error) => return Err(map_write_error(error, &Value::Null, &state).await),
    };
    if let Some(previous) = &result.previous {
        if let Some(cache) = &state.resource_cache {
            cache.invalidate(&resource_type, &previous.id).await;
        }
        if resource_type == "SearchParameter"
            && let Some(url) = previous.resource["url"].as_str()
        {
            let config = state.search_config.config();
            if config.registry.remove_by_url(url)
                && let Some(cache) = &config.cache
            {
                cache.clear();
            }
        }
    }
    Ok((
        StatusCode::NO_CONTENT,
        deletion_headers(result.deletion.as_ref())?,
    ))
}

/// PATCH /[type]/[id] - Patch a resource using JSON Patch (RFC 6902)
#[tracing::instrument(name = "fhir.patch", skip_all, fields(resource_type = %resource_type, id = %id))]
pub async fn patch_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    authorization: Option<Extension<crate::write::AuthorizedWrite>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
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

    let media_type = content_type.split(';').next().unwrap_or("").trim();
    let is_json_patch = media_type.eq_ignore_ascii_case("application/json-patch+json");
    let is_fhirpath_patch = media_type.eq_ignore_ascii_case("application/fhir+json");

    if !is_json_patch && !is_fhirpath_patch {
        return Err(ApiError::unsupported_media_type(format!(
            "PATCH requires Content-Type: application/json-patch+json or application/fhir+json, got: {}",
            content_type
        )));
    }

    // Extract If-Match header for version checking
    let if_match = crate::write::if_match_header(&headers)?;

    // Parse Prefer header for return preference
    let prefer_return = headers
        .get("Prefer")
        .and_then(|h| h.to_str().ok())
        .map(parse_prefer_return);

    let write = crate::write::PreparedWrite::prepare(
        &state,
        crate::write::WriteMode::Update,
        &resource_type,
        Some(&id),
        authorization.map(|Extension(authorization)| authorization.0),
    )
    .await?;
    let existing = match write.snapshot(&state).await {
        Ok(existing) => existing,
        Err(error) => return Err(map_write_error(error, &Value::Null, &state).await),
    };

    // Apply patch based on content type
    let patched = if is_json_patch {
        apply_json_patch(&existing.resource, &body)
    } else {
        // FHIRPath Patch
        let model_provider_trait: SharedModelProvider = state.model_provider.clone();
        apply_fhirpath_patch(
            &state.fhirpath_engine,
            &model_provider_trait,
            &existing.resource,
            &body,
        )
        .await
    };
    // A denied resource policy must not disclose data through patch errors.
    if let Err(error) = write
        .authorize_snapshot(&state, &existing, patched.as_ref().ok())
        .await
    {
        return Err(map_write_error(error, &Value::Null, &state).await);
    }
    if let Some(expected_version) = &if_match
        && expected_version != &existing.version_id
    {
        return Err(ApiError::precondition_failed(
            "Resource version does not match If-Match",
        ));
    }
    let mut patched_json = patched?;

    // Basic structural validation (resourceType match)
    let resource_types = state.resource_type_set.load();
    if let Err(e) =
        crate::validation::validate_resource(&resource_type, &patched_json, resource_types.as_ref())
    {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }

    // Full schema + FHIRPath constraint validation using ValidationService
    let references = validate_for_checked_write(&patched_json, &state).await?;

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
    preprocess_payload(&resource_type, &mut patched_json).await?;

    // Always pin the snapshot used for patching, even without a client ETag.
    // Final native grants and policies are checked again on the locked row and
    // actual resulting JSON; a concurrent change cannot be silently overwritten.
    match write
        .run(
            &state,
            Some(&patched_json),
            &references,
            Some(&existing.version_id),
            true,
        )
        .await
    {
        Ok(result) => {
            let stored = result.stored.expect("PATCH returns a stored resource");
            postprocess_resource(&resource_type, &id, &stored.resource, &state).await?;
            let version_id = &stored.version_id;
            let last_updated_ts = stored.last_updated.unix_timestamp();

            let mut response_headers = HeaderMap::new();

            insert_header_if_valid(
                &mut response_headers,
                header::CONTENT_LOCATION,
                fhir_versioned_resource_url(
                    &state.fhir_base_url(),
                    &resource_type,
                    &id,
                    version_id,
                ),
            );

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
                    Ok((StatusCode::OK, response_headers).into_response())
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
                    Ok((StatusCode::OK, response_headers, Json(outcome)).into_response())
                }
                _ => Ok((StatusCode::OK, response_headers, Json(stored.resource)).into_response()),
            }
        }
        Err(error) => Err(map_write_error(error, &patched_json, &state).await),
    }
}

/// PATCH /[type]?[search params] - Conditional patch with snapshot version pinning.
#[tracing::instrument(name = "fhir.conditional_patch", skip_all, fields(resource_type = %resource_type))]
pub async fn conditional_patch_resource(
    State(state): State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    authorization: Option<Extension<crate::write::AuthorizedWrite>>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request("Unknown resource type"));
    }
    let query = raw
        .filter(|query| !query.is_empty())
        .ok_or_else(|| ApiError::bad_request("Conditional patch requires search parameters"))?;
    let media = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let media = media.split(';').next().unwrap_or("").trim();
    let json_patch = media.eq_ignore_ascii_case("application/json-patch+json");
    if !json_patch && !media.eq_ignore_ascii_case("application/fhir+json") {
        return Err(ApiError::unsupported_media_type(
            "PATCH requires application/json-patch+json or application/fhir+json",
        ));
    }
    let if_match = crate::write::if_match_header(&headers)?;
    let write = crate::write::PreparedWrite::prepare(
        &state,
        crate::write::WriteMode::Update,
        &resource_type,
        None,
        authorization.map(|Extension(authorization)| authorization.0),
    )
    .await?;
    let conditional = write.conditional(&state, &query).await?;
    let previews = match conditional.patch_snapshots(&state).await {
        Ok(previews) if previews.is_empty() => {
            return Err(ApiError::not_found(
                "No resource matches the conditional criteria",
            ));
        }
        Ok(previews) => previews,
        Err(error) => return Err(map_write_error(error, &Value::Null, &state).await),
    };
    let mut prepared = Vec::new();
    for previous in previews {
        let patched = if json_patch {
            apply_json_patch(&previous.resource, &body)
        } else {
            let model: SharedModelProvider = state.model_provider.clone();
            apply_fhirpath_patch(&state.fhirpath_engine, &model, &previous.resource, &body).await
        };
        if let Err(error) = write
            .authorize_snapshot(&state, &previous, patched.as_ref().ok())
            .await
        {
            return Err(map_write_error(error, &Value::Null, &state).await);
        }
        prepared.push((previous, patched));
    }
    if prepared.len() > 1 {
        return Err(ApiError::precondition_failed(
            "Multiple resources match the conditional criteria",
        ));
    }
    let (previous, patched) = prepared.pop().expect("nonempty conditional selection");
    if let Some(expected) = &if_match
        && expected != &previous.version_id
    {
        return Err(ApiError::precondition_failed(
            "Resource version does not match If-Match",
        ));
    }
    let mut candidate = patched?;
    validate_payload_structure(
        &resource_type,
        &candidate,
        &IdPolicy::Update {
            path_id: previous.id.clone(),
        },
    )
    .map_err(ApiError::bad_request)?;
    let references = validate_for_checked_write(&candidate, &state).await?;
    preprocess_payload(&resource_type, &mut candidate).await?;
    let result = match conditional
        .patch(
            &state,
            &previous,
            &candidate,
            &references,
            if_match.as_deref(),
        )
        .await
    {
        Ok(result) => result,
        Err(error) => return Err(map_write_error(error, &candidate, &state).await),
    };
    conditional_write_response(&state, &resource_type, result, &headers, "patched", false).await
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
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    State(state): State<crate::server::AppState>,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
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

    // Parse Prefer header for handling mode (strict/lenient)
    let unknown_param_handling = headers
        .get("Prefer")
        .and_then(|h| h.to_str().ok())
        .map(octofhir_search::UnknownParamHandling::from_prefer_header);

    let raw_q = raw.unwrap_or_default();
    let debug_request = search_debug_request(&state.config.search, &headers, &raw_q);
    let raw_q = strip_search_debug_params(&raw_q);
    let cfg = state.search_config.config();

    // Parse query string to SearchParams
    let mut search_params =
        octofhir_search::parse_query_string(&raw_q, cfg.default_count as u32, cfg.max_count as u32);
    apply_search_visibility(&state, visibility.clone(), &mut search_params).await?;

    // Strip result params from query suffix for pagination links

    let offset = search_params.offset.unwrap_or(0) as usize;
    let count = search_params.count.unwrap_or(10) as usize;

    // Execute search with raw JSON optimization and handling mode
    let result = execute_authorized_type_search(
        &state,
        visibility.as_ref().map(|visibility| &visibility.0),
        &resource_type,
        &search_params,
        octofhir_db_postgres::queries::RawSearchOptions {
            include_limits: state.config.search.include_limits,
            unknown_param_handling,
            collect_debug_plan: debug_request.collect_plan(),
            collect_explain_plan: debug_request.collect_explain_plan(),
            collect_explain_analyze: debug_request.collect_explain_analyze(),
            max_valueset_expansion: Some(state.config.search.max_valueset_expansion),
        },
    )
    .await?;

    let suffix = build_applied_query_suffix(&raw_q, &result.ignored_parameters);

    if debug_request.collect_plan() {
        let debug = result
            .debug
            .as_ref()
            .map(|debug| vec![(Some(resource_type.as_str()), debug)])
            .unwrap_or_default();
        return Ok((
            StatusCode::OK,
            Json(search_debug_parameters_resource(debug)),
        )
            .into_response());
    }

    let (total, total_is_exact) =
        resolved_search_total(result.total, result.has_more, offset, result.entries.len());

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

    // Create OperationOutcome warnings if there were unknown parameters
    let warnings = if !result.warnings.is_empty() {
        Some(octofhir_api::OperationOutcome::warnings(result.warnings))
    } else {
        None
    };

    let bundle = octofhir_api::bundle_from_search_raw_with_warnings_and_pagination(
        total,
        total_is_exact,
        result.has_more,
        resources,
        ids,
        included,
        &search_service_base(&state, uri.path()),
        &resource_type,
        offset,
        count,
        suffix.as_deref(),
        warnings,
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

fn search_service_base(state: &crate::server::AppState, path: &str) -> String {
    if path == "/fhir" || path.starts_with("/fhir/") {
        state.fhir_base_url()
    } else {
        state.base_url.trim_end_matches('/').to_string()
    }
}

async fn execute_authorized_type_search(
    state: &crate::server::AppState,
    visibility: Option<&crate::authorization::AuthorizedSearch>,
    resource_type: &str,
    params: &octofhir_storage::SearchParams,
    options: octofhir_db_postgres::queries::RawSearchOptions,
) -> Result<octofhir_storage::RawSearchResult, ApiError> {
    if let Some(visibility) = visibility {
        let policies = visibility
            .prepare_snapshots(state, &[resource_type], params)
            .await?;
        if policies.required() {
            return policies
                .execute(state, &[resource_type], params, options)
                .await;
        }
    }
    octofhir_db_postgres::queries::execute_search_raw_with_terminology_options(
        &state.read_db_pool,
        resource_type,
        params,
        Some(&state.search_config.config().registry),
        state.query_cache.as_deref(),
        state.terminology_provider.as_ref(),
        options,
    )
    .await
    .map_err(map_storage_error)
}

async fn execute_authorized_aggregate_search(
    state: &crate::server::AppState,
    visibility: Option<&crate::authorization::AuthorizedSearch>,
    types: &[&str],
    params: &octofhir_storage::SearchParams,
    options: octofhir_db_postgres::queries::RawSearchOptions,
) -> Result<octofhir_storage::RawSearchResult, ApiError> {
    if let Some(visibility) = visibility {
        let policies = visibility.prepare_snapshots(state, types, params).await?;
        if policies.required() {
            return policies.execute(state, types, params, options).await;
        }
    }
    octofhir_db_postgres::queries::search::execute_system_search_raw(
        &state.read_db_pool,
        types,
        params,
        &state.search_config.config().registry,
        state.terminology_provider.as_ref(),
        options,
    )
    .await
    .map_err(map_storage_error)
}

async fn apply_search_visibility(
    state: &crate::server::AppState,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    params: &mut octofhir_storage::SearchParams,
) -> Result<(), ApiError> {
    if let Some(Extension(visibility)) = visibility {
        visibility.apply(state, params).await
    } else if state.config.auth.policy.anonymous_access {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "Search authorization context unavailable",
        ))
    }
}

/// POST /[type]/_search - Search via POST with form-encoded parameters
#[tracing::instrument(name = "fhir.search.post", skip_all, fields(resource_type = %resource_type))]
pub async fn search_resource_post(
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    State(state): State<crate::server::AppState>,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
    Path(resource_type): Path<String>,
    RawQuery(raw): RawQuery,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    // Validate resource type
    if resource_type.parse::<ResourceType>().is_err() {
        return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        )));
    }

    let body_q = std::str::from_utf8(&body)
        .map_err(|_| ApiError::bad_request("Search form body must be valid UTF-8"))?;
    let url_q = raw.unwrap_or_default();
    let raw_q = match (url_q.is_empty(), body_q.is_empty()) {
        (true, true) => String::new(),
        (true, false) => body_q.to_string(),
        (false, true) => url_q,
        (false, false) => {
            let mut merged = String::with_capacity(url_q.len() + 1 + body_q.len());
            merged.push_str(&url_q);
            merged.push('&');
            merged.push_str(body_q);
            merged
        }
    };
    let debug_request = search_debug_request(&state.config.search, &headers, &raw_q);
    let raw_q = strip_search_debug_params(&raw_q);

    let cfg = state.search_config.config();
    let unknown_param_handling = headers
        .get("Prefer")
        .and_then(|h| h.to_str().ok())
        .map(octofhir_search::UnknownParamHandling::from_prefer_header);

    // Parse query string to SearchParams
    let mut search_params =
        octofhir_search::parse_query_string(&raw_q, cfg.default_count as u32, cfg.max_count as u32);
    apply_search_visibility(&state, visibility.clone(), &mut search_params).await?;

    // Strip result params from query suffix for pagination links

    let offset = search_params.offset.unwrap_or(0) as usize;
    let count = search_params.count.unwrap_or(10) as usize;

    // Execute search with raw JSON optimization and terminology modifier support.
    let result = execute_authorized_type_search(
        &state,
        visibility.as_ref().map(|visibility| &visibility.0),
        &resource_type,
        &search_params,
        octofhir_db_postgres::queries::RawSearchOptions {
            include_limits: state.config.search.include_limits,
            unknown_param_handling,
            collect_debug_plan: debug_request.collect_plan(),
            collect_explain_plan: debug_request.collect_explain_plan(),
            collect_explain_analyze: debug_request.collect_explain_analyze(),
            max_valueset_expansion: Some(state.config.search.max_valueset_expansion),
        },
    )
    .await?;

    let suffix = build_applied_query_suffix(&raw_q, &result.ignored_parameters);

    if debug_request.collect_plan() {
        let debug = result
            .debug
            .as_ref()
            .map(|debug| vec![(Some(resource_type.as_str()), debug)])
            .unwrap_or_default();
        return Ok((
            StatusCode::OK,
            Json(search_debug_parameters_resource(debug)),
        )
            .into_response());
    }

    let (total, total_is_exact) =
        resolved_search_total(result.total, result.has_more, offset, result.entries.len());

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

    // Create OperationOutcome warnings if there were unknown parameters
    let warnings = if !result.warnings.is_empty() {
        Some(octofhir_api::OperationOutcome::warnings(result.warnings))
    } else {
        None
    };

    let bundle = octofhir_api::bundle_from_search_raw_with_warnings_and_pagination(
        total,
        total_is_exact,
        result.has_more,
        resources,
        ids,
        included,
        &search_service_base(&state, uri.path()),
        &resource_type,
        offset,
        count,
        suffix.as_deref(),
        warnings,
    );

    let result_params: HashMap<String, String> = ["_summary", "_elements"]
        .into_iter()
        .filter_map(|key| {
            search_params
                .parameters
                .get(key)
                .and_then(|values| values.first())
                .map(|value| (key.to_string(), value.clone()))
        })
        .collect();

    if !result_params.is_empty() {
        let bundle_value = apply_result_params(bundle, &result_params)?;
        return Ok((StatusCode::OK, Json(bundle_value)).into_response());
    }

    // Return Bundle directly - RawJson entries serialize efficiently via RawValue
    Ok((StatusCode::OK, Json(bundle)).into_response())
}

/// GET / or GET /?_type=Patient,Observation - System-level search
#[tracing::instrument(name = "fhir.search.system", skip_all)]
pub async fn system_search(
    State(state): State<crate::server::AppState>,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, ApiError> {
    let type_names = system_search_types(&state, params.get("_type").map(String::as_str));
    execute_aggregate_search(
        state,
        visibility,
        headers,
        AggregateSearch {
            types: type_names,
            query: raw.unwrap_or_default(),
            path: String::new(),
            membership: None,
        },
    )
    .await
}

fn system_search_types(state: &crate::server::AppState, requested: Option<&str>) -> Vec<String> {
    if let Some(types_param) = requested {
        types_param
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    } else {
        state
            .resource_type_set
            .load()
            .iter()
            .filter(|name| name.parse::<ResourceType>().is_ok() && !is_internal_resource_type(name))
            .cloned()
            .collect()
    }
}

struct AggregateSearch {
    types: Vec<String>,
    query: String,
    path: String,
    membership: Option<HashMap<String, octofhir_storage::SearchConstraint>>,
}

fn apply_compartment_membership(
    params: &mut octofhir_storage::SearchParams,
    types: &[String],
    membership: &HashMap<String, octofhir_storage::SearchConstraint>,
) {
    let grants = params.type_constraints.take();
    let mut constraints = HashMap::new();
    for resource_type in types {
        let grant = match &grants {
            Some(grants) => grants
                .get(resource_type)
                .cloned()
                .unwrap_or_else(|| octofhir_storage::SearchConstraint::Or(Vec::new())),
            None => octofhir_storage::SearchConstraint::And(Vec::new()),
        };
        // A denied branch must not compile compartment parameters that
        // are irrelevant to this caller (or unavailable for that type).
        if matches!(&grant, octofhir_storage::SearchConstraint::Or(parts) if parts.is_empty()) {
            constraints.insert(resource_type.clone(), grant);
            continue;
        }
        let member = membership
            .get(resource_type)
            .cloned()
            .unwrap_or_else(|| octofhir_storage::SearchConstraint::Or(Vec::new()));
        constraints.insert(
            resource_type.clone(),
            octofhir_storage::SearchConstraint::And(vec![grant, member]),
        );
    }
    params.type_constraints = Some(constraints);
}

async fn execute_aggregate_search(
    state: crate::server::AppState,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
    request: AggregateSearch,
) -> Result<Response, ApiError> {
    let type_names = request.types;
    let types: Vec<&str> = type_names.iter().map(String::as_str).collect();
    let params: HashMap<String, String> = url::form_urlencoded::parse(request.query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if types.is_empty() {
        return Err(ApiError::bad_request(
            "_type parameter must specify at least one resource type",
        ));
    }

    let raw_q = request.query;
    let debug_request = search_debug_request(&state.config.search, &headers, &raw_q);
    let raw_q = strip_search_debug_params(&raw_q);
    let cfg = state.search_config.config();
    let unknown_param_handling = headers
        .get("Prefer")
        .and_then(|h| h.to_str().ok())
        .map(octofhir_search::UnknownParamHandling::from_prefer_header);

    if types.iter().any(|name| {
        name.parse::<ResourceType>().is_err() || !state.resource_type_set.load().contains(*name)
    }) {
        return Err(ApiError::bad_request(
            "_type contains an unsupported resource type",
        ));
    }
    let mut search_params =
        octofhir_search::parse_query_string(&raw_q, cfg.default_count as u32, cfg.max_count as u32);
    apply_aggregate_search_visibility(&state, visibility.clone(), &type_names, &mut search_params)
        .await?;
    if let Some(membership) = request.membership {
        apply_compartment_membership(&mut search_params, &type_names, &membership);
    }
    let result = execute_authorized_aggregate_search(
        &state,
        visibility.as_ref().map(|visibility| &visibility.0),
        &types,
        &search_params,
        octofhir_db_postgres::queries::RawSearchOptions {
            include_limits: state.config.search.include_limits,
            unknown_param_handling,
            collect_debug_plan: debug_request.collect_plan(),
            collect_explain_plan: debug_request.collect_explain_plan(),
            collect_explain_analyze: debug_request.collect_explain_analyze(),
            max_valueset_expansion: Some(state.config.search.max_valueset_expansion),
        },
    )
    .await?;
    if debug_request.collect_plan() {
        let debug = result
            .debug
            .as_ref()
            .map(|debug| (None, debug))
            .into_iter()
            .collect();
        return Ok((
            StatusCode::OK,
            Json(search_debug_parameters_resource(debug)),
        )
            .into_response());
    }
    let bundle =
        build_aggregate_search_bundle(&state, &search_params, &raw_q, &request.path, result);

    if params.contains_key("_summary") || params.contains_key("_elements") {
        let bundle_value = apply_result_params(bundle, &params)?;
        return Ok((StatusCode::OK, Json(bundle_value)).into_response());
    }

    Ok((StatusCode::OK, Json(bundle)).into_response())
}

fn build_aggregate_search_bundle(
    state: &crate::server::AppState,
    search_params: &octofhir_storage::SearchParams,
    raw_q: &str,
    path: &str,
    result: octofhir_storage::RawSearchResult,
) -> octofhir_api::Bundle {
    let fhir_base = state.fhir_base_url();
    let count = search_params
        .count
        .unwrap_or(state.search_config.config().default_count as u32) as usize;
    let offset = search_params.offset.unwrap_or(0) as usize;
    let (total, exact) =
        resolved_search_total(result.total, result.has_more, offset, result.entries.len());
    let suffix = build_applied_query_suffix(raw_q, &result.ignored_parameters);
    let links = octofhir_api::build_search_links_with_total_mode(
        total,
        exact,
        result.has_more,
        &fhir_base,
        path,
        offset,
        count,
        suffix.as_deref(),
    );
    let mut entries: Vec<octofhir_api::BundleEntry> = result
        .entries
        .into_iter()
        .map(|r| (r, "match"))
        .chain(result.included.into_iter().map(|r| (r, "include")))
        .map(|(entry, mode)| octofhir_api::BundleEntry {
            full_url: Some(format!(
                "{}/{}/{}",
                fhir_base, entry.resource_type, entry.id
            )),
            resource: Some(octofhir_api::RawJson::from_string(entry.resource_json)),
            search: Some(octofhir_api::BundleEntrySearch {
                mode: mode.into(),
                score: None,
            }),
            request: None,
            response: None,
        })
        .collect();
    if count > 0 && !result.warnings.is_empty() {
        entries.push(octofhir_api::BundleEntry {
            full_url: None,
            resource: Some(
                serde_json::to_value(octofhir_api::OperationOutcome::warnings(result.warnings))
                    .expect("OperationOutcome serializes")
                    .into(),
            ),
            search: Some(octofhir_api::BundleEntrySearch {
                mode: "outcome".into(),
                score: None,
            }),
            request: None,
            response: None,
        });
    }
    octofhir_api::Bundle::searchset_with_total(total.map(|n| n as u64), entries, links)
}

/// POST /_search uses the same system query path after merging URL and form criteria.
pub async fn system_search_post(
    state: State<crate::server::AppState>,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    body: Bytes,
) -> Result<Response, ApiError> {
    let query = merge_search_query(raw, &body)?;
    let parameters = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    system_search(
        state,
        visibility,
        headers,
        Query(parameters),
        RawQuery(Some(query)),
    )
    .await
    .map(IntoResponse::into_response)
}

async fn apply_aggregate_search_visibility(
    state: &crate::server::AppState,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    types: &[String],
    params: &mut octofhir_storage::SearchParams,
) -> Result<(), ApiError> {
    if let Some(Extension(visibility)) = visibility {
        visibility.apply_types(state, types, params).await
    } else if state.config.auth.policy.anonymous_access {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "Search authorization context unavailable",
        ))
    }
}

fn merge_search_query(raw: Option<String>, body: &[u8]) -> Result<String, ApiError> {
    let body = std::str::from_utf8(body)
        .map_err(|_| ApiError::bad_request("Search form body must be valid UTF-8"))?;
    let query = raw.unwrap_or_default();
    Ok(match (query.is_empty(), body.is_empty()) {
        (_, true) => query,
        (true, false) => body.to_string(),
        (false, false) => format!("{query}&{body}"),
    })
}

/// Keep every applied criterion and projection in self/continuation links.
/// Paging controls are rendered separately using the actual clamped values.
#[cfg(test)]
fn build_query_suffix_for_links(raw_q: &str) -> Option<String> {
    build_applied_query_suffix(raw_q, &[])
}

fn build_applied_query_suffix(raw_q: &str, ignored: &[String]) -> Option<String> {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in url::form_urlencoded::parse(raw_q.as_bytes()) {
        let name = key.split(':').next().unwrap_or(&key);
        if !matches!(key.as_ref(), "_count" | "_offset")
            && !ignored.iter().any(|ignored| ignored == name)
        {
            serializer.append_pair(&key, &value);
        }
    }
    let query = serializer.finish();
    (!query.is_empty()).then_some(query)
}

fn resolved_search_total(
    explicit_total: Option<u32>,
    has_more: bool,
    offset: usize,
    page_len: usize,
) -> (Option<usize>, bool) {
    if let Some(total) = explicit_total {
        return (Some(total as usize), true);
    }

    // An empty page beyond offset zero only establishes an upper bound.
    // It cannot distinguish an exhausted result set from an oversized offset.
    if has_more || (page_len == 0 && offset > 0) {
        return (None, false);
    }

    match offset.checked_add(page_len) {
        Some(total) => (Some(total), true),
        None => (None, false),
    }
}

const SEARCH_DEBUG_PARAM: &str = "_debug";
const SEARCH_DEBUG_VALUE: &str = "search";
const SEARCH_PLAN_DEBUG_VALUE: &str = "search-plan";
const SEARCH_SQL_DEBUG_VALUE: &str = "search-sql";
const SEARCH_EXPLAIN_DEBUG_VALUE: &str = "search-explain";
const SEARCH_EXPLAIN_ANALYZE_DEBUG_VALUE: &str = "search-explain-analyze";
const SEARCH_HEADER_PLAN_VALUE: &str = "plan";
const SEARCH_HEADER_SQL_VALUE: &str = "sql";
const SEARCH_HEADER_EXPLAIN_VALUE: &str = "explain";
const SEARCH_HEADER_EXPLAIN_ANALYZE_VALUE: &str = "explain-analyze";
const SEARCH_DEBUG_HEADER: &str = "x-octofhir-search-debug";
const LEGACY_SEARCH_DEBUG_HEADER: &str = "x-octofhir-debug";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchDebugRequest {
    None,
    Plan,
    Explain,
    ExplainAnalyze,
}

impl SearchDebugRequest {
    fn collect_plan(self) -> bool {
        !matches!(self, Self::None)
    }

    fn collect_explain_plan(self) -> bool {
        matches!(self, Self::Explain | Self::ExplainAnalyze)
    }

    fn collect_explain_analyze(self) -> bool {
        matches!(self, Self::ExplainAnalyze)
    }
}

fn search_debug_request(
    settings: &crate::config::SearchSettings,
    headers: &HeaderMap,
    raw_q: &str,
) -> SearchDebugRequest {
    if !settings.allow_debug_search_plan {
        return SearchDebugRequest::None;
    }

    let request = query_search_debug_request(raw_q)
        .or_else(|| header_search_debug_request(headers))
        .unwrap_or(SearchDebugRequest::None);

    if matches!(request, SearchDebugRequest::ExplainAnalyze)
        && !settings.allow_debug_search_explain_analyze
    {
        return SearchDebugRequest::None;
    }

    request
}

fn query_search_debug_request(raw_q: &str) -> Option<SearchDebugRequest> {
    url::form_urlencoded::parse(raw_q.as_bytes()).find_map(|(key, value)| {
        (key == SEARCH_DEBUG_PARAM)
            .then(|| parse_search_debug_value(&value))
            .flatten()
    })
}

fn header_search_debug_request(headers: &HeaderMap) -> Option<SearchDebugRequest> {
    headers
        .get(SEARCH_DEBUG_HEADER)
        .or_else(|| headers.get(LEGACY_SEARCH_DEBUG_HEADER))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(',')
                .map(str::trim)
                .find_map(parse_search_debug_value)
        })
}

fn parse_search_debug_value(value: &str) -> Option<SearchDebugRequest> {
    match value {
        SEARCH_DEBUG_VALUE
        | SEARCH_SQL_DEBUG_VALUE
        | SEARCH_PLAN_DEBUG_VALUE
        | SEARCH_HEADER_PLAN_VALUE
        | SEARCH_HEADER_SQL_VALUE => Some(SearchDebugRequest::Plan),
        SEARCH_EXPLAIN_DEBUG_VALUE | SEARCH_HEADER_EXPLAIN_VALUE => {
            Some(SearchDebugRequest::Explain)
        }
        SEARCH_EXPLAIN_ANALYZE_DEBUG_VALUE | SEARCH_HEADER_EXPLAIN_ANALYZE_VALUE => {
            Some(SearchDebugRequest::ExplainAnalyze)
        }
        _ => None,
    }
}

fn strip_search_debug_params(raw_q: &str) -> String {
    raw_q
        .split('&')
        .filter(|part| !is_search_debug_param(part))
        .collect::<Vec<_>>()
        .join("&")
}

fn is_search_debug_param(raw_query_part: &str) -> bool {
    let key = raw_query_part
        .split_once('=')
        .map(|(key, _)| key)
        .unwrap_or(raw_query_part);

    key == SEARCH_DEBUG_PARAM
        || urlencoding::decode(key)
            .map(|decoded| decoded == SEARCH_DEBUG_PARAM)
            .unwrap_or(false)
}

fn search_debug_parameters_resource(
    debug_entries: Vec<(Option<&str>, &octofhir_storage::RawSearchDebug)>,
) -> Value {
    let parameter = debug_entries
        .into_iter()
        .map(|(resource_type, debug)| {
            let mut parts = Vec::new();
            if let Some(resource_type) = resource_type {
                parts.push(json!({"name": "resourceType", "valueString": resource_type}));
            }
            parts.push(json!({"name": "engine", "valueString": "native-ir"}));
            parts.push(json!({"name": "analyze", "valueBoolean": debug.analyze}));
            if let Some(sql_shape) = &debug.sql_shape {
                parts.push(json!({"name": "sql", "valueString": sql_shape}));
            }
            if let Some(plan) = &debug.plan {
                parts.push(json!({
                    "name": "plan",
                    "valueString": serde_json::to_string_pretty(plan).unwrap_or_else(|_| "null".to_string())
                }));
            }
            if let Some(explain) = &debug.explain {
                parts.push(json!({
                    "name": "explain",
                    "valueString": serde_json::to_string_pretty(explain).unwrap_or_else(|_| "null".to_string())
                }));
            }
            if let Some(value) = debug.build_elapsed_ms {
                parts.push(json!({"name": "buildElapsedMs", "valueDecimal": value}));
            }
            if let Some(value) = debug.db_execute_elapsed_ms {
                parts.push(json!({"name": "dbExecuteElapsedMs", "valueDecimal": value}));
            }

            json!({
                "name": "search",
                "part": parts
            })
        })
        .collect::<Vec<_>>();

    json!({
        "resourceType": "Parameters",
        "parameter": parameter
    })
}

/// Apply _summary and _elements result parameters to a bundle
pub(crate) fn apply_result_params(
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

                if should_tag_subsetted(params) {
                    add_subsetted_tag(resource);
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
    /// CQL (Clinical Quality Language)
    pub cql: bool,
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
            cql: config.cql.enabled,
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
            ApiError::NotFound(format!(
                "Schema not found for resource type: {}",
                resource_type
            ))
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
            if let Some(ref cat) = params.category
                && &op.category != cat
            {
                return false;
            }
            // Module filter
            if let Some(ref module) = params.module
                && &op.module != module
            {
                return false;
            }
            // Public filter
            if let Some(public) = params.public
                && op.public != public
            {
                return false;
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
            .filter(|app: &App| app.is_active())
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
        let full_path = if let Some(base_path) = &app.base_path {
            format!("{}{}", base_path, custom_op.path)
        } else {
            custom_op.path.clone()
        };

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
    if body.description.is_none()
        && let Some(public) = body.public
    {
        match state
            .operation_registry
            .set_operation_public(&id, public)
            .await
        {
            Ok(Some(op)) => {
                tracing::info!(operation_id = %id, public = public, "Operation public flag updated");
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
            if body.public.is_some()
                && let Err(e) = state.operation_registry.sync_operations(false).await
            {
                tracing::warn!(error = %e, "Failed to sync operation registry after update");
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
pub(crate) fn map_storage_error(e: StorageError) -> ApiError {
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
            format!("Version conflict: expected {expected}, got {actual}"),
        ),
        StorageError::WriteConflict { message } => ApiError::conflict(message),
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

struct PreparedBundleSearch {
    request: AggregateSearch,
    params: octofhir_storage::SearchParams,
    strict: bool,
    snapshots: Option<crate::authorization::PreparedSnapshotSearch>,
}

async fn prepare_bundle_search(
    state: &crate::server::AppState,
    entry: &Value,
    visibility: Option<crate::authorization::AuthorizedSearch>,
    headers: &HeaderMap,
) -> Result<Option<PreparedBundleSearch>, ApiError> {
    if !entry["request"]["method"]
        .as_str()
        .is_some_and(|method| method.eq_ignore_ascii_case("GET"))
    {
        return Ok(None);
    }
    let url = entry["request"]["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.url"))?;
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let path = path.trim_start_matches('/');
    let parts: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
    let request = match parts.as_slice() {
        [] => {
            let requested = url::form_urlencoded::parse(query.as_bytes())
                .filter(|(key, _)| key == "_type")
                .last()
                .map(|(_, value)| value.into_owned());
            AggregateSearch {
                types: system_search_types(state, requested.as_deref()),
                query: query.into(),
                path: String::new(),
                membership: None,
            }
        }
        [resource_type] if !resource_type.starts_with('_') && !resource_type.starts_with('$') => {
            AggregateSearch {
                types: vec![(*resource_type).into()],
                query: query.into(),
                path: (*resource_type).into(),
                membership: None,
            }
        }
        [compartment_type, compartment_id, resource_type]
            if !resource_type.starts_with('_') && !resource_type.starts_with('$') =>
        {
            compartment_search_request(
                state,
                CompartmentSearch {
                    compartment_type: (*compartment_type).into(),
                    compartment_id: (*compartment_id).into(),
                    resource_type: (*resource_type).into(),
                    query: query.into(),
                },
            )?
        }
        _ => return Ok(None),
    };
    if request.types.is_empty()
        || request.types.iter().any(|resource_type| {
            resource_type.parse::<ResourceType>().is_err()
                || !state.resource_type_set.load().contains(resource_type)
        })
    {
        return Err(ApiError::bad_request(
            "Search contains an unsupported resource type",
        ));
    }
    let cfg = state.search_config.config();
    let mut params = octofhir_search::parse_query_string(
        &request.query,
        cfg.default_count as u32,
        cfg.max_count as u32,
    );
    apply_aggregate_search_visibility(
        state,
        visibility.clone().map(Extension),
        &request.types,
        &mut params,
    )
    .await?;
    if let Some(membership) = &request.membership {
        apply_compartment_membership(&mut params, &request.types, membership);
    }
    let types: Vec<_> = request.types.iter().map(String::as_str).collect();
    let params = octofhir_db_postgres::queries::search::prepare_system_search_params(
        &types,
        &params,
        &cfg.registry,
        state.terminology_provider.as_ref(),
        state.config.search.max_valueset_expansion,
    )
    .await
    .map_err(map_storage_error)?;
    let strict = headers
        .get("Prefer")
        .and_then(|value| value.to_str().ok())
        .map(octofhir_search::UnknownParamHandling::from_prefer_header)
        == Some(octofhir_search::UnknownParamHandling::Strict);
    // Compile trusted predicates before a transaction's connection is acquired.
    // Keep both the predicates and handling mode for execution on that connection.
    for resource_type in &request.types {
        let scoped = params.for_resource_type(resource_type);
        octofhir_search::build_native_ir_query_from_params_with_config(
            resource_type,
            &scoped,
            &cfg.registry,
            "public",
            &octofhir_search::ParamsSearchConfig {
                unknown_param_handling: if strict {
                    octofhir_search::UnknownParamHandling::Strict
                } else {
                    octofhir_search::UnknownParamHandling::Lenient
                },
                collect_debug_plan: false,
            },
        )
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    }
    let snapshots = if let Some(visibility) = &visibility {
        let prepared = visibility.prepare_snapshots(state, &types, &params).await?;
        prepared.required().then_some(prepared)
    } else {
        None
    };
    Ok(Some(PreparedBundleSearch {
        request,
        params,
        strict,
        snapshots,
    }))
}

fn bundle_search_response(
    state: &crate::server::AppState,
    prepared: &PreparedBundleSearch,
    result: octofhir_storage::RawSearchResult,
) -> Result<Value, ApiError> {
    let bundle = build_aggregate_search_bundle(
        state,
        &prepared.params,
        &prepared.request.query,
        &prepared.request.path,
        result,
    );
    let projections: HashMap<String, String> = ["_summary", "_elements"]
        .into_iter()
        .filter_map(|key| {
            prepared
                .params
                .parameters
                .get(key)
                .and_then(|values| values.last())
                .map(|value| (key.into(), value.clone()))
        })
        .collect();
    let resource = if projections.is_empty() {
        serde_json::to_value(bundle).map_err(|error| ApiError::internal(error.to_string()))?
    } else {
        apply_result_params(bundle, &projections)?
    };
    Ok(json!({"response":{"status":"200 OK"}, "resource":resource}))
}

async fn execute_bundle_search(
    state: &crate::server::AppState,
    prepared: &PreparedBundleSearch,
) -> Result<Value, ApiError> {
    let types: Vec<_> = prepared.request.types.iter().map(String::as_str).collect();
    let cfg = state.search_config.config();
    if let Some(snapshots) = &prepared.snapshots {
        let result = snapshots
            .execute(
                state,
                &types,
                &prepared.params,
                octofhir_db_postgres::queries::RawSearchOptions {
                    include_limits: state.config.search.include_limits,
                    unknown_param_handling: Some(if prepared.strict {
                        octofhir_search::UnknownParamHandling::Strict
                    } else {
                        octofhir_search::UnknownParamHandling::Lenient
                    }),
                    ..Default::default()
                },
            )
            .await?;
        return bundle_search_response(state, prepared, result);
    }
    let result = octofhir_db_postgres::queries::search::execute_system_search_raw(
        &state.read_db_pool,
        &types,
        &prepared.params,
        &cfg.registry,
        None,
        octofhir_db_postgres::queries::RawSearchOptions {
            include_limits: state.config.search.include_limits,
            unknown_param_handling: Some(if prepared.strict {
                octofhir_search::UnknownParamHandling::Strict
            } else {
                octofhir_search::UnknownParamHandling::Lenient
            }),
            ..Default::default()
        },
    )
    .await
    .map_err(map_storage_error)?;
    bundle_search_response(state, prepared, result)
}

/// POST / - Process transaction or batch bundle
#[tracing::instrument(name = "fhir.bundle", skip_all)]
pub async fn transaction_handler(
    State(state): State<crate::server::AppState>,
    Extension(auth): Extension<Arc<octofhir_auth::middleware::AuthContext>>,
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

    // FHIR R4 default for transaction-response entries is `return=representation`
    // (each entry's response includes the stored resource). Clients that
    // explicitly request `Prefer: return=minimal` get only status/location/etag.
    let bundle_include_resource = headers
        .get("prefer")
        .and_then(|v| v.to_str().ok())
        .map(|v| !v.to_ascii_lowercase().contains("return=minimal"))
        .unwrap_or(true);

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

    if !matches!(bundle_type, "transaction" | "batch") {
        return Err(ApiError::bad_request(
            "Expected transaction or batch Bundle",
        ));
    }
    let mut prepared_searches = HashMap::new();
    let mut prepared_reads = HashMap::new();
    let mut write_contexts = HashMap::new();
    if bundle_type == "transaction" || prefer_async {
        for (index, entry) in bundle["entry"]
            .as_array()
            .ok_or_else(|| ApiError::bad_request("Missing or invalid bundle entries"))?
            .iter()
            .enumerate()
        {
            let method = entry["request"]["method"].as_str().unwrap_or("");
            if matches!(
                method.to_ascii_uppercase().as_str(),
                "POST" | "PUT" | "PATCH" | "DELETE"
            ) {
                if let Some(authorization) =
                    crate::authorization::authorize_batch_write(&state, &auth, entry)?
                {
                    write_contexts.insert(index, authorization.0);
                }
                continue;
            }
            let visibility = crate::authorization::authorize_entry(&state, &auth, entry).await?;
            if let Some(read) =
                crate::authorization::prepare_entry_read(&state, &auth, entry).await?
            {
                prepared_reads.insert(index, read);
            }
            if let Some(prepared) =
                prepare_bundle_search(&state, entry, visibility, &headers).await?
            {
                prepared_searches.insert(index, prepared);
            }
        }
    }

    // If async requested, submit job
    if prefer_async {
        let request = crate::async_jobs::AsyncJobRequest {
            request_type: format!("bundle-{}", bundle_type),
            method: "POST".to_string(),
            url: "/".to_string(),
            body: Some(bundle),
            headers: Some(json!(
                headers
                    .iter()
                    .filter(|(name, _)| matches!(name.as_str(), "prefer" | "x-skip-validation"))
                    .filter_map(|(name, value)| value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_owned(), value.to_owned())))
                    .collect::<HashMap<_, _>>()
            )),
            client_id: Some(auth.client_id().to_string()),
            authorization: crate::async_jobs::TrustedJobAuthorization::capture(
                (!state.is_anonymous_context(&auth)).then_some(auth.as_ref()),
                state.config.auth.policy.anonymous_access,
            )?,
        };

        let job_id = state
            .async_job_manager
            .submit_job(request)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to submit async job: {}", e)))?;

        return Ok(create_async_accepted_response(
            job_id,
            &state.fhir_base_url(),
        ));
    }

    // Validation of POST entries mirrors single-create: on by default, honoring
    // X-Skip-Validation only when the config allows it (same gate as create_resource).
    let skip_validation = should_skip_validation(&headers, &state.config.validation);
    let mut prepared_writes =
        crate::write::PreparedTransactionWrites::prepare(&state, write_contexts).await?;

    // Otherwise, process synchronously
    match bundle_type {
        "transaction" => {
            let (status, json) = process_transaction(
                &state,
                bundle,
                bundle_include_resource,
                skip_validation,
                &prepared_searches,
                &prepared_reads,
                &mut prepared_writes,
            )
            .await?;
            Ok((status, HeaderMap::new(), json))
        }
        "batch" => {
            let (status, json) =
                process_batch(&state, &bundle, bundle_include_resource, &auth, &headers).await?;
            Ok((status, HeaderMap::new(), json))
        }
        _ => Err(ApiError::bad_request(format!(
            "Bundle type '{}' not supported at root endpoint. Use 'transaction' or 'batch'.",
            bundle_type
        ))),
    }
}

/// Workers use the same grouped transaction protocol, with the checkpoint and
/// resource changes committed together. A changed input cannot reuse a result.
pub(crate) async fn execute_worker_write_chunk(
    state: &crate::server::AppState,
    entries: Vec<Value>,
    skip_validation: bool,
    step: crate::async_jobs::CheckpointStep,
) -> Result<Value, ApiError> {
    let claim = crate::async_jobs::current_execution_claim()
        .ok_or_else(|| ApiError::internal("Worker has no execution claim"))?;
    let context = crate::operations::OperationContext::for_job(state, claim.job_id)
        .await
        .map_err(ApiError::from)?;
    if let Some(result) = state.async_job_manager.restore_checkpoint(&step).await? {
        return Ok(result["result"].clone());
    }
    let auth = context
        .auth()
        .cloned()
        .map(Arc::new)
        .unwrap_or_else(|| state.anonymous_auth_context.clone());
    let mut contexts = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        validate_bundle_entry_identity(entry)?;
        if let Some(authorization) =
            crate::authorization::authorize_batch_write(state, &auth, entry)?
        {
            contexts.insert(index, authorization.0);
        }
    }
    let mut writes = crate::write::PreparedTransactionWrites::prepare(state, contexts).await?;
    let bundle = json!({"resourceType":"Bundle", "type":"transaction", "entry":entries});
    let (_, Json(result)) = step
        .scope(process_transaction(
            state,
            bundle,
            false,
            skip_validation,
            &HashMap::new(),
            &HashMap::new(),
            &mut writes,
        ))
        .await?;
    Ok(result)
}

/// Process a transaction bundle atomically - all entries succeed or all fail.
///
/// Per FHIR R4 spec (http.html#transaction), uses pre-scan with pre-assigned IDs:
/// 1. Pre-scan: assign IDs to POST entries, build complete urn:uuid reference map
/// 2. Resolve: replace all urn:uuid references in all resources using the complete map
/// 3. Execute: process entries in verb order (DELETE → POST → PUT → GET) in one DB transaction
async fn process_transaction(
    state: &crate::server::AppState,
    mut bundle: Value,
    include_resource: bool,
    skip_validation: bool,
    prepared_searches: &HashMap<usize, PreparedBundleSearch>,
    prepared_reads: &HashMap<usize, crate::authorization::PreparedRead>,
    prepared_writes: &mut crate::write::PreparedTransactionWrites,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Some(Value::Array(mut planned_entries)) = bundle.get_mut("entry").map(Value::take) else {
        return Err(ApiError::bad_request("Missing or invalid bundle entries"));
    };

    if planned_entries.is_empty() {
        let response_bundle = json!({
            "resourceType": "Bundle",
            "type": "transaction-response",
            "entry": []
        });
        return Ok((StatusCode::OK, Json(response_bundle)));
    }

    for entry in &mut planned_entries {
        validate_bundle_entry_identity(entry)?;
        if entry["request"]["method"]
            .as_str()
            .is_some_and(|method| method.eq_ignore_ascii_case("POST"))
            && let Some(body) = entry.get_mut("resource")
        {
            body["id"] = json!(octofhir_core::generate_id());
        }
    }
    let entries = &planned_entries;

    // Sort entries by HTTP method for execution, while retaining the original
    // request index so the transaction-response can preserve FHIR response order.
    let sorted_entries = sort_transaction_entries(entries);
    let mut full_urls = HashSet::new();
    for (_, entry) in &sorted_entries {
        if entry["request"]["method"].as_str().is_some_and(|method| {
            matches!(
                method.to_ascii_uppercase().as_str(),
                "PUT" | "PATCH" | "DELETE"
            )
        }) {
            parse_bundle_if_match(&entry["request"])?;
        }
        if let Some(full_url) = entry["fullUrl"].as_str()
            && !full_urls.insert(full_url)
        {
            return Err(ApiError::bad_request("Duplicate fullUrl in transaction"));
        }
    }

    // Compile each deduplicated predicate for its actual c/u/d grant, before
    // checkout. Preview and final selection both use the primary transaction.
    let conditions = crate::write::PreparedTransactionConditions::prepare(
        state,
        &sorted_entries,
        prepared_writes,
    )
    .await?;
    let mut planned_metadata = None;
    let conditional_snapshots = if conditions.is_empty()
        && !sorted_entries
            .iter()
            .any(|(_, entry)| transaction_write_phase(entry).is_some())
    {
        HashMap::new()
    } else {
        let mut preview = state
            .storage
            .begin_transaction()
            .await
            .map_err(map_storage_error)?;
        if !conditions.is_empty() {
            preview
                .lock_conditional_writes()
                .await
                .map_err(map_storage_error)?;
        }
        planned_metadata = Some(
            preview
                .reserve_write_metadata(None)
                .await
                .map_err(map_storage_error)?,
        );
        let matches = if conditions.is_empty() {
            Ok(HashMap::new())
        } else {
            select_transaction_conditions_phased(
                state,
                &*preview,
                &conditions,
                prepared_writes,
                &sorted_entries,
                planned_metadata.as_ref(),
            )
            .await
        };
        preview.rollback().await.map_err(map_storage_error)?;
        matches.map_err(map_transaction_write_error)?
    };
    let conditional_matches = transaction_conditional_matches(conditional_snapshots.clone());

    // Policies for every ambiguous match were checked under the preview gate.
    // Fail before treating an unresolved DELETE as an empty policy context.
    if conditional_matches.iter().any(|(index, matched)| {
        matches!(matched, ConditionalMatch::Many)
            && !entries[*index]["request"]["method"]
                .as_str()
                .is_some_and(|m| m.eq_ignore_ascii_case("PATCH"))
    }) {
        return Err(ApiError::precondition_failed(
            "Multiple resources match the conditional criteria",
        ));
    }

    // Phase 1: Pre-scan — build complete reference map before creating anything,
    // using the conditional matches resolved above so entry processing never
    // has to repeat a search.
    let mut reference_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut pre_assigned_ids: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut matched_conditional: std::collections::HashMap<usize, Arc<MatchedExisting>> =
        std::collections::HashMap::new();

    for (original_idx, entry) in &sorted_entries {
        let method = entry["request"]["method"].as_str().unwrap_or("");
        let full_url = entry["fullUrl"].as_str();
        let url = entry["request"]["url"].as_str().unwrap_or("");
        let matched = conditional_matches.get(original_idx);
        let resource_type = url.split('?').next().unwrap_or(url);

        match method.to_uppercase().as_str() {
            "POST" => match matched {
                Some(ConditionalMatch::One(existing)) => {
                    if let Some(fu) = full_url {
                        reference_map
                            .insert(fu.to_string(), format!("{}/{}", resource_type, existing.id));
                    }
                    matched_conditional.insert(*original_idx, existing.clone());
                }
                Some(ConditionalMatch::Many) => {
                    return Err(ApiError::precondition_failed(
                        "Multiple matches for conditional create",
                    ));
                }
                // Either an unconditional create, or a conditional create that
                // matched nothing — both create a new resource.
                _ => {
                    if let Some(fu) = full_url {
                        let new_id = entry["resource"]["id"]
                            .as_str()
                            .expect("planned POST id")
                            .to_owned();
                        reference_map
                            .insert(fu.to_string(), format!("{}/{}", resource_type, new_id));
                        pre_assigned_ids.insert(fu.to_string(), new_id);
                    }
                }
            },
            "PUT" | "PATCH" => match matched {
                Some(ConditionalMatch::One(existing)) => {
                    if method.eq_ignore_ascii_case("PUT")
                        && let Some(id) = entry["resource"]["id"].as_str()
                        && id != existing.id
                    {
                        return Err(ApiError::bad_request(
                            "Conditional PUT body id does not match the selected resource",
                        ));
                    }
                    if let Some(fu) = full_url {
                        reference_map
                            .insert(fu.to_string(), format!("{}/{}", resource_type, existing.id));
                    }
                }
                Some(ConditionalMatch::Many) => {
                    return Err(ApiError::precondition_failed(
                        "Multiple matches for conditional update",
                    ));
                }
                Some(ConditionalMatch::NoMatch) => {
                    if method.eq_ignore_ascii_case("PATCH") {
                        return Err(ApiError::not_found(
                            "No resource matches the conditional criteria",
                        ));
                    }
                    // A conditional update that matches nothing creates a new
                    // resource, so pre-assign its id like a create does —
                    // otherwise references to this entry's fullUrl would be
                    // rewritten to the search url.
                    if let Some(fu) = full_url {
                        let new_id = entry["resource"]["id"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(octofhir_core::generate_id);
                        reference_map
                            .insert(fu.to_string(), format!("{}/{}", resource_type, new_id));
                        pre_assigned_ids.insert(fu.to_string(), new_id);
                    }
                }
                // Plain `PUT Type/id` — the id is already in the url.
                None => {
                    if let Some(fu) = full_url {
                        reference_map.insert(fu.to_string(), url.to_string());
                    }
                }
            },
            _ => {}
        }
    }

    // An identity may be modified only once, including identities resolved by
    // conditional requests. Otherwise one txid can describe different bodies.
    let mut identities = HashSet::new();
    for (original_idx, entry) in &sorted_entries {
        let method = entry["request"]["method"]
            .as_str()
            .unwrap_or("")
            .to_uppercase();
        if !matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
            continue;
        }
        let url = entry["request"]["url"].as_str().unwrap_or("");
        let resource_type = url.split(['?', '/']).next().unwrap_or("");
        let matched = conditional_matches.get(original_idx);
        let identity = if let Some(ConditionalMatch::One(existing)) = matched {
            Some(format!("{resource_type}/{}", existing.id))
        } else if method != "POST" && !url.contains('?') && url.split('/').count() == 2 {
            Some(url.to_string())
        } else {
            entry["fullUrl"]
                .as_str()
                .and_then(|full_url| reference_map.get(full_url))
                .cloned()
                .or_else(|| {
                    entry["resource"]["id"]
                        .as_str()
                        .map(|id| format!("{resource_type}/{id}"))
                })
        };
        if let Some(identity) = identity
            && !identities.insert(identity.clone())
        {
            return Err(ApiError::bad_request(format!(
                "Duplicate resource identity in transaction: {identity}"
            )));
        }
    }

    let mut reference_versions = HashMap::new();
    for (index, entry) in &sorted_entries {
        if let Some(full_url) = entry["fullUrl"].as_str() {
            let version = matched_conditional
                .get(index)
                .map(|row| row.version_id.as_str())
                .or_else(|| {
                    planned_metadata
                        .as_ref()
                        .map(|metadata| metadata.version_id.as_str())
                });
            if let Some(version) = version {
                reference_versions.insert(full_url.to_owned(), version.to_owned());
            }
        }
    }
    let mut uri_remapper = crate::bundle_references::BundleReferenceRemapper::default();

    // Phase 2: Build resolved entries — replace all urn:uuid references using the complete map
    let mut resolved_entries: Vec<(usize, Value)> = Vec::new();
    let mut entry_references: HashMap<usize, Vec<String>> = HashMap::new();
    // Conditional writes re-read the original entries per phase below; every
    // other bundle hands its entries over instead of copying them.
    let keep_originals = !conditions.is_empty();
    let order: Vec<usize> = sorted_entries.iter().map(|(index, _)| *index).collect();
    let request_methods: Vec<String> = planned_entries
        .iter()
        .map(|entry| {
            entry["request"]["method"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_uppercase()
        })
        .collect();
    for original_idx in &order {
        let mut resolved_entry = if keep_originals {
            planned_entries[*original_idx].clone()
        } else {
            planned_entries[*original_idx].take()
        };
        let method = resolved_entry["request"]["method"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_uppercase();

        // Inject pre-assigned ID into POST resources
        if let Some(new_id) = resolved_entry["fullUrl"]
            .as_str()
            .and_then(|fu| pre_assigned_ids.get(fu))
            .cloned()
            && let Some(res) = resolved_entry.get_mut("resource")
        {
            res["id"] = json!(new_id);
        }

        if method == "PUT"
            && entry_conditional_query(&resolved_entry).is_some()
            && let Some(resource) = resolved_entry.get_mut("resource")
        {
            match conditional_matches.get(original_idx) {
                Some(ConditionalMatch::One(existing)) => {
                    resource["id"] = json!(existing.id);
                }
                Some(ConditionalMatch::NoMatch) if resource["id"].as_str().is_none() => {
                    resource["id"] = json!(octofhir_core::generate_id());
                }
                _ => {}
            }
        }

        // Resolve all urn:uuid references in the resource
        if !matched_conditional.contains_key(original_idx)
            && let Some(res) = resolved_entry.get_mut("resource")
        {
            let references = uri_remapper
                .resolve(
                    state.model_provider.as_ref(),
                    res,
                    &reference_map,
                    &reference_versions,
                )
                .await?;
            entry_references.insert(*original_idx, references);
            if method == "POST" || method == "PUT" {
                let resource_type = res["resourceType"].as_str().unwrap_or_default().to_owned();
                preprocess_payload(&resource_type, res).await?;
            }
        }

        resolved_entries.push((*original_idx, resolved_entry));
    }

    for (index, entry) in &resolved_entries {
        if entry_conditional_query(entry).is_some() {
            let id = match conditional_matches.get(index) {
                Some(ConditionalMatch::One(existing)) => Some(existing.id.as_str()),
                _ if entry["request"]["method"]
                    .as_str()
                    .is_some_and(|method| method.eq_ignore_ascii_case("PUT")) =>
                {
                    entry["resource"]["id"].as_str()
                }
                _ => None,
            };
            prepared_writes.bind_target(*index, id);
        }
    }

    // Patch evaluation and validation may read through the connection pool.
    // Prepare before taking the transaction connection, then enforce the exact
    // snapshot version during the write so validation cannot become stale.
    let mut prepared_patches = std::collections::HashMap::new();
    for (original_idx, entry) in &resolved_entries {
        if !entry["request"]["method"]
            .as_str()
            .is_some_and(|m| m.eq_ignore_ascii_case("PATCH"))
        {
            continue;
        }
        let url = entry["request"]["url"].as_str().unwrap_or("");
        let resource_type = url.split(['/', '?']).next().unwrap_or_default();
        let previews = if let Some(rows) = conditional_snapshots.get(original_idx) {
            if rows.is_empty() {
                return Err(ApiError::not_found(
                    "No resource matches the conditional criteria",
                ));
            }
            rows.clone()
        } else {
            let (_, id) = batchable_target(url).ok_or_else(|| {
                ApiError::bad_request("Transaction PATCH requires Type/id or Type?criteria")
            })?;
            let existing = if let Some(write) = prepared_writes.get(*original_idx) {
                write
                    .snapshot(state)
                    .await
                    .map_err(map_transaction_write_error)?
            } else {
                state
                    .storage
                    .read(resource_type, id)
                    .await
                    .map_err(map_storage_error)?
                    .ok_or_else(|| ApiError::not_found(format!("{resource_type}/{id} not found")))?
            };
            Arc::new(vec![existing])
        };
        let patch = entry
            .get("resource")
            .ok_or_else(|| ApiError::bad_request("PATCH entry requires a resource"))?;
        let provider: SharedModelProvider = state.model_provider.clone();
        let mut patched_rows = Vec::new();
        for existing in previews.iter() {
            let patched = crate::patch::apply_bundle_patch(
                &state.fhirpath_engine,
                &provider,
                &existing.resource,
                patch,
            )
            .await;
            if let Some(write) = prepared_writes.get(*original_idx) {
                write
                    .resolved(Some(&existing.id))
                    .authorize_snapshot(state, existing, patched.as_ref().ok())
                    .await
                    .map_err(map_transaction_write_error)?;
            }
            patched_rows.push((existing, patched));
        }
        if patched_rows.len() > 1 {
            return Err(ApiError::precondition_failed(
                "Multiple resources match the conditional criteria",
            ));
        }
        let (existing, patched) = patched_rows.pop().expect("nonempty patch preview");
        if let Some(expected) = parse_bundle_if_match(&entry["request"])?
            && expected != existing.version_id
        {
            return Err(ApiError::precondition_failed(
                "Patch If-Match does not match the current version",
            ));
        }
        let mut patched = patched?;
        validate_payload_structure(
            resource_type,
            &patched,
            &IdPolicy::Update {
                path_id: existing.id.clone(),
            },
        )
        .map_err(ApiError::bad_request)?;
        preprocess_payload(resource_type, &mut patched).await?;
        prepared_patches.insert(*original_idx, (patched, existing.version_id.clone()));
    }

    for (body, _) in prepared_patches.values_mut() {
        let _ = uri_remapper
            .resolve(
                state.model_provider.as_ref(),
                body,
                &reference_map,
                &reference_versions,
            )
            .await?;
    }

    // Resolve against the proposed final graph, including replacements,
    // deletions and evaluated patches. Matched POST content is ignored by R4.
    let mut conditional_refs = HashSet::new();
    let mut conditional_rows: HashSet<(String, String)> = HashSet::new();
    for (index, entry) in &resolved_entries {
        if matched_conditional.contains_key(index) {
            continue;
        }
        let body = prepared_patches
            .get(index)
            .map(|(body, _)| body)
            .or_else(|| {
                entry["request"]["method"]
                    .as_str()
                    .filter(|method| {
                        method.eq_ignore_ascii_case("POST") || method.eq_ignore_ascii_case("PUT")
                    })
                    .and_then(|_| entry.get("resource"))
            });
        if let Some(body) = body {
            let collected = match entry_references.remove(index) {
                Some(references) if !prepared_patches.contains_key(index) => references,
                _ => {
                    uri_remapper
                        .references(state.model_provider.as_ref(), body)
                        .await?
                }
            };
            let mut any = false;
            for reference in collected {
                if is_conditional_reference(&reference) {
                    any = true;
                    conditional_refs.insert(reference);
                }
            }
            if any {
                conditional_rows.insert((
                    body["resourceType"].as_str().unwrap_or_default().to_owned(),
                    body["id"].as_str().unwrap_or_default().to_owned(),
                ));
            }
        }
    }
    let conditional_ref_predicates =
        prepare_conditional_reference_predicates(state, &conditional_refs).await?;
    let reference_predicates: Vec<_> = conditional_ref_predicates
        .iter()
        .map(|(_, rt, params)| (rt.clone(), params.clone()))
        .collect();
    let mut conditional_ref_candidates = Vec::new();
    let mut conditional_ref_dependent = false;
    let mut conditional_ref_templates = None;
    if !conditional_refs.is_empty() {
        // Only rows that carry conditional references (for their paths) and
        // rows of a predicate's type (as proposed matches) take part, unless the
        // predicates turn out to depend on the bundle's own references.
        let predicate_types: HashSet<&str> = reference_predicates
            .iter()
            .map(|(resource_type, _)| resource_type.as_str())
            .collect();
        let rows = proposed_transaction_graph(
            &resolved_entries,
            &conditional_matches,
            &matched_conditional,
            &prepared_patches,
            &|resource_type: &str, _: &str| predicate_types.contains(resource_type),
        )?;
        let mut path_sources = Vec::new();
        for (index, entry) in &resolved_entries {
            let body = match prepared_patches.get(index) {
                Some((body, _)) => Some(body),
                None if matches!(request_methods[*index].as_str(), "POST" | "PUT")
                    && !matched_conditional.contains_key(index) =>
                {
                    entry.get("resource")
                }
                None => None,
            };
            if let Some(body) = body {
                let resource_type = body["resourceType"].as_str().unwrap_or_default();
                let id = body["id"].as_str().unwrap_or_default();
                if conditional_rows.contains(&(resource_type.to_owned(), id.to_owned())) {
                    path_sources.push((resource_type, id, body));
                }
            }
        }
        let mut graph = ConditionalReferenceGraph::prepare(
            rows,
            path_sources,
            &mut uri_remapper,
            state.model_provider.as_ref(),
        )
        .await?;
        (conditional_ref_candidates, conditional_ref_dependent) =
            conditional_reference_candidate_predicates(state, &reference_predicates, &graph);
        if conditional_ref_dependent {
            graph.resources = proposed_transaction_graph(
                &resolved_entries,
                &conditional_matches,
                &matched_conditional,
                &prepared_patches,
                &|_: &str, _: &str| true,
            )?;
        }

        let resolved = resolve_conditional_references_batched(
            state,
            &conditional_ref_predicates,
            &conditional_ref_candidates,
            conditional_ref_dependent,
            &graph,
            &mut planned_metadata,
        )
        .await?;
        if let Some(metadata) = &planned_metadata {
            for row in &mut graph.resources {
                if let Some(body) = &mut row.resource {
                    apply_planned_metadata(body, metadata)?;
                }
            }
        }
        for (index, entry) in &mut resolved_entries {
            if !matched_conditional.contains_key(index)
                && entry["request"]["method"].as_str().is_some_and(|m| {
                    m.eq_ignore_ascii_case("POST") || m.eq_ignore_ascii_case("PUT")
                })
                && let Some(body) = entry.get_mut("resource")
            {
                graph.bind_resource(body, &resolved)?;
            }
        }
        for (patched, _) in prepared_patches.values_mut() {
            graph.bind_resource(patched, &resolved)?;
        }
        conditional_ref_templates = Some(graph);
        reference_map.extend(resolved);
    }

    if let Some(metadata) = &planned_metadata {
        for (index, entry) in &mut resolved_entries {
            if !matched_conditional.contains_key(index)
                && entry["request"]["method"].as_str().is_some_and(|m| {
                    m.eq_ignore_ascii_case("POST") || m.eq_ignore_ascii_case("PUT")
                })
                && let Some(body) = entry.get_mut("resource")
            {
                apply_planned_metadata(body, metadata)?;
            }
        }
        for (body, _) in prepared_patches.values_mut() {
            apply_planned_metadata(body, metadata)?;
        }
    }

    let mut candidates = HashMap::new();
    for (index, entry) in &resolved_entries {
        let resource = if let Some(matched) = matched_conditional.get(index) {
            Some(&matched.resource)
        } else if let Some((patched, _)) = prepared_patches.get(index) {
            Some(patched)
        } else if entry["request"]["method"].as_str().is_some_and(|method| {
            method.eq_ignore_ascii_case("POST") || method.eq_ignore_ascii_case("PUT")
        }) {
            entry.get("resource")
        } else {
            None
        };
        if let Some(resource) = resource {
            let resource_type = resource["resourceType"].as_str().unwrap_or_default();
            let id = resource["id"].as_str().unwrap_or_default();
            let snapshot = Arc::new(resource.clone());
            if let Some(version) = resource["meta"]["versionId"].as_str() {
                candidates.insert(
                    format!("{resource_type}/{id}/_history/{version}"),
                    snapshot.clone(),
                );
            }
            candidates.insert(format!("{resource_type}/{id}"), snapshot);
        }
    }
    let candidates = Arc::new(candidates);
    let mut validation_resources = Vec::new();
    if !skip_validation {
        for (index, entry) in &resolved_entries {
            if matched_conditional.contains_key(index) {
                continue;
            }
            let resource = if let Some((patched, _)) = prepared_patches.get(index) {
                Some(patched)
            } else if entry["request"]["method"].as_str().is_some_and(|method| {
                method.eq_ignore_ascii_case("POST") || method.eq_ignore_ascii_case("PUT")
            }) {
                entry.get("resource")
            } else {
                None
            };
            if let Some(resource) = resource {
                validation_resources.push(resource);
            }
        }
    }

    let reference_checks = if skip_validation {
        LocalReferenceSet::default()
    } else {
        validate_bundle_writes(state, &validation_resources, candidates).await?
    };

    // Phase 3: Execute all entries in one database transaction.
    //
    // Entries whose SQL differs only by the row it touches are gathered into
    // per-resource-type batches and issued as a single set-based statement:
    // `create_batch` (INSERT ... FROM UNNEST), `update_batch` (UPDATE ... FROM
    // UNNEST) and `delete_batch` (DELETE ... WHERE id = ANY). What cannot join a
    // batch — conditional urls, `If-Match`, PATCH, GET — falls
    // through to the per-entry path.
    //
    // Batches flush in the same DELETE → POST → PUT order that
    // `transaction_method_order` sorts entries into, so a bundle that deletes a
    // row and then rewrites it still applies in that order.
    tracing::debug!("Beginning native PostgreSQL transaction for Bundle processing");
    let mut tx = state
        .storage
        .begin_transaction()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to begin transaction: {}", e)))?;

    // Conditional writes (and reference graphs that depend on this bundle's
    // own resources) need the exclusive gate so their search and write are
    // atomic. Plain conditional references to existing resources do not: they
    // are re-checked below, and serializing every Synthea-style bundle on a
    // database-wide lock would turn concurrent imports into a queue.
    if !conditional_matches.is_empty() || conditional_ref_dependent {
        // Acquire before any row lock. This avoids shared-to-exclusive gate
        // upgrades and pins chained predicates while the graph is committed.
        tx.lock_conditional_writes()
            .await
            .map_err(map_storage_error)?;
    }

    if let Some(metadata) = &planned_metadata {
        tx.reserve_write_metadata(Some(metadata))
            .await
            .map_err(map_storage_error)?;
    }
    let mut previous_entries = HashSet::new();
    let include_write_resource = include_resource || !prepared_writes.is_empty();
    let mut response_entries: Vec<Option<Value>> = vec![None; resolved_entries.len()];
    let mut post_batches: std::collections::HashMap<String, Vec<(usize, Value)>> =
        std::collections::HashMap::new();
    let mut put_batches: std::collections::HashMap<String, Vec<(usize, Value)>> =
        std::collections::HashMap::new();
    let mut delete_batches: std::collections::HashMap<String, Vec<(usize, String)>> =
        std::collections::HashMap::new();

    for (original_idx, entry) in &resolved_entries {
        let request = &entry["request"];
        let method = request["method"].as_str().unwrap_or("").to_uppercase();
        let url = request["url"].as_str().unwrap_or("");

        match method.as_str() {
            "POST" => {
                // Conditional create that matched in pre-scan — return existing
                if let Some(matched) = matched_conditional.get(original_idx) {
                    response_entries[*original_idx] =
                        Some(build_matched_transaction_response_entry(
                            matched,
                            "200 OK",
                            include_write_resource,
                        ));
                    continue;
                }

                let resource_type = url.split('?').next().unwrap_or(url).to_string();

                resource_type.parse::<ResourceType>().map_err(|_| {
                    ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
                })?;

                let mut resource = entry
                    .get("resource")
                    .cloned()
                    .ok_or_else(|| ApiError::bad_request("POST entry requires a resource"))?;
                resource["resourceType"] = json!(resource_type);

                post_batches
                    .entry(resource_type)
                    .or_default()
                    .push((*original_idx, resource));
            }
            "PUT" => {
                let Some((resource_type, id)) = batchable_target(url) else {
                    continue; // conditional url — per-entry path
                };
                // `If-Match` needs a per-resource version check, and the admin
                // access policy is guarded individually.
                if request.get("ifMatch").is_some() || resource_type == "AccessPolicy" {
                    continue;
                }
                if resource_type.parse::<ResourceType>().is_err() {
                    continue; // let the per-entry path report the bad type
                }
                let Some(mut resource) = entry.get("resource").cloned() else {
                    continue;
                };
                let slot = put_batches.entry(resource_type.to_string()).or_default();
                // A set-based UPDATE touches each row once, so a second write to
                // the same id must stay on the per-entry path — which runs after
                // the batches and therefore still lands last.
                if slot.iter().any(|(_, r)| r["id"].as_str() == Some(id)) {
                    continue;
                }
                resource["id"] = json!(id);
                resource["resourceType"] = json!(resource_type);
                slot.push((*original_idx, resource));
            }
            "DELETE" => {
                if request.get("ifMatch").is_some() {
                    continue;
                }
                let Some((resource_type, id)) = batchable_target(url) else {
                    continue; // conditional url — per-entry path
                };
                if resource_type == "AccessPolicy" {
                    continue;
                }
                if resource_type.parse::<ResourceType>().is_err() {
                    continue;
                }
                delete_batches
                    .entry(resource_type.to_string())
                    .or_default()
                    .push((*original_idx, id.to_string()));
            }
            _ => {}
        }
    }

    // Flush set-based and individual writes within each verb phase. An
    // If-Match or conditional DELETE must execute before every POST/PUT.
    for phase in 0..=6 {
        if phase <= 2 && !conditions.is_empty() {
            let sorted_entries = sort_transaction_entries(&planned_entries);
            let phase_entries: Vec<_> = sorted_entries
                .iter()
                .copied()
                .filter(|(_, entry)| transaction_write_phase(entry) == Some(phase))
                .collect();
            let current = transaction_conditional_matches(
                conditions
                    .select(state, &*tx, prepared_writes, &phase_entries, &[])
                    .await
                    .map_err(map_transaction_write_error)?,
            );
            if current
                .iter()
                .any(|(index, matched)| conditional_matches.get(index) != Some(matched))
            {
                return Err(ApiError::conflict(
                    "Conditional matching changed during validation; retry the transaction",
                ));
            }
        }
        if phase <= 2 {
            let phase_entries: Vec<_> = resolved_entries
                .iter()
                .filter(|(_, entry)| transaction_write_phase(entry) == Some(phase))
                .cloned()
                .collect();
            previous_entries.extend(
                prepared_writes
                    .authorize_previous(state, &*tx, &phase_entries, &prepared_patches)
                    .await
                    .map_err(map_transaction_write_error)?,
            );
        }
        if phase == 0 {
            // Flush all DELETE batches.
            for (resource_type, items) in std::mem::take(&mut delete_batches) {
                let mut ids: Vec<String> = items
                    .iter()
                    .filter(|(index, _)| {
                        prepared_writes.get(*index).is_none() || previous_entries.contains(index)
                    })
                    .map(|(_, id)| id.clone())
                    .collect();
                ids.sort();
                ids.dedup();
                let deleted = tx
                    .delete_batch_with_snapshots(&resource_type, &ids)
                    .await
                    .map_err(map_storage_error)?;
                let deleted: HashMap<_, _> = deleted
                    .iter()
                    .map(|row| (row.previous.id.as_str(), row))
                    .collect();
                for (idx, id) in &items {
                    response_entries[*idx] = Some(build_deletion_response_entry(
                        &resource_type,
                        Some(id),
                        deleted.get(id.as_str()).copied(),
                    )?);
                }
            }
        }
        if phase == 1 {
            // Flush all POST batches.
            for (resource_type, items) in std::mem::take(&mut post_batches) {
                let index_of: HashMap<String, usize> = items
                    .iter()
                    .filter_map(|(index, body)| {
                        body["id"].as_str().map(|id| (id.to_owned(), *index))
                    })
                    .collect();
                let resources: Vec<Value> = items.into_iter().map(|(_, r)| r).collect();
                let stored = tx
                    .create_batch(&resource_type, resources)
                    .await
                    .map_err(|e| {
                        tracing::warn!(
                            "Transaction batch-create failed, will auto-rollback: {}",
                            e
                        );
                        map_storage_error(e)
                    })?;
                for s in stored {
                    let index = index_of
                        .get(s.id.as_str())
                        .ok_or_else(|| ApiError::conflict("The stored create identity changed"))?;
                    response_entries[*index] = Some(build_owned_transaction_response_entry(
                        s,
                        "201 Created",
                        include_write_resource,
                    ));
                }
            }
        }
        if phase == 2 {
            // Flush all PUT batches. Ids that matched no row are re-issued as a single
            // create batch — FHIR update-as-create.
            for (resource_type, items) in std::mem::take(&mut put_batches) {
                let resources: Vec<Value> = items
                    .iter()
                    .filter(|(index, _)| {
                        prepared_writes.get(*index).is_none() || previous_entries.contains(index)
                    })
                    .map(|(_, r)| r.clone())
                    .collect();
                let outcome = if resources.is_empty() {
                    octofhir_storage::BatchUpdateOutcome::default()
                } else {
                    tx.update_batch(&resource_type, &resources)
                        .await
                        .map_err(|e| {
                            tracing::warn!(
                                "Transaction batch-update failed, will auto-rollback: {}",
                                e
                            );
                            map_storage_error(e)
                        })?
                };

                let index_of: std::collections::HashMap<&str, usize> = items
                    .iter()
                    .filter_map(|(idx, r)| r["id"].as_str().map(|id| (id, *idx)))
                    .collect();

                for s in &outcome.updated {
                    let Some(idx) = index_of.get(s.id.as_str()) else {
                        continue;
                    };
                    response_entries[*idx] = Some(build_stored_transaction_response_entry(
                        s,
                        "200 OK",
                        include_write_resource,
                    ));
                }

                let missing: std::collections::HashSet<&str> =
                    outcome.missing.iter().map(String::as_str).collect();
                let to_create: Vec<Value> = items
                    .iter()
                    .filter(|(index, r)| {
                        (prepared_writes.get(*index).is_some() && !previous_entries.contains(index))
                            || r["id"].as_str().is_some_and(|id| missing.contains(id))
                    })
                    .map(|(_, r)| r.clone())
                    .collect();
                if to_create.is_empty() {
                    continue;
                }
                let created = tx
                    .create_batch(&resource_type, to_create)
                    .await
                    .map_err(|e| {
                        tracing::warn!(
                            "Transaction batch update-as-create failed, will auto-rollback: {}",
                            e
                        );
                        map_storage_error(e)
                    })?;
                for s in &created {
                    let Some(idx) = index_of.get(s.id.as_str()) else {
                        continue;
                    };
                    response_entries[*idx] = Some(build_stored_transaction_response_entry(
                        s,
                        "201 Created",
                        include_write_resource,
                    ));
                }
            }
        }
        // Process whatever could not be batched (conditional urls, `If-Match`,
        // PATCH and GET) individually.
        for (original_idx, entry) in &resolved_entries {
            if entry["request"]["method"]
                .as_str()
                .map(transaction_method_order)
                != Some(phase)
                || response_entries[*original_idx].is_some()
            {
                continue;
            }
            let pre_resolved = conditional_matches.get(original_idx);
            match process_transaction_entry_with_tx(
                &mut *tx,
                state,
                entry,
                TransactionEntryOptions {
                    prepared_read: prepared_reads.get(original_idx),
                    include_resource: include_write_resource,
                    conditional_params: conditions.params(*original_idx),
                    authorized_previous: prepared_writes
                        .get(*original_idx)
                        .map(|_| previous_entries.contains(original_idx)),
                },
                pre_resolved,
                prepared_patches.get(original_idx),
                prepared_searches.get(original_idx),
            )
            .await
            {
                Ok(response_entry) => response_entries[*original_idx] = Some(response_entry),
                Err(e) => {
                    tracing::warn!("Transaction failed, will auto-rollback: {}", e);
                    return Err(e);
                }
            }
        }
    }

    let mut response_entries: Vec<Value> = response_entries
        .into_iter()
        .map(|opt| {
            opt.unwrap_or_else(|| json!({"response": {"status": "500 Internal Server Error"}}))
        })
        .collect();
    for entry in &mut response_entries {
        rebase_bundle_response_urls(state, entry);
    }

    prepared_writes
        .authorize_results(
            state,
            &*tx,
            &resolved_entries,
            &prepared_patches,
            &response_entries,
            &previous_entries,
        )
        .await
        .map_err(map_transaction_write_error)?;
    if !include_resource {
        for (index, entry) in response_entries.iter_mut().enumerate() {
            if matches!(request_methods[index].as_str(), "POST" | "PUT" | "PATCH") {
                entry
                    .as_object_mut()
                    .expect("response entry")
                    .remove("resource");
            }
        }
    }

    lock_bundle_references(&mut *tx, &reference_checks).await?;
    if conditional_ref_dependent {
        let templates = conditional_ref_templates
            .as_ref()
            .ok_or_else(|| ApiError::internal("Missing conditional reference templates"))?;
        // Confirm global uniqueness under the actual commit's exclusive write
        // gate as well: a valid preview alone cannot protect concurrent changes.
        let actual = solve_conditional_reference_graph(
            &*tx,
            &conditional_ref_predicates,
            &reference_predicates,
            &conditional_ref_candidates,
            templates,
            &state.config.server,
        )
        .await?;
        if actual
            .iter()
            .any(|(condition, target)| reference_map.get(condition) != Some(target))
        {
            return Err(ApiError::conflict(
                "Conditional reference graph changed during validation; retry the transaction",
            ));
        }
    }
    if !conditional_ref_predicates.is_empty() {
        let predicates: Vec<_> = conditional_ref_predicates
            .iter()
            .map(|(_, resource_type, params)| (resource_type.clone(), params.clone()))
            .collect();
        let matches = tx
            .search_conditional_batch(&predicates)
            .await
            .map_err(map_storage_error)?;
        if matches.len() != predicates.len() {
            return Err(ApiError::internal(
                "Incomplete conditional reference results",
            ));
        }
        for ((condition, resource_type, _), rows) in conditional_ref_predicates.iter().zip(matches)
        {
            if rows.len() != 1
                || reference_map.get(condition)
                    != rows
                        .first()
                        .map(|row| format!("{resource_type}/{}", row.id))
                        .as_ref()
            {
                return Err(ApiError::conflict(
                    "Conditional reference matching changed during validation; retry the transaction",
                ));
            }
        }
    }

    let response_bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction-response",
        "entry": response_entries,
    });
    crate::async_jobs::save_transaction_result(
        &mut *tx,
        &response_bundle,
        state.config.search.include_limits.max_bytes,
    )
    .await?;

    // Resource writes and the async outcome commit together.
    tx.commit()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to commit transaction: {}", e)))?;

    tracing::debug!(
        "Transaction committed successfully with {} entries",
        response_entries.len()
    );

    // Drop every row this bundle touched from the read cache. Single-resource
    // writes do this via `postprocess_resource`; without the same step here a
    // read that ran before the bundle would keep serving the pre-bundle version
    // until the entry's TTL expired.
    if let Some(cache) = &state.resource_cache {
        for (index, entry) in response_entries.iter().enumerate() {
            if !matches!(
                request_methods[index].as_str(),
                "POST" | "PUT" | "PATCH" | "DELETE"
            ) || matched_conditional.contains_key(&index)
            {
                continue;
            }
            if let Some(reference) = entry["fullUrl"].as_str().and_then(|url| {
                octofhir_core::parse_reference(url, Some(&state.fhir_base_url())).ok()
            }) {
                cache
                    .invalidate(&reference.resource_type, &reference.id)
                    .await;
            }
        }
    }

    Ok((StatusCode::OK, Json(response_bundle)))
}

async fn lock_bundle_references(
    tx: &mut dyn octofhir_storage::Transaction,
    references: &LocalReferenceSet,
) -> Result<(), ApiError> {
    let mut found = if references.groups.is_empty() {
        HashSet::new()
    } else {
        tx.lock_references(&references.groups)
            .await
            .map_err(map_storage_error)?
    };
    let mut targets = references.versions.clone();
    targets.extend(
        references
            .snapshots
            .iter()
            .map(|(reference, _)| reference.clone()),
    );
    targets.sort_by_key(octofhir_core::FhirReference::to_versioned);
    targets.dedup();
    let snapshots = tx
        .lock_reference_snapshots(&targets)
        .await
        .map_err(map_storage_error)?;
    found.extend(
        references
            .versions
            .iter()
            .map(octofhir_core::FhirReference::to_versioned)
            .filter(|key| snapshots.contains_key(key)),
    );
    if found != references.all {
        return Err(ApiError::unprocessable_entity(
            "A transaction reference target is unavailable",
            None,
        ));
    }
    for (reference, expected) in &references.snapshots {
        if snapshots.get(&reference.to_versioned()) != expected.as_deref() {
            return Err(ApiError::conflict(
                "A reference target changed during validation; retry the transaction",
            ));
        }
    }
    Ok(())
}

/// Matching sees completed earlier verb phases; siblings in a phase share one
/// snapshot so the result cannot depend on their input order.
fn transaction_write_phase(entry: &Value) -> Option<u8> {
    match entry["request"]["method"]
        .as_str()?
        .to_ascii_uppercase()
        .as_str()
    {
        "DELETE" => Some(0),
        "POST" => Some(1),
        "PUT" | "PATCH" => Some(2),
        _ => None,
    }
}

async fn select_transaction_conditions_phased(
    state: &crate::server::AppState,
    tx: &dyn octofhir_storage::Transaction,
    conditions: &crate::write::PreparedTransactionConditions,
    writes: &crate::write::PreparedTransactionWrites,
    entries: &[(usize, &Value)],
    metadata: Option<&octofhir_storage::TransactionWriteMetadata>,
) -> Result<HashMap<usize, Arc<Vec<octofhir_storage::StoredResource>>>, crate::write::WriteError> {
    let mut selected = HashMap::new();
    let mut graph = Vec::new();
    let mut aliases = HashMap::new();
    for (_, entry) in entries {
        let url = entry["request"]["url"].as_str().unwrap_or_default();
        if let Some(full_url) = entry["fullUrl"].as_str() {
            if entry["request"]["method"]
                .as_str()
                .is_some_and(|m| m.eq_ignore_ascii_case("POST"))
            {
                if let Some(id) = entry["resource"]["id"].as_str() {
                    aliases.insert(
                        full_url.to_owned(),
                        format!("{}/{id}", url.split('?').next().unwrap_or(url)),
                    );
                }
            } else if batchable_target(url).is_some() {
                aliases.insert(full_url.to_owned(), url.to_owned());
            }
        }
    }
    for phase in 0..=2 {
        let phase_entries: Vec<_> = entries
            .iter()
            .copied()
            .filter(|(_, entry)| transaction_write_phase(entry) == Some(phase))
            .collect();
        let matches = conditions
            .select(state, tx, writes, &phase_entries, &graph)
            .await?;
        if matches.iter().any(|(index, rows)| {
            rows.len() > 1
                && !entries
                    .iter()
                    .find(|(entry_index, _)| entry_index == index)
                    .is_some_and(|(_, entry)| {
                        entry["request"]["method"]
                            .as_str()
                            .is_some_and(|m| m.eq_ignore_ascii_case("PATCH"))
                    })
        }) {
            return Err(ApiError::precondition_failed(
                "Multiple resources match the conditional criteria",
            )
            .into());
        }
        // Bind every POST alias before constructing the overlay. References
        // between sibling creates are independent of their order.
        for (index, entry) in &phase_entries {
            if phase == 1
                && let Some(row) = matches.get(index).and_then(|rows| rows.first())
                && let Some(full_url) = entry["fullUrl"].as_str()
            {
                aliases.insert(
                    full_url.to_owned(),
                    format!("{}/{}", row.resource_type, row.id),
                );
            }
        }
        if phase < 2 {
            for (index, entry) in &phase_entries {
                let url = entry["request"]["url"].as_str().unwrap_or_default();
                let resource_type = url.split(['?', '/']).next().unwrap_or_default();
                let matched = matches.get(index).and_then(|rows| rows.first());
                if phase == 1 && matched.is_some() {
                    continue;
                }
                let id = if phase == 0 {
                    matched
                        .map(|row| row.id.as_str())
                        .or_else(|| batchable_target(url).map(|(_, id)| id))
                } else {
                    entry["resource"]["id"].as_str()
                };
                if let Some(id) = id {
                    let mut body = (phase == 1).then(|| entry["resource"].clone());
                    if let Some(body) = &mut body {
                        replace_known_reference_aliases(body, &aliases);
                        if let Some(metadata) = metadata {
                            apply_planned_metadata(body, metadata)?;
                        }
                    }
                    graph.push(octofhir_storage::ProposedResource {
                        resource_type: resource_type.to_owned(),
                        id: id.to_owned(),
                        resource: body,
                    });
                }
            }
        }
        selected.extend(matches);
    }
    Ok(selected)
}

fn apply_planned_metadata(
    body: &mut Value,
    metadata: &octofhir_storage::TransactionWriteMetadata,
) -> Result<(), ApiError> {
    if body.get("meta").is_some_and(|meta| !meta.is_object()) {
        return Err(ApiError::unprocessable_entity(
            "Resource meta must be an object",
            None,
        ));
    }
    if body.get("meta").is_none() {
        body["meta"] = json!({});
    }
    body["meta"]["versionId"] = json!(metadata.version_id);
    body["meta"]["lastUpdated"] = json!(metadata.last_updated);
    Ok(())
}

fn replace_known_reference_aliases(value: &mut Value, aliases: &HashMap<String, String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key == "reference"
                    && let Some(target) = value.as_str().and_then(|s| aliases.get(s))
                {
                    *value = Value::String(target.clone());
                } else {
                    replace_known_reference_aliases(value, aliases);
                }
            }
        }
        Value::Array(array) => {
            for value in array {
                replace_known_reference_aliases(value, aliases);
            }
        }
        _ => {}
    }
}

/// Snapshot of an existing resource captured during conditional pre-scan, so we
/// don't re-issue the same search inside the transaction.
#[derive(Clone, PartialEq)]
struct MatchedExisting {
    id: String,
    version_id: String,
    resource: Value,
}

/// Outcome of resolving one `Type?query` condition against existing data.
#[derive(Clone, PartialEq)]
enum ConditionalMatch {
    /// Nothing matched. Conditional create/update fall back to creating;
    /// conditional delete is a no-op; a conditional reference is an error.
    NoMatch,
    /// Exactly one resource matched.
    One(Arc<MatchedExisting>),
    /// More than one matched. Every conditional operation rejects ambiguity,
    /// but each with its own message, so the caller raises the error.
    Many,
}

fn transaction_conditional_matches(
    selections: HashMap<usize, Arc<Vec<octofhir_storage::StoredResource>>>,
) -> HashMap<usize, ConditionalMatch> {
    let mut snapshots = HashMap::new();
    selections
        .into_iter()
        .map(|(index, rows)| {
            let matched = match rows.as_slice() {
                [] => ConditionalMatch::NoMatch,
                [row] => {
                    let key = (
                        row.resource_type.clone(),
                        row.id.clone(),
                        row.version_id.clone(),
                    );
                    let snapshot = snapshots.entry(key).or_insert_with(|| {
                        Arc::new(MatchedExisting {
                            id: row.id.clone(),
                            version_id: row.version_id.clone(),
                            resource: row.resource.clone(),
                        })
                    });
                    ConditionalMatch::One(snapshot.clone())
                }
                _ => ConditionalMatch::Many,
            };
            (index, matched)
        })
        .collect()
}

/// The `Type?query` key an entry resolves against, if it is a conditional
/// create (`ifNoneExist` or `POST Type?query`), update, or delete.
///
/// Conditional PUT/DELETE urls are already in `Type?query` form. The native
/// selector additionally keys this criterion by write permission; identical
/// client predicates cannot share results across c/u/d grants.
pub(crate) fn entry_conditional_query(entry: &Value) -> Option<String> {
    let request = &entry["request"];
    let method = request["method"].as_str()?.to_uppercase();
    let url = request["url"].as_str()?;

    match method.as_str() {
        "POST" => {
            let resource_type = url.split('?').next().unwrap_or(url);
            transaction_post_condition(request, url)
                .map(|condition| format!("{resource_type}?{condition}"))
        }
        "PUT" | "PATCH" | "DELETE" => url
            .split_once('?')
            .filter(|(head, query)| !head.is_empty() && !query.is_empty())
            .map(|_| url.to_string()),
        _ => None,
    }
}

/// Resolve the condition of a conditional update/delete to at most one match.
///
/// A pinned single preview is reused. A preview miss is rechecked with the same
/// prepared grants after earlier Bundle writes. Newly selected identities fail
/// closed until the full uncommitted-graph authorization protocol is implemented.
async fn resolve_entry_condition(
    tx: &mut dyn octofhir_storage::Transaction,
    resource_type: &str,
    condition: &str,
    pre_resolved: Option<&ConditionalMatch>,
    prepared_params: Option<&octofhir_storage::SearchParams>,
    ambiguous_message: &'static str,
) -> Result<Option<Arc<MatchedExisting>>, ApiError> {
    match pre_resolved {
        Some(ConditionalMatch::One(existing)) => return Ok(Some(existing.clone())),
        Some(ConditionalMatch::Many) => {
            return Err(ApiError::precondition_failed(ambiguous_message));
        }
        Some(ConditionalMatch::NoMatch) => return Ok(None),
        None => {}
    }

    let search_params = conditional_search_params(condition);
    // Terminology preparation can move trusted grants into type_constraints.
    // A type search must flatten that map before calling the ordinary converter.
    let search_params = prepared_params
        .unwrap_or(&search_params)
        .for_resource_type(resource_type);
    let result = tx
        .search(resource_type, &search_params)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    match result.entries.len() {
        0 => Ok(None),
        1 => {
            let existing = &result.entries[0];
            Ok(Some(Arc::new(MatchedExisting {
                id: existing.id.clone(),
                version_id: existing.version_id.clone(),
                resource: existing.resource.clone(),
            })))
        }
        _ => Err(ApiError::precondition_failed(ambiguous_message)),
    }
}

/// Splits a `ResourceType/id` request url, which is the only shape a PUT or
/// DELETE can take in a batch — the resource type picks the table and the id
/// becomes one element of the statement's array parameter.
///
/// Returns `None` for conditional (`Type?query`) and any other url shape.
fn batchable_target(url: &str) -> Option<(&str, &str)> {
    if url.contains('?') {
        return None;
    }
    let (resource_type, id) = url.split_once('/')?;
    if resource_type.is_empty() || id.is_empty() || id.contains('/') {
        return None;
    }
    Some((resource_type, id))
}

fn parse_bundle_if_match(request: &Value) -> Result<Option<String>, ApiError> {
    request
        .get("ifMatch")
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| ApiError::bad_request("request.ifMatch must be a string"))?;
            crate::write::parse_if_match(value)
        })
        .transpose()
}

async fn check_bundle_if_match(
    tx: &mut dyn octofhir_storage::Transaction,
    resource_type: &str,
    id: &str,
    expected: &str,
) -> Result<(), ApiError> {
    let previous = match tx.read_for_update(resource_type, id).await {
        Err(StorageError::Deleted { .. }) => None,
        result => result.map_err(map_storage_error)?,
    };
    if previous.as_ref().map(|row| row.version_id.as_str()) != Some(expected) {
        return Err(ApiError::precondition_failed(
            "Resource version does not match If-Match",
        ));
    }
    Ok(())
}

async fn upsert_transaction_resource(
    tx: &mut dyn octofhir_storage::Transaction,
    resource_type: &str,
    resource: &Value,
    if_match: Option<&str>,
) -> Result<(octofhir_storage::StoredResource, bool), ApiError> {
    let id = resource["id"].as_str();
    if resource_type == "AccessPolicy" && id == Some(ADMIN_ACCESS_POLICY_ID) {
        return Err(ApiError::forbidden(
            "The default admin access policy cannot be modified",
        ));
    }
    let previous = match id {
        Some(id) => tx
            .read_for_update(resource_type, id)
            .await
            .map_err(map_storage_error)?,
        None => None,
    };
    if let Some(expected) = if_match
        && previous.as_ref().map(|row| row.version_id.as_str()) != Some(expected)
    {
        return Err(ApiError::precondition_failed(
            "Resource version does not match If-Match",
        ));
    }
    match previous {
        Some(previous) => tx
            .update(resource, Some(&previous.version_id))
            .await
            .map(|stored| (stored, false))
            .map_err(map_storage_error),
        None => tx
            .create(resource)
            .await
            .map(|stored| (stored, true))
            .map_err(map_storage_error),
    }
}

fn transaction_post_condition<'a>(request: &'a Value, url: &'a str) -> Option<&'a str> {
    request["ifNoneExist"].as_str().or_else(|| {
        url.find('?')
            .map(|idx| &url[idx + 1..])
            .filter(|condition| !condition.is_empty())
    })
}

/// Process a single transaction entry using a transaction object.
///
/// Resources are expected to already have pre-assigned IDs and resolved references
/// (done in the pre-scan phase of process_transaction).
struct TransactionEntryOptions<'a> {
    prepared_read: Option<&'a crate::authorization::PreparedRead>,
    conditional_params: Option<&'a octofhir_storage::SearchParams>,
    include_resource: bool,
    /// Present only after the trusted group protocol locked and authorized the
    /// exact previous snapshot. A missing row must not become an unchecked update.
    authorized_previous: Option<bool>,
}

async fn process_transaction_entry_with_tx(
    tx: &mut dyn octofhir_storage::Transaction,
    state: &crate::server::AppState,
    entry: &Value,
    options: TransactionEntryOptions<'_>,
    pre_resolved: Option<&ConditionalMatch>,
    prepared_patch: Option<&(Value, String)>,
    prepared_search: Option<&PreparedBundleSearch>,
) -> Result<Value, ApiError> {
    let include_resource = options.include_resource;
    let request = &entry["request"];
    let method = request["method"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.method in bundle entry"))?;
    let url = request["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.url in bundle entry"))?;

    let resource = entry.get("resource").cloned();

    match method.to_uppercase().as_str() {
        "POST" => {
            let mut resource =
                resource.ok_or_else(|| ApiError::bad_request("POST entry requires a resource"))?;

            let resource_type = url.split('?').next().unwrap_or(url);

            let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
            })?;

            if let Some(condition) = transaction_post_condition(request, url)
                && let Some(existing) = resolve_entry_condition(
                    tx,
                    resource_type,
                    condition,
                    pre_resolved,
                    options.conditional_params,
                    "Multiple matches for conditional create",
                )
                .await?
            {
                return Ok(build_matched_transaction_response_entry(
                    &existing,
                    "200 OK",
                    include_resource,
                ));
            }

            resource["resourceType"] = json!(resource_type);
            let stored = tx.create(&resource).await.map_err(map_storage_error)?;

            Ok(build_stored_transaction_response_entry(
                &stored,
                "201 Created",
                include_resource,
            ))
        }
        "PUT" => {
            let mut resource =
                resource.ok_or_else(|| ApiError::bad_request("PUT entry requires a resource"))?;
            let if_match = parse_bundle_if_match(request)?;

            let parts: Vec<&str> = url.split('/').collect();

            if parts.len() == 2 && !parts[1].contains('?') {
                let resource_type = parts[0];
                let id = parts[1];

                let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                    ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
                })?;

                if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
                    return Err(ApiError::forbidden(
                        "The default admin access policy cannot be modified",
                    ));
                }

                resource["id"] = json!(id);
                resource["resourceType"] = json!(resource_type);

                if options.authorized_previous == Some(false) {
                    let stored = tx.create(&resource).await.map_err(map_storage_error)?;
                    return Ok(build_stored_transaction_response_entry(
                        &stored,
                        "201 Created",
                        include_resource,
                    ));
                }
                if let Some(expected) = &if_match
                    && options.authorized_previous.is_none()
                {
                    check_bundle_if_match(tx, resource_type, id, expected).await?;
                }

                match tx.update(&resource, if_match.as_deref()).await {
                    Ok(stored) => Ok(build_stored_transaction_response_entry(
                        &stored,
                        "200 OK",
                        include_resource,
                    )),
                    Err(StorageError::NotFound { .. }) => {
                        let stored = tx.create(&resource).await.map_err(map_storage_error)?;

                        Ok(build_stored_transaction_response_entry(
                            &stored,
                            "201 Created",
                            include_resource,
                        ))
                    }
                    Err(e) => Err(map_storage_error(e)),
                }
            } else if url.contains('?') {
                let (resource_type, condition) = url
                    .find('?')
                    .map(|idx| (&url[..idx], &url[idx + 1..]))
                    .ok_or_else(|| ApiError::bad_request(format!("Invalid PUT URL: {}", url)))?;

                let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                    ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
                })?;

                let matched = resolve_entry_condition(
                    tx,
                    resource_type,
                    condition,
                    pre_resolved,
                    options.conditional_params,
                    "Multiple matches for conditional update",
                )
                .await?;

                match matched {
                    None => {
                        resource["resourceType"] = json!(resource_type);
                        let (stored, created) = if options.authorized_previous == Some(false) {
                            (tx.create(&resource).await.map_err(map_storage_error)?, true)
                        } else {
                            upsert_transaction_resource(
                                tx,
                                resource_type,
                                &resource,
                                if_match.as_deref(),
                            )
                            .await?
                        };

                        Ok(build_stored_transaction_response_entry(
                            &stored,
                            if created { "201 Created" } else { "200 OK" },
                            include_resource,
                        ))
                    }
                    Some(existing) => {
                        if resource_type == "AccessPolicy" && existing.id == ADMIN_ACCESS_POLICY_ID
                        {
                            return Err(ApiError::forbidden(
                                "The default admin access policy cannot be modified",
                            ));
                        }

                        resource["id"] = json!(existing.id.clone());
                        resource["resourceType"] = json!(resource_type);

                        if let Some(expected) = &if_match {
                            check_bundle_if_match(tx, resource_type, &existing.id, expected)
                                .await?;
                        }

                        let stored = tx
                            .update(&resource, if_match.as_deref())
                            .await
                            .map_err(map_storage_error)?;

                        Ok(build_stored_transaction_response_entry(
                            &stored,
                            "200 OK",
                            include_resource,
                        ))
                    }
                }
            } else {
                Err(ApiError::bad_request(format!("Invalid PUT URL: {}", url)))
            }
        }
        "DELETE" => {
            let if_match = parse_bundle_if_match(request)?;
            let parts: Vec<&str> = url.split('/').collect();
            if parts.len() == 2 && !parts[1].contains('?') {
                let resource_type = parts[0];
                let id = parts[1];

                let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                    ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
                })?;

                if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
                    return Err(ApiError::forbidden(
                        "The default admin access policy cannot be deleted",
                    ));
                }

                if let Some(expected) = &if_match
                    && options.authorized_previous.is_none()
                {
                    check_bundle_if_match(tx, resource_type, id, expected).await?;
                }
                let deleted = if options.authorized_previous != Some(false) {
                    tx.delete_batch_with_snapshots(resource_type, &[id.into()])
                        .await
                        .map_err(map_storage_error)?
                        .pop()
                } else {
                    None
                };
                build_deletion_response_entry(resource_type, Some(id), deleted.as_ref())
            } else if url.contains('?') {
                let (resource_type, condition) = url
                    .find('?')
                    .map(|idx| (&url[..idx], &url[idx + 1..]))
                    .ok_or_else(|| ApiError::bad_request(format!("Invalid DELETE URL: {}", url)))?;

                let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                    ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
                })?;

                let matched = resolve_entry_condition(
                    tx,
                    resource_type,
                    condition,
                    pre_resolved,
                    options.conditional_params,
                    "Multiple matches for conditional delete",
                )
                .await?;

                match matched {
                    None if if_match.is_some() => Err(ApiError::precondition_failed(
                        "Conditional resource does not exist but If-Match was provided",
                    )),
                    None => Ok(build_transaction_response_entry(
                        None,
                        "204 No Content",
                        Some(resource_type),
                        None,
                        None,
                    )),
                    Some(existing) => {
                        let id = &existing.id;
                        if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
                            return Err(ApiError::forbidden(
                                "The default admin access policy cannot be deleted",
                            ));
                        }

                        if let Some(expected) = &if_match {
                            check_bundle_if_match(tx, resource_type, id, expected).await?;
                        }
                        let deleted = tx
                            .delete_batch_with_snapshots(resource_type, std::slice::from_ref(id))
                            .await
                            .map_err(map_storage_error)?
                            .pop();
                        build_deletion_response_entry(resource_type, Some(id), deleted.as_ref())
                    }
                }
            } else {
                Err(ApiError::bad_request(format!(
                    "Invalid DELETE url format: {}",
                    url
                )))
            }
        }
        "GET" | "HEAD" => {
            if let Some(prepared) = prepared_search {
                let types: Vec<_> = prepared.request.types.iter().map(String::as_str).collect();
                if let Some(snapshots) = &prepared.snapshots {
                    let result = snapshots
                        .execute_with_tx(state, tx, &types, &prepared.params, prepared.strict)
                        .await?;
                    return bundle_search_response(state, prepared, result);
                }
                let result = tx
                    .search_system_raw(
                        &types,
                        &prepared.params,
                        state.config.search.include_limits,
                        prepared.strict,
                    )
                    .await
                    .map_err(map_storage_error)?;
                return bundle_search_response(state, prepared, result);
            }
            let url = url.split('?').next().unwrap_or(url).trim_start_matches('/');
            let parts: Vec<&str> = url.split('/').collect();
            if !(parts.len() == 2 || (parts.len() == 4 && parts[2] == "_history")) {
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
            let snapshot = if parts.len() == 4 {
                tx.version_snapshot(resource_type, id, parts[3]).await
            } else {
                tx.current_snapshot(resource_type, id).await
            }
            .map_err(map_storage_error)?
            .ok_or_else(|| ApiError::not_found(format!("{resource_type}/{id}")))?;
            let stored = stored_snapshot(&snapshot.resource)?;
            if let Some(authorization) = options.prepared_read {
                authorization
                    .authorize_with_tx(state, tx, &stored.resource)
                    .await?;
            }
            if snapshot.method == octofhir_storage::HistoryMethod::Delete {
                return Err(ApiError::gone(
                    "Requested resource revision records a deletion",
                ));
            }
            bundle_read_response(entry, &stored)
        }
        "PATCH" => {
            resource.ok_or_else(|| ApiError::bad_request("PATCH entry requires a resource"))?;

            let conditional_target;
            let (resource_type, id) = if let Some((resource_type, condition)) = url.split_once('?')
            {
                conditional_target = resolve_entry_condition(
                    tx,
                    resource_type,
                    condition,
                    pre_resolved,
                    options.conditional_params,
                    "Multiple matches for conditional patch",
                )
                .await?
                .ok_or_else(|| {
                    ApiError::not_found("No resource matches the conditional criteria")
                })?;
                (resource_type, conditional_target.id.as_str())
            } else {
                batchable_target(url)
                    .ok_or_else(|| ApiError::bad_request(format!("Invalid PATCH URL: {url}")))?
            };
            let if_match = parse_bundle_if_match(request)?;

            let _rt = resource_type.parse::<ResourceType>().map_err(|_| {
                ApiError::bad_request(format!("Unknown resource type: {}", resource_type))
            })?;

            if resource_type == "AccessPolicy" && id == ADMIN_ACCESS_POLICY_ID {
                return Err(ApiError::forbidden(
                    "The default admin access policy cannot be modified",
                ));
            }

            let (patched_json, base_version) = prepared_patch
                .ok_or_else(|| ApiError::internal("Missing prepared transaction patch"))?;
            if options.authorized_previous.is_none() {
                let existing = tx
                    .read(resource_type, id)
                    .await
                    .map_err(map_storage_error)?
                    .ok_or_else(|| {
                        ApiError::not_found(format!("{resource_type}/{id} not found"))
                    })?;
                if existing.version_id != *base_version {
                    return Err(if if_match.is_some() {
                        ApiError::precondition_failed("Patch target changed after validation")
                    } else {
                        ApiError::conflict("Patch target changed after validation")
                    });
                }
            }

            let stored = tx
                .update(patched_json, Some(base_version))
                .await
                .map_err(map_storage_error)?;

            Ok(build_stored_transaction_response_entry(
                &stored,
                "200 OK",
                include_resource,
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
    include_resource: bool,
    auth: &octofhir_auth::middleware::AuthContext,
    headers: &HeaderMap,
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

    let mut response_entries: Vec<Value> = Vec::new();

    // Process each entry independently (no rollback on failure)
    for entry in entries {
        if let Err(error) = validate_bundle_entry_identity(entry) {
            response_entries.push(json!({"response": {
                "status": format!("{} {}", error.status_code().as_u16(), error.status_code().canonical_reason().unwrap_or("Error")),
                "outcome": error.to_operation_outcome()
            }}));
            continue;
        }
        let method = entry["request"]["method"]
            .as_str()
            .unwrap_or("")
            .to_ascii_uppercase();
        if matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
            let result = async {
                let authorization =
                    crate::authorization::authorize_batch_write(state, auth, entry)?;
                crate::bundle_write::execute(state, entry, authorization, headers, include_resource)
                    .await
            }
            .await;
            response_entries.push(match result {
                Ok(response) => response,
                Err(error) => json!({"response":{
                    "status":format!("{} {}",error.status_code().as_u16(),error.status_code().canonical_reason().unwrap_or("Error")),
                    "outcome":error.to_operation_outcome()
                }}),
            });
            continue;
        }
        let admission = async {
            let visibility = crate::authorization::authorize_entry(state, auth, entry).await?;
            let search = prepare_bundle_search(state, entry, visibility, headers).await?;
            let read = crate::authorization::prepare_entry_read(state, auth, entry).await?;
            Ok::<_, ApiError>((search, read))
        }
        .await;
        let (prepared_search, prepared_read) = match admission {
            Ok(prepared) => prepared,
            Err(error) => {
                response_entries.push(json!({"response": {
                    "status": format!("{} {}", error.status_code().as_u16(), error.status_code().canonical_reason().unwrap_or("Error")),
                    "outcome": error.to_operation_outcome()
                }}));
                continue;
            }
        };
        let result = if let Some(prepared) = prepared_search.as_ref() {
            execute_bundle_search(state, prepared).await
        } else if matches!(method.as_str(), "GET" | "HEAD") {
            process_get_entry(state, entry, prepared_read.as_ref())
                .await
                .map(|(response, _)| response)
        } else {
            Err(ApiError::bad_request("Unsupported batch interaction"))
        };
        match result {
            Ok(response_entry) => response_entries.push(response_entry),
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

    for entry in &mut response_entries {
        rebase_bundle_response_urls(state, entry);
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
fn sort_transaction_entries(entries: &[Value]) -> Vec<(usize, &Value)> {
    let mut sorted: Vec<_> = entries.iter().enumerate().collect();

    sorted.sort_by(|a, b| {
        let method_a = a.1["request"]["method"].as_str().unwrap_or("");
        let method_b = b.1["request"]["method"].as_str().unwrap_or("");

        let order_a = transaction_method_order(method_a);
        let order_b = transaction_method_order(method_b);

        order_a.cmp(&order_b).then_with(|| a.0.cmp(&b.0))
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

/// Process GET (read, vread, search) entry
async fn process_get_entry(
    state: &crate::server::AppState,
    entry: &Value,
    authorization: Option<&crate::authorization::PreparedRead>,
) -> Result<(Value, Option<(String, String)>), ApiError> {
    let url = entry["request"]["url"].as_str().unwrap_or("");
    let url = url.split('?').next().unwrap_or(url).trim_start_matches('/');
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
        let snapshot = state
            .storage
            .version_snapshot(resource_type, id, version)
            .await
            .map_err(map_storage_error)?
            .ok_or_else(|| {
                ApiError::not_found(format!(
                    "{}/{}/_history/{} not found",
                    resource_type, id, version
                ))
            })?;

        let stored = stored_snapshot(&snapshot.resource)?;
        if let Some(authorization) = authorization {
            authorization.authorize(state, &stored.resource).await?;
        }
        if snapshot.method == octofhir_storage::HistoryMethod::Delete {
            return Err(ApiError::gone(
                "Requested resource revision records a deletion",
            ));
        }
        let response_entry = bundle_read_response(entry, &stored)?;

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

        let snapshot = state
            .storage
            .current_snapshot(resource_type, id)
            .await
            .map_err(map_storage_error)?
            .ok_or_else(|| ApiError::not_found(format!("{}/{} not found", resource_type, id)))?;

        let stored = stored_snapshot(&snapshot.resource)?;
        if let Some(authorization) = authorization {
            authorization.authorize(state, &stored.resource).await?;
        }
        if snapshot.method == octofhir_storage::HistoryMethod::Delete {
            return Err(ApiError::gone(
                "Requested resource revision records a deletion",
            ));
        }
        let response_entry = bundle_read_response(entry, &stored)?;

        Ok((response_entry, None))
    } else {
        Err(ApiError::bad_request(format!("Invalid GET URL: {}", url)))
    }
}

fn bundle_read_response(
    entry: &Value,
    stored: &octofhir_storage::StoredResource,
) -> Result<Value, ApiError> {
    let request = &entry["request"];
    let mut headers = HeaderMap::new();
    if let Some(value) = request.get("ifNoneMatch") {
        let invalid = || ApiError::bad_request("Invalid request.ifNoneMatch");
        let value = value.as_str().ok_or_else(invalid)?;
        headers.insert(
            header::IF_NONE_MATCH,
            header::HeaderValue::from_str(value).map_err(|_| invalid())?,
        );
    }
    let modified_since = request
        .get("ifModifiedSince")
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| ApiError::bad_request("Invalid request.ifModifiedSince"))?;
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                .map_err(|_| {
                    ApiError::bad_request("request.ifModifiedSince requires a FHIR instant")
                })
        })
        .transpose()?;
    let not_modified = if headers.contains_key(header::IF_NONE_MATCH) {
        snapshot_not_modified(&headers, &stored.version_id, stored.last_updated)
    } else {
        modified_since.is_some_and(|since| stored.last_updated <= since)
    };
    let head = request["method"]
        .as_str()
        .is_some_and(|method| method.eq_ignore_ascii_case("HEAD"));
    let params: HashMap<String, String> = request["url"]
        .as_str()
        .and_then(|url| url.split_once('?'))
        .map(|(_, query)| {
            url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();
    let resource = if head || not_modified {
        None
    } else {
        Some(
            match apply_result_params_to_resource(&stored.resource.to_string(), &params)? {
                Some(projected) => serde_json::from_slice(&projected)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
                None => stored.resource.clone(),
            },
        )
    };
    let mut response = build_transaction_response_entry(
        resource.as_ref(),
        if not_modified {
            "304 Not Modified"
        } else {
            "200 OK"
        },
        Some(&stored.resource_type),
        Some(&stored.id),
        Some(&stored.version_id),
    );
    response["response"]["lastModified"] = json!(
        stored
            .last_updated
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|error| ApiError::internal(error.to_string()))?
    );
    Ok(response)
}

fn snapshot_not_modified(
    headers: &HeaderMap,
    version: &str,
    updated: time::OffsetDateTime,
) -> bool {
    if headers.contains_key(header::IF_NONE_MATCH) {
        return octofhir_api::check_if_none_match(headers, version);
    }
    headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok())
        .is_some_and(|since| {
            updated.unix_timestamp()
                <= since
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_secs() as i64)
                    .unwrap_or(i64::MIN)
        })
}

fn not_modified_response(version: &str, updated: time::OffsetDateTime) -> Response {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(header::ETAG, format!("W/\"{version}\""))
        .header(
            header::LAST_MODIFIED,
            httpdate::fmt_http_date(
                std::time::UNIX_EPOCH
                    + std::time::Duration::from_secs(updated.unix_timestamp().max(0) as u64),
            ),
        )
        .body(Body::empty())
        .expect("valid snapshot response headers")
}

/// True for `Type?query` conditional references — excludes `urn:` placeholders,
/// contained `#id`, absolute URLs, and resolved `Type/id` references.
fn is_conditional_reference(s: &str) -> bool {
    match s.split_once('?') {
        Some((head, rest)) => {
            !rest.is_empty()
                && head.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && head.chars().all(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Resolve conditional references with an independent cardinality check per
/// predicate. A global page over OR-ed identifiers can hide matches for a
/// different identifier, regardless of the page size chosen.
async fn prepare_conditional_reference_predicates(
    state: &crate::server::AppState,
    conditions: &HashSet<String>,
) -> Result<Vec<(String, String, octofhir_storage::SearchParams)>, ApiError> {
    if conditions.is_empty() {
        return Ok(Vec::new());
    }
    let mut conditions: Vec<_> = conditions.iter().collect();
    conditions.sort_unstable();
    let cfg = state.search_config.config();
    let mut predicates = Vec::with_capacity(conditions.len());
    for condition in conditions {
        let (resource_type, query) = condition
            .split_once('?')
            .filter(|(head, query)| !head.is_empty() && !query.is_empty())
            .ok_or_else(|| ApiError::bad_request("Conditional reference requires Type?criteria"))?;
        let params = octofhir_db_postgres::queries::search::prepare_system_search_params(
            &[resource_type],
            &conditional_search_params(query),
            &cfg.registry,
            state.terminology_provider.as_ref(),
            state.config.search.max_valueset_expansion,
        )
        .await
        .map_err(map_storage_error)?;
        predicates.push((condition.clone(), resource_type.to_owned(), params));
    }
    Ok(predicates)
}

fn proposed_transaction_graph(
    entries: &[(usize, Value)],
    conditions: &HashMap<usize, ConditionalMatch>,
    matched_posts: &HashMap<usize, Arc<MatchedExisting>>,
    patches: &HashMap<usize, (Value, String)>,
    keep: &dyn Fn(&str, &str) -> bool,
) -> Result<Vec<octofhir_storage::ProposedResource>, ApiError> {
    let mut graph = Vec::new();
    for (index, entry) in entries {
        let method = entry["request"]["method"]
            .as_str()
            .unwrap_or("")
            .to_ascii_uppercase();
        if matched_posts.contains_key(index)
            || !matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE")
        {
            continue;
        }
        let url = entry["request"]["url"].as_str().unwrap_or("");
        let resource_type = url.split(['?', '/']).next().unwrap_or_default();
        let body = patches
            .get(index)
            .map(|(body, _)| body)
            .or_else(|| entry.get("resource"));
        let id = if method == "DELETE" {
            match conditions.get(index) {
                Some(ConditionalMatch::One(row)) => Some(row.id.as_str()),
                Some(ConditionalMatch::NoMatch) => None,
                _ => batchable_target(url).map(|(_, id)| id),
            }
        } else {
            body.and_then(|body| body["id"].as_str())
        };
        let Some(id) = id else {
            if method == "DELETE" {
                continue;
            }
            return Err(ApiError::bad_request("Missing proposed resource identity"));
        };
        if !keep(resource_type, id) {
            continue;
        }
        graph.push(octofhir_storage::ProposedResource {
            resource_type: resource_type.to_owned(),
            id: id.to_owned(),
            resource: (method != "DELETE").then(|| body.cloned()).flatten(),
        });
    }
    Ok(graph)
}

#[derive(Clone)]
struct ConditionalReferenceGraph {
    resources: Vec<octofhir_storage::ProposedResource>,
    paths: HashMap<(String, String), Vec<(String, String)>>,
    changed_ancestors: HashMap<String, HashSet<String>>,
}

impl ConditionalReferenceGraph {
    /// `path_sources` are the bodies that carry conditional references; their
    /// paths are read in place, so only `resources` (proposed matches) are owned.
    async fn prepare(
        resources: Vec<octofhir_storage::ProposedResource>,
        path_sources: Vec<(&str, &str, &Value)>,
        remapper: &mut crate::bundle_references::BundleReferenceRemapper,
        provider: &dyn ModelProvider,
    ) -> Result<Self, ApiError> {
        let mut paths = HashMap::new();
        let mut changed_ancestors: HashMap<String, HashSet<String>> = HashMap::new();
        for (resource_type, id, body) in path_sources {
            let references: Vec<_> = remapper
                .reference_paths(provider, body)
                .await?
                .into_iter()
                .filter(|(_, value)| is_conditional_reference(value))
                .collect();
            let ancestors = changed_ancestors
                .entry(resource_type.to_owned())
                .or_default();
            for (pointer, _) in &references {
                let segments: Vec<_> = pointer
                    .split('/')
                    .skip(1)
                    .filter(|segment| !segment.chars().all(|c| c.is_ascii_digit()))
                    .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
                    .collect();
                for length in 1..=segments.len() {
                    ancestors.insert(segments[..length].join("."));
                }
            }
            paths.insert((resource_type.to_owned(), id.to_owned()), references);
        }
        Ok(Self {
            resources,
            paths,
            changed_ancestors,
        })
    }

    fn bind_resource(
        &self,
        body: &mut Value,
        assignment: &HashMap<String, String>,
    ) -> Result<(), ApiError> {
        let identity = (
            body["resourceType"].as_str().unwrap_or_default().to_owned(),
            body["id"].as_str().unwrap_or_default().to_owned(),
        );
        if let Some(paths) = self.paths.get(&identity) {
            for (pointer, condition) in paths {
                let target = assignment
                    .get(condition)
                    .ok_or_else(|| ApiError::internal("Missing conditional reference binding"))?;
                let value = body
                    .pointer_mut(pointer)
                    .ok_or_else(|| ApiError::internal("Conditional reference path changed"))?;
                *value = Value::String(target.clone());
            }
        }
        Ok(())
    }

    fn bind(
        &self,
        assignment: &HashMap<String, String>,
    ) -> Result<Vec<octofhir_storage::ProposedResource>, ApiError> {
        let mut resources = self.resources.clone();
        for row in &mut resources {
            if let Some(body) = &mut row.resource {
                self.bind_resource(body, assignment)?;
            }
        }
        Ok(resources)
    }
}

fn conditional_reference_candidate_predicates(
    state: &crate::server::AppState,
    predicates: &[(String, octofhir_storage::SearchParams)],
    graph: &ConditionalReferenceGraph,
) -> (Vec<(String, octofhir_storage::SearchParams)>, bool) {
    let registry = state.search_config.config().registry.snapshot();
    let coarse: Vec<_> = predicates
        .iter()
        .map(|(resource_type, params)| {
            let mut params = params.clone();
            // Only demonstrably invariant selectors may narrow an unbound domain.
            // Chained/reverse/reference/composite/full-text and custom expressions
            // can change when any occurrence of a conditional Reference is bound.
            params.parameters.retain(|name, _| {
                let code = name.split(':').next().unwrap_or(name);
                if matches!(
                    code,
                    "_id" | "_lastUpdated" | "_tag" | "_security" | "_profile"
                ) {
                    return true;
                }
                !name.contains('.')
                    && !name.starts_with("_has:")
                    && registry.get(resource_type, code).is_some_and(|parameter| {
                        !matches!(
                            parameter.param_type,
                            octofhir_search::SearchParameterType::Reference
                                | octofhir_search::SearchParameterType::Composite
                                | octofhir_search::SearchParameterType::Special
                        ) && parameter.expression.as_deref().is_some_and(|expression| {
                            conditional_reference_invariant_expression(expression)
                                && expression.split('|').all(|branch| {
                                    let path = branch.trim().trim_matches(['(', ')']).trim();
                                    let (root, path) =
                                        path.split_once('.').expect("validated simple path");
                                    (root != resource_type
                                        && !matches!(root, "Resource" | "DomainResource"))
                                        || graph
                                            .changed_ancestors
                                            .get(resource_type)
                                            .is_none_or(|paths| !paths.contains(path))
                                })
                        })
                    })
            });
            // These domains are candidates, never an authorization decision.
            // A prepared terminology/grant constraint may itself traverse a
            // Reference. The complete native predicates retain those constraints.
            params.constraint = None;
            params.type_constraints = None;
            (resource_type.clone(), params)
        })
        .collect();
    let dependent = predicates
        .iter()
        .zip(&coarse)
        .any(|((resource_type, full), (_, coarse))| {
            full.parameters != coarse.parameters
                || full
                    .constraint
                    .as_ref()
                    .is_some_and(|c| !conditional_constraint_unrestricted(c))
                || full.type_constraints.as_ref().is_some_and(|types| {
                    types
                        .get(resource_type)
                        .is_none_or(|c| !conditional_constraint_unrestricted(c))
                })
        });
    (coarse, dependent)
}

fn conditional_reference_invariant_expression(expression: &str) -> bool {
    expression.split('|').all(|path| {
        let parts: Vec<_> = path
            .trim()
            .trim_matches(['(', ')'])
            .trim()
            .split('.')
            .collect();
        parts.len() > 1
            && parts[0]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            && parts[0].chars().all(|c| c.is_ascii_alphabetic())
            && parts.iter().all(|part| {
                !part.is_empty()
                    && *part != "reference"
                    && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    })
}

fn conditional_constraint_unrestricted(constraint: &octofhir_storage::SearchConstraint) -> bool {
    use octofhir_storage::SearchConstraint;
    match constraint {
        SearchConstraint::And(parts) => parts.iter().all(conditional_constraint_unrestricted),
        SearchConstraint::Or(parts) => parts.iter().any(conditional_constraint_unrestricted),
        SearchConstraint::Query(params) => params.is_empty(),
        _ => false,
    }
}

async fn resolve_conditional_references_batched(
    state: &crate::server::AppState,
    prepared: &[(String, String, octofhir_storage::SearchParams)],
    coarse: &[(String, octofhir_storage::SearchParams)],
    dependent: bool,
    graph: &ConditionalReferenceGraph,
    metadata: &mut Option<octofhir_storage::TransactionWriteMetadata>,
) -> Result<HashMap<String, String>, ApiError> {
    let predicates: Vec<_> = prepared
        .iter()
        .map(|(_, rt, params)| (rt.clone(), params.clone()))
        .collect();
    let mut tx = state
        .storage
        .begin_transaction()
        .await
        .map_err(map_storage_error)?;
    // Only a graph whose matches depend on this bundle's own proposed
    // resources must be pinned; references to existing resources are
    // re-verified inside the committing transaction instead.
    if dependent {
        tx.lock_conditional_writes()
            .await
            .map_err(map_storage_error)?;
    }
    let reserved = tx
        .reserve_write_metadata(metadata.as_ref())
        .await
        .map_err(map_storage_error)?;
    // Invariant predicates only see rows of their own type, so the rest of the
    // bundle need not be shipped to Postgres as proposed rows.
    let types: HashSet<&str> = predicates.iter().map(|(rt, _)| rt.as_str()).collect();
    let mut graph = if dependent {
        graph.clone()
    } else {
        ConditionalReferenceGraph {
            resources: graph
                .resources
                .iter()
                .filter(|row| types.contains(row.resource_type.as_str()))
                .cloned()
                .collect(),
            paths: HashMap::new(),
            changed_ancestors: HashMap::new(),
        }
    };
    for row in &mut graph.resources {
        if let Some(body) = &mut row.resource {
            apply_planned_metadata(body, &reserved)?;
        }
    }
    *metadata = Some(reserved);
    let result = tokio::time::timeout(state.config.write_timeout(), async {
        if !dependent {
            let relevant = &graph.resources;
            let matches = if relevant.is_empty() {
                tx.search_conditional_batch(&predicates)
                    .await
                    .map_err(map_storage_error)?
            } else {
                tx.search_proposed_conditional_batch(&predicates, relevant)
                    .await
                    .map_err(map_storage_error)?
            };
            return conditional_reference_bindings(prepared, &matches);
        }
        solve_conditional_reference_graph(
            &*tx,
            prepared,
            &predicates,
            coarse,
            &graph,
            &state.config.server,
        )
        .await
    })
    .await
    .unwrap_or_else(|_| {
        Err(conditional_reference_cost_limit(
            "Conditional reference resolution exceeded the configured write timeout",
        ))
    });
    tx.rollback().await.map_err(map_storage_error)?;
    result
}

fn conditional_reference_cost_limit(message: &str) -> ApiError {
    ApiError::unprocessable_entity(
        message,
        Some(json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity":"error", "code":"too-costly", "diagnostics":message}]
        })),
    )
}

fn conditional_reference_bindings(
    prepared: &[(String, String, octofhir_storage::SearchParams)],
    matches: &[Vec<octofhir_storage::StoredResource>],
) -> Result<HashMap<String, String>, ApiError> {
    if matches.len() != prepared.len() {
        return Err(ApiError::internal("Incomplete proposed reference results"));
    }
    let mut bindings = HashMap::new();
    for ((condition, resource_type, _), rows) in prepared.iter().zip(matches) {
        match rows.as_slice() {
            [row] => {
                bindings.insert(condition.clone(), format!("{resource_type}/{}", row.id));
            }
            [] => {
                return Err(ApiError::unprocessable_entity(
                    format!("Conditional reference matched no resources: {condition}"),
                    None,
                ));
            }
            _ => {
                return Err(ApiError::precondition_failed(format!(
                    "Conditional reference matched multiple resources: {condition}"
                )));
            }
        }
    }
    Ok(bindings)
}

async fn solve_conditional_reference_graph(
    tx: &dyn octofhir_storage::Transaction,
    prepared: &[(String, String, octofhir_storage::SearchParams)],
    predicates: &[(String, octofhir_storage::SearchParams)],
    coarse: &[(String, octofhir_storage::SearchParams)],
    templates: &ConditionalReferenceGraph,
    limits: &crate::config::ServerConfig,
) -> Result<HashMap<String, String>, ApiError> {
    if coarse.len() > limits.max_conditional_reference_total_candidates {
        return Err(conditional_reference_cost_limit(
            "Conditional reference domains exceed the configured candidate budget",
        ));
    }
    let mut domains = Vec::with_capacity(coarse.len());
    let mut total_candidates = 0usize;
    for chunk in coarse.chunks(128) {
        let mut batch = tx
            .search_proposed_conditional_candidates(
                chunk,
                &templates.resources,
                limits.max_conditional_reference_candidates,
            )
            .await
            .map_err(map_storage_error)?;
        if batch.len() != chunk.len() {
            return Err(ApiError::internal(
                "Incomplete conditional reference candidate results",
            ));
        }
        total_candidates =
            total_candidates.saturating_add(batch.iter().map(Vec::len).sum::<usize>());
        if total_candidates > limits.max_conditional_reference_total_candidates {
            return Err(conditional_reference_cost_limit(
                "Conditional reference candidates exceed the configured total budget",
            ));
        }
        domains.append(&mut batch);
    }
    if domains.len() != prepared.len() {
        return Err(ApiError::internal(
            "Incomplete conditional reference candidate results",
        ));
    }
    for ((condition, _, _), domain) in prepared.iter().zip(&domains) {
        if domain.len() > limits.max_conditional_reference_candidates {
            return Err(conditional_reference_cost_limit(
                "Conditional reference candidates exceed the configured per-predicate limit",
            ));
        }
        if domain.is_empty() {
            return Err(ApiError::unprocessable_entity(
                format!("Conditional reference matched no resources: {condition}"),
                None,
            ));
        }
    }
    let mut choices = vec![0; prepared.len()];
    let mut solution = None;
    let mut checked = 0usize;
    loop {
        if checked >= limits.max_conditional_reference_assignments {
            // One found assignment is insufficient until alternatives have been
            // exhausted. Never accept a partial uniqueness proof at the budget.
            return Err(conditional_reference_cost_limit(
                "Conditional reference assignments exceed the configured resolution limit",
            ));
        }
        checked += 1;
        let assignment: HashMap<_, _> = prepared
            .iter()
            .enumerate()
            .map(|(index, (condition, rt, _))| {
                (
                    condition.clone(),
                    format!("{rt}/{}", domains[index][choices[index]]),
                )
            })
            .collect();
        let graph = templates.bind(&assignment)?;
        // Native chains and reverse chains see this entire assigned graph. Each
        // full predicate must have exactly one match, equal to its assigned ID.
        let matches = tx
            .search_proposed_conditional_batch(predicates, &graph)
            .await
            .map_err(map_storage_error)?;
        if matches.len() != prepared.len() {
            return Err(ApiError::internal("Incomplete assigned graph results"));
        }
        if prepared.iter().zip(&matches).all(|((condition, rt, _), rows)| {
            matches!(rows.as_slice(), [row] if assignment.get(condition) == Some(&format!("{rt}/{}", row.id)))
        }) {
            if solution.is_some() {
                return Err(ApiError::precondition_failed(
                    "Conditional references have multiple valid graph bindings"));
            }
            solution = Some(assignment);
        }
        // Iterative enumeration avoids a call stack proportional to Bundle size.
        let mut cursor = choices.len();
        loop {
            if cursor == 0 {
                return solution.ok_or_else(|| {
                    ApiError::unprocessable_entity(
                        "Conditional references have no stable unambiguous binding",
                        None,
                    )
                });
            }
            cursor -= 1;
            choices[cursor] += 1;
            if choices[cursor] < domains[cursor].len() {
                break;
            }
            choices[cursor] = 0;
        }
        tokio::task::yield_now().await;
    }
}

/// Result controls affect search presentation, never conditional cardinality.
pub(crate) fn conditional_search_params(query: &str) -> octofhir_storage::SearchParams {
    let mut params = octofhir_search::parse_query_string(query, 2, 2);
    params.parameters.retain(|key, _| {
        !matches!(
            key.split(':').next().unwrap_or(key),
            "_summary" | "_elements" | "_include" | "_revinclude" | "_format"
        )
    });
    params.count = Some(2);
    params.offset = Some(0);
    params.sort = None;
    params.total = None;
    params
}

/// Build a transaction response entry
fn build_matched_transaction_response_entry(
    stored: &MatchedExisting,
    status: &str,
    include_resource: bool,
) -> Value {
    let mut response = build_transaction_response_entry(
        include_resource.then_some(&stored.resource),
        status,
        stored.resource["resourceType"].as_str(),
        Some(&stored.id),
        Some(&stored.version_id),
    );
    response["response"]["lastModified"] = stored.resource["meta"]["lastUpdated"].clone();
    response
}

/// Owned variant for freshly written rows: moves the body into the entry.
fn build_owned_transaction_response_entry(
    stored: octofhir_storage::StoredResource,
    status: &str,
    include_resource: bool,
) -> Value {
    let last_modified = stored.resource["meta"]["lastUpdated"].clone();
    let mut response = build_transaction_response_entry(
        None,
        status,
        Some(&stored.resource_type),
        Some(&stored.id),
        Some(&stored.version_id),
    );
    if include_resource {
        response["resource"] = stored.resource;
    }
    response["response"]["lastModified"] = last_modified;
    response
}

fn build_stored_transaction_response_entry(
    stored: &octofhir_storage::StoredResource,
    status: &str,
    include_resource: bool,
) -> Value {
    let mut response = build_transaction_response_entry(
        include_resource.then_some(&stored.resource),
        status,
        Some(&stored.resource_type),
        Some(&stored.id),
        Some(&stored.version_id),
    );
    response["response"]["lastModified"] = stored.resource["meta"]["lastUpdated"].clone();
    response
}

fn build_deletion_response_entry(
    resource_type: &str,
    id: Option<&str>,
    deleted: Option<&octofhir_storage::DeletedResource>,
) -> Result<Value, ApiError> {
    let mut response = build_transaction_response_entry(
        None,
        "204 No Content",
        Some(resource_type),
        id,
        deleted.map(|row| row.version_id.as_str()),
    );
    response["response"]
        .as_object_mut()
        .unwrap()
        .remove("location");
    if let Some(deleted) = deleted {
        response["response"]["lastModified"] = json!(
            deleted
                .last_updated
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| ApiError::internal(error.to_string()))?
        );
    }
    Ok(response)
}

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
        if let Some(updated) = res["meta"]["lastUpdated"].as_str() {
            entry["response"]["lastModified"] = json!(updated);
        }
    }

    if let (Some(rt), Some(id)) = (resource_type, id) {
        if let Some(version) = version_id {
            entry["response"]["location"] = json!(format!("{rt}/{id}/_history/{version}"));
            entry["response"]["etag"] = json!(format!("W/\"{version}\""));
        }
        entry["fullUrl"] = json!(format!("/{rt}/{id}"));
    }

    entry
}

fn rebase_bundle_response_urls(state: &crate::server::AppState, entry: &mut Value) {
    if let Some(relative) = entry["fullUrl"]
        .as_str()
        .and_then(|url| url.strip_prefix('/'))
        .map(str::to_owned)
    {
        entry["fullUrl"] = json!(format!("{}/{relative}", state.fhir_base_url()));
    }
    if let Some(relative) = entry["response"]["location"]
        .as_str()
        .filter(|url| !url.contains(':'))
        .map(|url| url.trim_start_matches('/').to_owned())
    {
        entry["response"]["location"] = json!(format!("{}/{relative}", state.fhir_base_url()));
    }
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
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    Path((compartment_id, resource_type)): Path<(String, String)>,
    RawQuery(query_string): RawQuery,
    state_context: (
        State<crate::server::AppState>,
        crate::operations::OperationContext,
    ),
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    authorization: Option<Extension<crate::authorization::AuthorizedHistory>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (State(state), context) = state_context;
    let compartment_type = uri
        .path()
        .strip_prefix("/fhir/")
        .unwrap_or(uri.path().trim_start_matches('/'))
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    if crate::operations::is_operation(&resource_type) {
        let params = crate::operations::OperationParams::Get(
            url::form_urlencoded::parse(query_string.as_deref().unwrap_or_default().as_bytes())
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect(),
        );
        return crate::operations::router::instance_operation_handler_internal(
            &state,
            &context,
            &compartment_type,
            &compartment_id,
            &resource_type,
            params,
        )
        .await;
    }
    let query = Query::<HistoryQueryParams>::try_from_uri(&uri)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if resource_type == "_history" {
        return instance_history(
            State(state),
            authorization,
            Path((compartment_type, compartment_id)),
            query,
        )
        .await
        .map(IntoResponse::into_response);
    }
    execute_compartment_search(
        state,
        visibility,
        headers,
        CompartmentSearch {
            compartment_type,
            compartment_id,
            resource_type,
            query: query_string.unwrap_or_default(),
        },
    )
    .await
}

/// Search every linked type in a compartment, with one global page and total.
pub async fn compartment_search_all(
    Path((compartment_type, compartment_id)): Path<(String, String)>,
    State(state): State<crate::server::AppState>,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    execute_compartment_search(
        state,
        visibility,
        headers,
        CompartmentSearch {
            compartment_type,
            compartment_id,
            resource_type: "*".into(),
            query: query.unwrap_or_default(),
        },
    )
    .await
}

/// POST /Compartment/id/type/_search accepts URL and form criteria together.
pub async fn compartment_search_post(
    Path((compartment_type, compartment_id, resource_type)): Path<(String, String, String)>,
    State(state): State<crate::server::AppState>,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    body: Bytes,
) -> Result<Response, ApiError> {
    execute_compartment_search(
        state,
        visibility,
        headers,
        CompartmentSearch {
            compartment_type,
            compartment_id,
            resource_type,
            query: merge_search_query(raw, &body)?,
        },
    )
    .await
}

struct CompartmentSearch {
    compartment_type: String,
    compartment_id: String,
    resource_type: String,
    query: String,
}

async fn execute_compartment_search(
    state: crate::server::AppState,
    visibility: Option<Extension<crate::authorization::AuthorizedSearch>>,
    headers: HeaderMap,
    request: CompartmentSearch,
) -> Result<Response, ApiError> {
    let request = compartment_search_request(&state, request)?;
    execute_aggregate_search(state, visibility, headers, request).await
}

fn compartment_search_request(
    state: &crate::server::AppState,
    request: CompartmentSearch,
) -> Result<AggregateSearch, ApiError> {
    octofhir_core::validate_id(&request.compartment_id)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let definition = state
        .compartment_registry
        .get(&request.compartment_type)
        .map_err(|e| ApiError::not_found(e.to_string()))?;
    let types = if request.resource_type == "*" {
        let mut types: Vec<String> = definition
            .resource_types()
            .into_iter()
            .filter(|name| state.resource_type_set.load().contains(*name))
            .map(str::to_string)
            .collect();
        types.push(request.compartment_type.clone());
        types.sort_unstable();
        types.dedup();
        types
    } else if request.resource_type == request.compartment_type
        || definition
            .get_inclusion_params(&request.resource_type)
            .is_some()
    {
        vec![request.resource_type.clone()]
    } else {
        return Err(ApiError::bad_request(format!(
            "Resource type '{}' is not linked to the {} compartment",
            request.resource_type, request.compartment_type
        )));
    };
    let membership = types
        .iter()
        .map(|resource_type| {
            (
                resource_type.clone(),
                definition.search_membership_constraint(
                    &request.compartment_id,
                    resource_type,
                    &state.base_url,
                ),
            )
        })
        .collect();
    let path = format!(
        "{}/{}/{}",
        request.compartment_type, request.compartment_id, request.resource_type
    );
    Ok(AggregateSearch {
        types,
        query: request.query,
        path,
        membership: Some(membership),
    })
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
    context: crate::operations::OperationContext,
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

    job.authorize_owner(context.auth(), state.config.auth.policy.anonymous_access)?;

    if matches!(
        job.request_type.as_str(),
        "bulk_export" | "viewdefinition_export"
    ) && job.status == crate::async_jobs::AsyncJobStatus::Failed
    {
        return Err(ApiError::internal("Bulk export failed"));
    }

    // For bulk export jobs that are completed, return the manifest directly
    if matches!(
        job.request_type.as_str(),
        "bulk_export" | "viewdefinition_export"
    ) && job.status == crate::async_jobs::AsyncJobStatus::Completed
        && let Some(result) = job.result
    {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::EXPIRES,
            axum::http::HeaderValue::from_str(
                &job.expires_at
                    .format("%a, %d %b %Y %H:%M:%S GMT")
                    .to_string(),
            )
            .map_err(|_| ApiError::internal("Invalid export expiration"))?,
        );
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
        return Ok((StatusCode::OK, headers, Json(result)).into_response());
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
        if let Ok(progress_value) =
            axum::http::HeaderValue::from_str(&format!("Processing: {}% complete", progress_pct))
        {
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
    context: crate::operations::OperationContext,
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

    job.authorize_owner(context.auth(), state.config.auth.policy.anonymous_access)?;

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
    context: crate::operations::OperationContext,
) -> Result<impl IntoResponse, ApiError> {
    let job = state
        .async_job_manager
        .get_job(job_id)
        .await
        .map_err(|_| ApiError::not_found("Async job is unavailable"))?;
    job.authorize_owner(context.auth(), state.config.auth.policy.anonymous_access)?;
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

    Ok(
        if matches!(
            job.request_type.as_str(),
            "bulk_export" | "viewdefinition_export"
        ) {
            StatusCode::ACCEPTED
        } else {
            StatusCode::NO_CONTENT
        },
    )
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
    context: crate::operations::OperationContext,
) -> Result<impl IntoResponse, ApiError> {
    use std::path::PathBuf;
    use tokio::fs;

    // Validate job_id is a valid UUID to prevent path traversal
    let uuid = uuid::Uuid::parse_str(&job_id)
        .map_err(|_| ApiError::BadRequest("Invalid job ID format".to_string()))?;

    // Validate filename doesn't contain path traversal attempts
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(ApiError::BadRequest("Invalid filename".to_string()));
    }

    let job = state
        .async_job_manager
        .get_job(uuid)
        .await
        .map_err(|_| ApiError::not_found("Async job is unavailable"))?;
    job.authorize_owner(context.auth(), state.config.auth.policy.anonymous_access)?;
    if job.status != crate::async_jobs::AsyncJobStatus::Completed
        || !job.result.as_ref().is_some_and(|result| {
            ["output", "error"].iter().any(|field| {
                result[*field].as_array().is_some_and(|files| {
                    files.iter().any(|file| {
                        file["url"].as_str().is_some_and(|url| {
                            url == format!(
                                "{}/_bulk-files/{uuid}/{filename}",
                                state.fhir_base_url()
                            )
                        })
                    })
                })
            })
        })
    {
        return Err(ApiError::not_found("Export file is unavailable"));
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
        header::HeaderValue::from_static(if job.request_type == "viewdefinition_export" {
            "application/x-ndjson"
        } else {
            "application/fhir+ndjson"
        }),
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
    /// StructureDefinition.derivation: "specialization" (base resource/type) or
    /// "constraint" (profile). None for non-StructureDefinition resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivation: Option<String>,
    /// StructureDefinition.kind: "resource" | "complex-type" | "primitive-type" | "logical".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
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
    /// Filter by resource type. Accept both `resource_type` and `resourceType`.
    #[serde(alias = "resourceType")]
    pub resource_type: Option<String>,
    /// Free-text search across name, url and resource id (case-insensitive).
    #[serde(alias = "q")]
    pub search: Option<String>,
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
    use sqlx_core::row::Row;
    use sqlx_postgres::Postgres;

    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.offset.unwrap_or(0);

    // Normalize optional filters.
    let resource_type = params
        .resource_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let search = params
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    // ILIKE pattern; escape wildcards so user input is matched literally.
    let search_pattern = search.map(|s| {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        format!("%{}%", escaped)
    });

    // Shared WHERE-clause builder so list and count stay in sync.
    let push_filters = |qb: &mut sqlx_core::query_builder::QueryBuilder<Postgres>| {
        qb.push(" WHERE package_name = ")
            .push_bind(name.clone())
            .push(" AND package_version = ")
            .push_bind(version.clone());
        if let Some(rt) = resource_type {
            qb.push(" AND resource_type = ").push_bind(rt.to_string());
        }
        if let Some(ref pat) = search_pattern {
            qb.push(" AND (name ILIKE ")
                .push_bind(pat.clone())
                .push(" ESCAPE '\\' OR url ILIKE ")
                .push_bind(pat.clone())
                .push(" ESCAPE '\\' OR resource_id ILIKE ")
                .push_bind(pat.clone())
                .push(" ESCAPE '\\')");
        }
    };

    let mut qb = sqlx_core::query_builder::QueryBuilder::<Postgres>::new(
        "SELECT resource_type, resource_id, url, name, version, \
         content->>'derivation' AS derivation, content->>'kind' AS kind \
         FROM fcm.resources",
    );
    push_filters(&mut qb);
    qb.push(" ORDER BY resource_type, name, url LIMIT ")
        .push_bind(limit as i64)
        .push(" OFFSET ")
        .push_bind(offset as i64);

    let rows = qb
        .build()
        .fetch_all(state.db_pool.as_ref())
        .await
        .map_err(|e| ApiError::Internal(format!("Database error: {}", e)))?;

    let resources: Vec<PackageResourceSummary> = rows
        .iter()
        .map(|row| PackageResourceSummary {
            id: row.get("resource_id"),
            url: row.get("url"),
            name: row.get("name"),
            version: row.get("version"),
            resource_type: row.get("resource_type"),
            derivation: row.get("derivation"),
            kind: row.get("kind"),
        })
        .collect();

    // Total count with the same filters (ignoring limit/offset).
    let mut count_qb = sqlx_core::query_builder::QueryBuilder::<Postgres>::new(
        "SELECT COUNT(*) FROM fcm.resources",
    );
    push_filters(&mut count_qb);
    let total: i64 = count_qb
        .build_query_scalar()
        .fetch_one(state.db_pool.as_ref())
        .await
        .unwrap_or(0);

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

    // Try to get the pre-converted FHIRSchema from the database first.
    match store
        .get_fhirschema_from_package(&decoded_url, &name, &version)
        .await
    {
        Ok(Some(schema)) => return Ok(Json(schema.content)),
        Ok(None) => { /* fall through to on-the-fly conversion */ }
        Err(e) => return Err(ApiError::Internal(format!("Database error: {}", e))),
    }

    // Fallback: convert the StructureDefinition to a FHIRSchema on the fly.
    convert_structure_definition_on_the_fly(&state, &name, &version, &decoded_url).await
}

/// Fetch a StructureDefinition resource from a package and translate it to a
/// FHIRSchema at request time (used when no schema was pre-converted at install).
async fn convert_structure_definition_on_the_fly(
    state: &crate::server::AppState,
    name: &str,
    version: &str,
    decoded_url: &str,
) -> Result<Json<Value>, ApiError> {
    use octofhir_fhirschema::types::StructureDefinition;
    use sqlx_core::query::query;
    use sqlx_core::row::Row;

    let row = query(
        r#"
        SELECT content, resource_type
        FROM fcm.resources
        WHERE package_name = $1 AND package_version = $2 AND url = $3
        "#,
    )
    .bind(name)
    .bind(version)
    .bind(decoded_url)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::Internal(format!("Database error: {}", e)))?;

    let Some(row) = row else {
        return Err(ApiError::NotFound(format!(
            "Resource not found: {} in {}@{}",
            decoded_url, name, version
        )));
    };

    let resource_type: String = row.get("resource_type");
    if resource_type != "StructureDefinition" {
        return Err(ApiError::BadRequest(format!(
            "FHIRSchema is only available for StructureDefinition resources (got {})",
            resource_type
        )));
    }

    let content: Value = row.get("content");
    let sd: StructureDefinition = serde_json::from_value(content)
        .map_err(|e| ApiError::BadRequest(format!("Invalid StructureDefinition: {}", e)))?;

    let schema = octofhir_fhirschema::translate(sd, None)
        .map_err(|e| ApiError::Internal(format!("Failed to convert StructureDefinition: {}", e)))?;

    let schema_json = serde_json::to_value(&schema)
        .map_err(|e| ApiError::Internal(format!("Failed to serialize FHIRSchema: {}", e)))?;

    Ok(Json(schema_json))
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
            state.validation_service.invalidate_caches();
            // The FHIRPath engine memoizes element-type resolution
            // (`(type, property) -> TypeInfo`) across evaluations; those entries
            // are derived from the schema set and go stale when packages change.
            state.fhirpath_engine.clear_element_type_cache();
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
/// ```text
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
            .map_err(ApiError::Internal)?;

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
/// These are administrative resources defined in the octofhir-auth / octofhir-app /
/// octofhir-ui / octofhir-console IGs.
const INTERNAL_RESOURCE_TYPES: &[&str] = &[
    "User",
    "Role",
    "Client",
    "AccessPolicy",
    "IdentityProvider",
    "CustomOperation",
    "App",
    "AuthSession",
    // octofhir-ui IG
    "UserPreference",
    // octofhir-console IG
    "ConsoleCollection",
    "ConsoleSavedRequest",
    "ConsoleEnvironment",
    "ConsoleHistoryEntry",
];

/// Check if a resource type is an internal resource type.
fn is_internal_resource_type(resource_type: &str) -> bool {
    INTERNAL_RESOURCE_TYPES.contains(&resource_type)
}

/// Converts a GatewayError to an ApiError.
fn gateway_error_to_api_error(e: crate::gateway::error::GatewayError) -> ApiError {
    use crate::gateway::error::GatewayError;
    match e {
        GatewayError::Unauthorized(msg) => ApiError::unauthorized(msg),
        GatewayError::Forbidden(msg) => ApiError::forbidden(msg),
        GatewayError::BadRequest(msg) => ApiError::bad_request(msg),
        GatewayError::RouteNotFound { path, .. } => {
            ApiError::not_found(format!("No route for {}", path))
        }
        other => ApiError::internal(other.to_string()),
    }
}

/// Try to dispatch a request to the gateway router.
///
/// Returns the gateway response if a matching route is found,
/// or a 404 error if no route matches.
async fn dispatch_to_gateway_or_404(
    state: &crate::server::AppState,
    path: &str,
    resource_type: &str,
    request: Request<Body>,
) -> Result<Response, ApiError> {
    let method = request.method().as_str();

    if state.gateway_router.get_route(method, path).await.is_some() {
        crate::gateway::router::gateway_fallback_handler(State(state.clone()), request)
            .await
            .map(IntoResponse::into_response)
            .map_err(gateway_error_to_api_error)
    } else {
        Err(ApiError::not_found(format!(
            "Resource type '{}' is not available at this path",
            resource_type
        )))
    }
}

/// Try to dispatch to gateway with a pre-read body.
///
/// This variant is used by POST/PUT handlers where the body has already been extracted.
/// It rebuilds the request with the provided body bytes.
async fn dispatch_to_gateway_with_body_or_404(
    state: &crate::server::AppState,
    method: &str,
    path: &str,
    resource_type: &str,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if state.gateway_router.get_route(method, path).await.is_some() {
        // Rebuild the request with the body
        let mut request_builder = Request::builder().method(method).uri(path);

        // Copy headers
        if let Some(headers_mut) = request_builder.headers_mut() {
            headers_mut.extend(headers);
        }

        let request = request_builder
            .body(Body::from(body))
            .map_err(|e| ApiError::internal(format!("Failed to build request: {}", e)))?;

        crate::gateway::router::gateway_fallback_handler(State(state.clone()), request)
            .await
            .map(IntoResponse::into_response)
            .map_err(gateway_error_to_api_error)
    } else {
        Err(ApiError::not_found(format!(
            "Resource type '{}' is not available at this path",
            resource_type
        )))
    }
}

/// Validates App operations for path conflicts.
///
/// Fetches other Apps from database and validates that the new/updated App
/// doesn't have operations that conflict with system paths or other Apps.
async fn validate_app_payload(
    state: &crate::server::AppState,
    payload: &Value,
    exclude_id: Option<&str>,
) -> Result<(), ApiError> {
    // Parse the App from payload
    let app: App = serde_json::from_value(payload.clone())
        .map_err(|e| ApiError::bad_request(format!("Invalid App payload: {}", e)))?;

    // Fetch all other Apps for conflict checking
    let other_apps = fetch_other_apps(state, exclude_id).await?;

    // Validate operations
    if let Err(errors) = validate_app_operations(&app, &other_apps) {
        return Err(app_validation_errors_to_api_error(errors));
    }

    Ok(())
}

/// Fetches all Apps from the database, optionally excluding one by ID.
async fn fetch_other_apps(
    state: &crate::server::AppState,
    exclude_id: Option<&str>,
) -> Result<Vec<App>, ApiError> {
    use octofhir_storage::SearchParams;

    // Search for all App resources
    let params = SearchParams::new().with_count(1000);
    let search_result = state
        .storage
        .search("App", &params)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to fetch Apps: {}", e)))?;

    let mut apps = Vec::new();
    for stored in &search_result.entries {
        if let Ok(app) = serde_json::from_value::<App>(stored.resource.clone()) {
            // Exclude the App being updated
            if let Some(exclude) = exclude_id
                && app.id.as_deref() == Some(exclude)
            {
                continue;
            }
            apps.push(app);
        }
    }

    Ok(apps)
}

/// Converts App validation errors to an ApiError with OperationOutcome.
fn app_validation_errors_to_api_error(errors: Vec<PathValidationError>) -> ApiError {
    let issues: Vec<Value> = errors
        .iter()
        .enumerate()
        .map(|(i, e)| {
            json!({
                "severity": "error",
                "code": "business-rule",
                "diagnostics": e.to_string(),
                "expression": [format!("App.operations[{}].path", i)]
            })
        })
        .collect();

    let outcome = json!({
        "resourceType": "OperationOutcome",
        "issue": issues
    });

    ApiError::unprocessable_entity(
        format!("{} validation error(s) in App operations", errors.len()),
        Some(outcome),
    )
}

/// GET /{resource_type} - Search internal resources or dispatch to gateway.
///
/// This handler first checks if the resource type is an internal resource type.
/// If not, it checks if there's a matching gateway route and dispatches to it.
/// This allows App operations to be served at any root path (e.g., /psychportal/status).
pub async fn internal_search_resource(
    state: State<crate::server::AppState>,
    headers: HeaderMap,
    Path(resource_type): Path<String>,
    query: Query<HashMap<String, String>>,
    raw: RawQuery,
    request: Request<Body>,
) -> Result<Response, ApiError> {
    if is_internal_resource_type(&resource_type) {
        let visibility = request
            .extensions()
            .get::<crate::authorization::AuthorizedSearch>()
            .cloned()
            .map(Extension);
        let uri = request
            .extensions()
            .get::<axum::extract::OriginalUri>()
            .cloned()
            .unwrap_or_else(|| axum::extract::OriginalUri(request.uri().clone()));
        return search_resource(
            uri,
            state,
            visibility,
            headers,
            Path(resource_type),
            query,
            raw,
        )
        .await
        .map(IntoResponse::into_response);
    }

    // Not an internal resource - try gateway
    let path = format!("/{}", resource_type);
    dispatch_to_gateway_or_404(&state, &path, &resource_type, request).await
}

/// POST /{resource_type} - Create internal resource or dispatch to gateway.
///
/// This handler first checks if the resource type is an internal resource type.
/// If not, it checks if there's a matching gateway route and dispatches to it.
pub async fn internal_create_resource(
    state: State<crate::server::AppState>,
    Path(resource_type): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !is_internal_resource_type(&resource_type) {
        // Not an internal resource - try gateway
        let path = format!("/{}", resource_type);
        return dispatch_to_gateway_with_body_or_404(
            &state,
            "POST",
            &path,
            &resource_type,
            headers,
            body,
        )
        .await;
    }

    // Parse JSON payload for internal resources
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::bad_request(format!("Invalid JSON: {}", e)))?;

    // Validate App operations for path conflicts
    if resource_type == "App" {
        validate_app_payload(&state, &payload, None).await?;
    }

    let mut response = create_resource(
        state.clone(),
        Path(resource_type.clone()),
        None,
        headers,
        Json(payload.clone()),
    )
    .await?;
    rebase_internal_write_headers(&state, &mut response);

    // Reconcile operations if this is an App resource
    if resource_type == "App" {
        // On create, the server assigns the id (client-supplied id is ignored per
        // FHIR §3.1.0.1.3), so the request payload has none. Recover it from the
        // Location header and inject it before reconciling.
        let app_id = response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(extract_id_from_location);

        let Some(app_id) = app_id else {
            return Err(ApiError::internal(
                "Failed to reconcile operations: could not determine created App id",
            ));
        };

        let mut app_payload = payload.clone();
        if let Some(obj) = app_payload.as_object_mut() {
            obj.insert("id".to_string(), Value::String(app_id.clone()));
        }

        if let Err(e) = reconcile_app_operations(&state, &app_payload).await {
            // Rollback: delete the App we just created
            let _ = state.storage.delete("App", &app_id).await;
            tracing::error!(
                error = %e,
                app_id = %app_id,
                "Failed to reconcile operations, rolled back App creation"
            );
            return Err(ApiError::internal(format!(
                "Failed to reconcile operations: {}",
                e
            )));
        }
    }

    Ok(response.into_response())
}

/// GET /{resource_type}/{id} - Read internal resource or dispatch to gateway.
///
/// This handler first checks if the resource type is an internal resource type.
/// If not, it checks if there's a matching gateway route and dispatches to it.
pub async fn internal_read_resource(
    state: State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, ApiError> {
    if is_internal_resource_type(&resource_type) {
        let query_params = request
            .uri()
            .query()
            .map(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .into_owned()
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default();

        return read_resource(
            state,
            None,
            Path((resource_type, id)),
            headers,
            Query(query_params),
        )
        .await
        .map(IntoResponse::into_response);
    }

    // Not an internal resource - try gateway
    let path = format!("/{}/{}", resource_type, id);
    dispatch_to_gateway_or_404(&state, &path, &resource_type, request).await
}

/// PUT /{resource_type}/{id} - Update internal resource or dispatch to gateway.
///
/// This handler first checks if the resource type is an internal resource type.
/// If not, it checks if there's a matching gateway route and dispatches to it.
pub async fn internal_update_resource(
    state: State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !is_internal_resource_type(&resource_type) {
        // Not an internal resource - try gateway
        let path = format!("/{}/{}", resource_type, id);
        return dispatch_to_gateway_with_body_or_404(
            &state,
            "PUT",
            &path,
            &resource_type,
            headers,
            body,
        )
        .await;
    }

    // Parse JSON payload for internal resources
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::bad_request(format!("Invalid JSON: {}", e)))?;

    // Validate App operations for path conflicts
    if resource_type == "App" {
        validate_app_payload(&state, &payload, Some(&id)).await?;
    }

    // Store old App state for rollback
    let old_app = if resource_type == "App" {
        state.storage.read("App", &id).await.ok().flatten()
    } else {
        None
    };

    let mut response = update_resource(
        state.clone(),
        Path((resource_type.clone(), id.clone())),
        None,
        headers,
        Json(payload.clone()),
    )
    .await?;
    rebase_internal_write_headers(&state, &mut response);

    // Reconcile operations if this is an App resource. PUT uses the URL id, which
    // the request body may omit — inject it so reconciliation can find the App.
    let mut app_payload = payload.clone();
    if resource_type == "App"
        && let Some(obj) = app_payload.as_object_mut()
    {
        obj.insert("id".to_string(), Value::String(id.clone()));
    }
    if resource_type == "App"
        && let Err(e) = reconcile_app_operations(&state, &app_payload).await
    {
        // Rollback: restore old App state
        if let Some(old_resource) = old_app {
            let _ = state.storage.update(&old_resource.resource, None).await;
            tracing::error!(
                error = %e,
                app_id = %id,
                "Failed to reconcile operations, rolled back App update"
            );
        }
        return Err(ApiError::internal(format!(
            "Failed to reconcile operations: {}",
            e
        )));
    }

    Ok(response.into_response())
}

/// DELETE /{resource_type}/{id} - Delete internal resource or dispatch to gateway.
///
/// This handler first checks if the resource type is an internal resource type.
/// If not, it checks if there's a matching gateway route and dispatches to it.
pub async fn internal_delete_resource(
    state: State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    request: Request<Body>,
) -> Result<Response, ApiError> {
    if is_internal_resource_type(&resource_type) {
        return delete_resource(
            state,
            Path((resource_type, id)),
            None,
            request.headers().clone(),
        )
        .await
        .map(IntoResponse::into_response);
    }

    // Not an internal resource - try gateway
    let path = format!("/{}/{}", resource_type, id);
    dispatch_to_gateway_or_404(&state, &path, &resource_type, request).await
}

/// Helper function to reconcile App-derived resources (operations, subscriptions, and provisioned resources).
///
/// Calls reconciliation functions from the reconcile module to synchronize:
/// - CustomOperation resources based on App.operations[]
/// - AppSubscription resources based on App.subscriptions[]
/// - Provisioned resources based on App.resources (Client, AccessPolicy, etc.)
async fn reconcile_app_operations(
    state: &crate::server::AppState,
    app_payload: &Value,
) -> Result<(), ApiError> {
    use crate::gateway::types::App;
    use crate::reconcile::{reconcile_operations, reconcile_resources, reconcile_subscriptions};
    use std::collections::HashMap;

    // Deserialize App from payload
    let app: App = serde_json::from_value(app_payload.clone())
        .map_err(|e| ApiError::bad_request(format!("Invalid App resource: {}", e)))?;

    let app_id = app
        .id
        .as_ref()
        .ok_or_else(|| ApiError::internal("App has no ID"))?;

    // Reconcile operations
    reconcile_operations(
        &state.storage,
        app_id,
        &app.name,
        &app.endpoint,
        &app.operations,
    )
    .await?;

    // Reconcile subscriptions
    reconcile_subscriptions(&state.storage, app_id, &app.name, &app.subscriptions).await?;

    // Reconcile provisioned resources (if present)
    if let Some(resources_json) = &app.resources {
        let resources: HashMap<String, HashMap<String, Value>> =
            serde_json::from_str(resources_json)
                .map_err(|e| ApiError::bad_request(format!("Invalid App.resources JSON: {}", e)))?;

        reconcile_resources(&state.storage, app_id, &resources).await?;

        // Trigger policy cache reload if AccessPolicy resources were reconciled
        if resources.contains_key("AccessPolicy") {
            tracing::info!(app_id = %app_id, "Triggering policy cache reload after AccessPolicy reconciliation");
            state.policy_reload_service.trigger_reload();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn capability_fhir_version_matches_the_selected_model_release() {
        use octofhir_core::fhir::FhirVersion;
        let registry = octofhir_search::SearchParameterRegistry::new();
        let operations = crate::operations::OperationRegistry::new();
        for (release, expected) in [
            (FhirVersion::R4, "4.0.1"),
            (FhirVersion::R4B, "4.3.0"),
            (FhirVersion::R5, "5.0.0"),
            (FhirVersion::R6, "6.0.0"),
        ] {
            let metadata = build_capability_statement(
                release,
                "http://example.org",
                &["Patient".into()],
                &registry,
                &operations,
                &HashMap::new(),
                false,
            )
            .await
            .unwrap();
            assert_eq!(metadata["fhirVersion"], expected);
            assert_eq!(metadata["rest"][0]["resource"][0]["type"], "Patient");
        }
    }

    #[test]
    fn capability_operations_follow_enabled_handlers_and_invocation_levels() {
        use crate::operations::{OperationDefinition, OperationKind, OperationRegistry};
        let mut registry = OperationRegistry::new();
        for (code, resource) in [
            ("everything", "Patient"),
            ("evaluate-measure", "Measure"),
            ("run", "ViewDefinition"),
            ("unknown", "Patient"),
        ] {
            registry.register(OperationDefinition {
                code: code.into(),
                url: format!("http://example.org/OperationDefinition/{code}"),
                kind: OperationKind::Operation,
                system: true,
                type_level: true,
                instance: true,
                resource: vec![resource.into()],
                parameters: vec![],
                affects_state: false,
            });
        }
        let handlers: HashMap<String, crate::operations::DynOperationHandler> = HashMap::from([
            (
                "everything".into(),
                std::sync::Arc::new(crate::operations::EverythingOperation::new())
                    as crate::operations::DynOperationHandler,
            ),
            (
                "evaluate-measure".into(),
                std::sync::Arc::new(crate::operations::EvaluateMeasureOperation::new())
                    as crate::operations::DynOperationHandler,
            ),
            (
                "run".into(),
                std::sync::Arc::new(crate::operations::ViewDefinitionRunOperation::new(false))
                    as crate::operations::DynOperationHandler,
            ),
        ]);
        let mut metadata = json!({"rest":[{"mode":"server","resource":[{"type":"Patient"},{"type":"Measure"},{"type":"ViewDefinition"}]}]});
        apply_capability_operations(&mut metadata, &registry, &handlers, false);
        let rest = &metadata["rest"][0];
        assert!(
            rest.get("operation").is_none(),
            "unsupported system levels must not be advertised"
        );
        assert_eq!(
            rest["resource"][0]["operation"],
            json!([{"name":"everything","definition":"http://example.org/OperationDefinition/everything"}])
        );
        assert!(rest["resource"][1].get("operation").is_none());
        assert!(rest["resource"][2].get("operation").is_none());
        apply_capability_operations(&mut metadata, &registry, &handlers, true);
        assert_eq!(
            metadata["rest"][0]["operation"],
            json!([{"name":"graphql","definition":"http://hl7.org/fhir/OperationDefinition/Resource-graphql"}])
        );
    }

    #[test]
    fn capability_search_refresh_tracks_registry_updates_and_removals() {
        use octofhir_search::{SearchParameter, SearchParameterRegistry, SearchParameterType};
        let registry = SearchParameterRegistry::new();
        let mut metadata = json!({"rest":[{"resource":[{"type":"Patient","searchParam":[]}]}]});
        registry.upsert(
            SearchParameter::new(
                "custom",
                "http://example.org/custom",
                SearchParameterType::Token,
                vec!["Patient".into()],
            )
            .with_expression("Patient.identifier"),
        );
        refresh_capability_search(&mut metadata, &registry);
        let params = metadata["rest"][0]["resource"][0]["searchParam"]
            .as_array()
            .unwrap();
        assert!(
            params
                .iter()
                .any(|p| p["name"] == "custom" && p["type"] == "token")
        );
        assert!(registry.remove_by_url("http://example.org/custom"));
        refresh_capability_search(&mut metadata, &registry);
        assert!(
            !metadata["rest"][0]["resource"][0]["searchParam"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "custom")
        );
    }

    #[test]
    fn conditional_reference_detection() {
        assert!(is_conditional_reference(
            "Organization?identifier=http://ex.org|ORG-1"
        ));
        assert!(is_conditional_reference("Patient?identifier=12345"));
        // Not conditional references:
        assert!(!is_conditional_reference("Patient/123"));
        assert!(!is_conditional_reference("urn:uuid:abc-123"));
        assert!(!is_conditional_reference("#contained"));
        assert!(!is_conditional_reference(
            "http://ext.example/fhir/Patient/1"
        ));
        assert!(!is_conditional_reference("Patient?")); // empty query
    }

    #[test]
    fn conditional_search_ignores_paging_and_projection_controls() {
        let params = conditional_search_params(
            "identifier=sys%7CA&_count=0&_offset=100&_summary=count&_elements=id&_include=Patient:general-practitioner&_sort=-_id",
        );
        assert_eq!(params.parameters.len(), 1);
        assert_eq!(params.parameters["identifier"], vec!["sys|A"]);
        assert_eq!(params.count, Some(2));
        assert_eq!(params.offset, Some(0));
        assert!(params.sort.is_none());
    }

    #[tokio::test]
    async fn collect_conditional_references_dedupes_and_skips_non_conditional() {
        let provider = octofhir_fhirschema::FhirSchemaModelProvider::new(
            octofhir_fhirschema::embedded::get_schemas(
                octofhir_fhirschema::embedded::FhirVersion::R4,
            )
            .clone(),
            octofhir_fhir_model::FhirVersion::R4,
        );
        let resource = json!({
            "resourceType": "Encounter",
            "subject": {"reference": "urn:uuid:pat-1"},
            "serviceProvider": {"reference": "Organization?identifier=sys|ORG-1"},
            "participant": [
                {"individual": {"reference": "Organization?identifier=sys|ORG-1"}},
                {"individual": {"reference": "Practitioner?identifier=npi|999"}}
            ],
            "location": [{"location": {"reference": "Location/already-resolved"}}]
        });
        let mut remapper = crate::bundle_references::BundleReferenceRemapper::default();
        let out: HashSet<_> = remapper
            .references(&provider, &resource)
            .await
            .unwrap()
            .into_iter()
            .filter(|reference| is_conditional_reference(reference))
            .collect();
        assert_eq!(out.len(), 2, "duplicates collapse, non-conditional skipped");
        assert!(out.contains("Organization?identifier=sys|ORG-1"));
        assert!(out.contains("Practitioner?identifier=npi|999"));
    }

    #[tokio::test]
    async fn conditional_bindings_change_only_typed_reference_paths() {
        let provider = octofhir_fhirschema::FhirSchemaModelProvider::new(
            octofhir_fhirschema::embedded::get_schemas(
                octofhir_fhirschema::embedded::FhirVersion::R4,
            )
            .clone(),
            octofhir_fhir_model::FhirVersion::R4,
        );
        let condition = "Practitioner?identifier=urn%3Aconditional%7CP";
        let resource = json!({"resourceType":"Patient","id":"typed-plan",
            "generalPractitioner":[{"reference":condition}],
            "identifier":[{"system":condition,"value":condition}], "photo":[{"url":condition}],
            "meta":{"profile":[condition]},
            "extension":[{"url":"urn:typed:ref","valueReference":{"reference":condition}},
                {"url":"urn:typed:uri","valueUri":condition},
                {"url":"urn:typed:canonical","valueCanonical":condition}],
            "_birthDate":{"extension":[{"url":"urn:typed:primitive","valueReference":{"reference":condition}}]},
            "opaque":{"reference":condition},
            "text":{"status":"generated","div":format!("<div xmlns='http://www.w3.org/1999/xhtml'><a href='{condition}'>link</a></div>")}
        });
        let mut remapper = crate::bundle_references::BundleReferenceRemapper::default();
        let graph = ConditionalReferenceGraph::prepare(
            vec![octofhir_storage::ProposedResource {
                resource_type: "Patient".into(),
                id: "typed-plan".into(),
                resource: Some(resource.clone()),
            }],
            vec![("Patient", "typed-plan", &resource)],
            &mut remapper,
            &provider,
        )
        .await
        .unwrap();
        let assignments = HashMap::from([(condition.to_owned(), "Practitioner/actual".to_owned())]);
        let rows = graph.bind(&assignments).unwrap();
        let mut expected = resource;
        expected["generalPractitioner"][0]["reference"] = json!("Practitioner/actual");
        expected["extension"][0]["valueReference"]["reference"] = json!("Practitioner/actual");
        expected["_birthDate"]["extension"][0]["valueReference"]["reference"] =
            json!("Practitioner/actual");
        assert_eq!(rows[0].resource.as_ref().unwrap(), &expected);
    }

    #[test]
    fn test_sort_transaction_entries_preserves_original_indexes() {
        let entries = vec![
            json!({"request": {"method": "GET", "url": "Patient/p1"}}),
            json!({"request": {"method": "POST", "url": "Patient"}}),
            json!({"request": {"method": "DELETE", "url": "Patient/p2"}}),
        ];

        let sorted = sort_transaction_entries(&entries);
        let original_indexes: Vec<usize> = sorted.iter().map(|(idx, _)| *idx).collect();

        assert_eq!(original_indexes, vec![2, 1, 0]);
    }

    #[test]
    fn test_history_query_params_accept_offset_aliases() {
        let standard: HistoryQueryParams = serde_json::from_value(json!({
            "_count": 10,
            "_offset": 25
        }))
        .unwrap();
        assert_eq!(standard.count, Some(10));
        assert_eq!(standard.offset, Some(25));

        let legacy: HistoryQueryParams = serde_json::from_value(json!({
            "_count": 10,
            "__offset": 30
        }))
        .unwrap();
        assert_eq!(legacy.offset, Some(30));
    }

    #[tokio::test]
    async fn test_preprocess_app_secret_plaintext() {
        let resource_type = "App";
        let mut payload = json!({
            "resourceType": "App",
            "id": "test-app",
            "name": "Test App",
            "secret": "app_test_secret_1234567890abcdef1234567890abcdef1234567890abcdef12"
        });

        preprocess_payload(resource_type, &mut payload)
            .await
            .unwrap();

        let secret = payload["secret"].as_str().unwrap();
        // Should be hashed (Argon2id format)
        assert!(
            secret.starts_with("$argon2id$"),
            "Secret should be hashed with Argon2id"
        );
        assert_ne!(
            secret, "app_test_secret_1234567890abcdef1234567890abcdef1234567890abcdef12",
            "Secret should not be plaintext"
        );
    }

    #[tokio::test]
    async fn test_preprocess_app_secret_already_hashed() {
        let resource_type = "App";
        let original_hash = "$argon2id$v=19$m=19456,t=2,p=1$abcdef$1234567890";
        let mut payload = json!({
            "resourceType": "App",
            "id": "test-app",
            "name": "Test App",
            "secret": original_hash
        });

        preprocess_payload(resource_type, &mut payload)
            .await
            .unwrap();

        let secret = payload["secret"].as_str().unwrap();
        // Should remain unchanged if already hashed
        assert_eq!(
            secret, original_hash,
            "Already hashed secret should not be re-hashed"
        );
    }

    #[tokio::test]
    async fn test_preprocess_app_no_secret() {
        let resource_type = "App";
        let mut payload = json!({
            "resourceType": "App",
            "id": "test-app",
            "name": "Test App"
        });

        // Should not fail if no secret field
        preprocess_payload(resource_type, &mut payload)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_preprocess_app_secret_verification() {
        let resource_type = "App";
        let original_secret = "app_my_secret_key_1234567890abcdef1234567890abcdef1234567890abc";
        let mut payload = json!({
            "resourceType": "App",
            "id": "test-app",
            "name": "Test App",
            "secret": original_secret
        });

        preprocess_payload(resource_type, &mut payload)
            .await
            .unwrap();

        let hashed_secret = payload["secret"].as_str().unwrap();

        // Verify that the original secret can be verified against the hash
        assert!(
            octofhir_auth::verify_app_secret(original_secret, hashed_secret).unwrap(),
            "Original secret should verify against the hash"
        );

        // Verify that wrong secret doesn't match
        assert!(
            !octofhir_auth::verify_app_secret("wrong_secret", hashed_secret).unwrap(),
            "Wrong secret should not verify"
        );
    }

    #[test]
    fn test_parse_prefer_return_is_case_insensitive() {
        assert!(matches!(
            parse_prefer_return("respond-async, RETURN=OperationOutcome"),
            PreferReturn::OperationOutcome
        ));
        assert!(matches!(
            parse_prefer_return("RETURN=MINIMAL"),
            PreferReturn::Minimal
        ));
        assert!(matches!(
            parse_prefer_return("return=representation"),
            PreferReturn::Representation
        ));
    }

    #[test]
    fn test_fhir_versioned_resource_url_uses_base_path() {
        let url = fhir_versioned_resource_url("http://localhost:8080/fhir", "Patient", "123", "7");
        assert_eq!(url, "http://localhost:8080/fhir/Patient/123/_history/7");
    }

    #[test]
    fn test_resolved_search_total_infers_final_page_without_count_query() {
        assert_eq!(resolved_search_total(None, false, 20, 3), (Some(23), true));
        assert_eq!(resolved_search_total(None, true, 20, 10), (None, false));
        assert_eq!(
            resolved_search_total(Some(42), true, 20, 10),
            (Some(42), true)
        );
    }

    #[test]
    fn test_empty_page_does_not_invent_total_or_last_link() {
        for offset in [1, 20, 1000] {
            let (total, exact) = resolved_search_total(None, false, offset, 0);
            assert_eq!((total, exact), (None, false));
            let links = octofhir_api::build_search_links_with_total_mode(
                total,
                exact,
                false,
                "http://localhost/fhir",
                "Patient",
                offset,
                10,
                None,
            );
            assert!(
                !links
                    .iter()
                    .any(|link| matches!(link.relation.as_str(), "last" | "next"))
            );
        }
        assert_eq!(resolved_search_total(None, false, 0, 0), (Some(0), true));
        assert_eq!(
            resolved_search_total(None, false, usize::MAX, 1),
            (None, false)
        );
        assert_eq!(
            resolved_search_total(Some(5), false, 1000, 0),
            (Some(5), true)
        );
    }

    #[test]
    fn test_search_plan_debug_requires_config_flag() {
        let mut settings = crate::config::SearchSettings::default();
        let headers = HeaderMap::new();

        assert!(!search_debug_request(&settings, &headers, "_debug=search").collect_plan());
        assert!(!search_debug_request(&settings, &headers, "_debug=search-plan").collect_plan());

        settings.allow_debug_search_plan = true;
        assert!(search_debug_request(&settings, &headers, "_debug=search").collect_plan());
        assert!(search_debug_request(&settings, &headers, "_debug=search-sql").collect_plan());
        assert!(search_debug_request(&settings, &headers, "_debug=search-plan").collect_plan());
        assert!(!search_debug_request(&settings, &headers, "_debug=other").collect_plan());
    }

    #[test]
    fn test_search_plan_debug_header_request() {
        let settings = crate::config::SearchSettings {
            allow_debug_search_plan: true,
            ..Default::default()
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            SEARCH_DEBUG_HEADER,
            SEARCH_HEADER_PLAN_VALUE.parse().unwrap(),
        );

        assert!(search_debug_request(&settings, &headers, "").collect_plan());
    }

    #[test]
    fn test_search_explain_debug_request_requires_config_flag() {
        let mut settings = crate::config::SearchSettings::default();
        let headers = HeaderMap::new();

        assert_eq!(
            search_debug_request(&settings, &headers, "_debug=search-explain"),
            SearchDebugRequest::None
        );

        settings.allow_debug_search_plan = true;
        assert_eq!(
            search_debug_request(&settings, &headers, "_debug=search-explain"),
            SearchDebugRequest::Explain
        );
        assert_eq!(
            search_debug_request(&settings, &headers, "_debug=search-explain-analyze"),
            SearchDebugRequest::None
        );

        settings.allow_debug_search_explain_analyze = true;
        assert_eq!(
            search_debug_request(&settings, &headers, "_debug=search-explain-analyze"),
            SearchDebugRequest::ExplainAnalyze
        );
    }

    #[test]
    fn test_search_explain_debug_header_request() {
        let settings = crate::config::SearchSettings {
            allow_debug_search_plan: true,
            ..Default::default()
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            SEARCH_DEBUG_HEADER,
            SEARCH_HEADER_EXPLAIN_VALUE.parse().unwrap(),
        );

        assert_eq!(
            search_debug_request(&settings, &headers, ""),
            SearchDebugRequest::Explain
        );
    }

    #[test]
    fn test_strip_search_debug_params() {
        assert_eq!(
            strip_search_debug_params("birthdate=ge2000-01-01&_debug=search-plan&_count=5"),
            "birthdate=ge2000-01-01&_count=5"
        );
        assert_eq!(
            strip_search_debug_params("birthdate=ge2000-01-01&_debug=search-explain&_count=5"),
            "birthdate=ge2000-01-01&_count=5"
        );
        assert_eq!(
            strip_search_debug_params("%5Fdebug=search-plan&name=Smith"),
            "name=Smith"
        );
        assert_eq!(strip_search_debug_params("_debug=search-plan"), "");
    }

    #[test]
    fn test_search_debug_parameters_resource_is_fhir_parameters_and_redacted() {
        let debug = octofhir_storage::RawSearchDebug {
            sql_shape: Some("SELECT * FROM resources WHERE id = $?".to_string()),
            plan: Some(json!({"resource_type": "Patient", "predicates": []})),
            explain: Some(json!([{"Plan": {"Node Type": "Seq Scan"}}])),
            analyze: false,
            build_elapsed_ms: Some(1.25),
            db_execute_elapsed_ms: Some(2.5),
        };

        let value = search_debug_parameters_resource(vec![(Some("Patient"), &debug)]);

        assert_eq!(value["resourceType"], "Parameters");
        let rendered = serde_json::to_string(&value).unwrap();
        assert!(rendered.contains("native-ir"));
        assert!(rendered.contains("SELECT * FROM resources WHERE id = $?"));
        assert!(rendered.contains("Patient"));
        assert!(!rendered.contains("$1"));
    }

    #[test]
    fn test_debug_param_removed_from_pagination_suffix() {
        let raw_q = strip_search_debug_params("birthdate=ge2000-01-01&_debug=search-plan&_count=5");

        assert_eq!(
            build_query_suffix_for_links(&raw_q),
            Some("birthdate=ge2000-01-01".to_string())
        );
    }

    #[test]
    fn search_links_keep_projections_and_applied_repeated_criteria() {
        let suffix = build_applied_query_suffix(
            "identifier=urn%3Atest%7Cfoo&identifier=urn%3Atest%7Cbar&unknown%3Amissing=true&%5Fcount=500&%5Foffset=3&_elements=id%2Cactive&_summary=true",
            &["unknown".into()],
        ).unwrap();
        let pairs: Vec<_> = url::form_urlencoded::parse(suffix.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("identifier".into(), "urn:test|foo".into()),
                ("identifier".into(), "urn:test|bar".into()),
                ("_elements".into(), "id,active".into()),
                ("_summary".into(), "true".into()),
            ]
        );
    }

    #[test]
    fn test_apply_result_params_to_resource_marks_subsetted() {
        let resource_json = serde_json::to_string(&json!({
            "resourceType": "Patient",
            "id": "123",
            "name": [{"family": "Subset"}],
            "telecom": [{"system": "phone", "value": "+10000000000"}]
        }))
        .unwrap();

        let params = HashMap::from([("_elements".to_string(), "name".to_string())]);
        let filtered = apply_result_params_to_resource(&resource_json, &params)
            .unwrap()
            .expect("filtered result");
        let filtered: Value = serde_json::from_slice(&filtered).unwrap();

        assert!(filtered.get("name").is_some());
        assert!(filtered.get("telecom").is_none());
        let tags = filtered["meta"]["tag"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(tags.iter().any(|tag| {
            tag["system"] == "http://terminology.hl7.org/CodeSystem/v3-ObservationValue"
                && tag["code"] == "SUBSETTED"
        }));
    }
}
