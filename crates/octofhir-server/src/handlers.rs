use crate::mapping::{IdPolicy, envelope_from_json, json_from_envelope};
use axum::{
    Json,
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use octofhir_api::ApiError;
use octofhir_core::{CoreError, ResourceType};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Serialize)]
pub struct HealthResponse<'a> {
    status: &'a str,
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
                    if content.get("type").and_then(|v| v.as_str()) == Some(rt) {
                        if let Some(url) = content.get("url").and_then(|v| v.as_str()) {
                            match content.get("derivation").and_then(|v| v.as_str()) {
                                Some("specialization") => base_profile = Some(url.to_string()),
                                Some("constraint") => supported_profiles.push(url.to_string()),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        let resource = octofhir_api::CapabilityStatementRestResource::new(*rt)
            .with_interactions(&["read", "search-type", "create", "update", "delete"][..])
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
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.create", resource_type = %resource_type);
    let _g = span.enter();
    if let Err(e) = crate::validation::validate_resource(&resource_type, &payload) {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }
    // Map JSON -> envelope and insert into storage
    match envelope_from_json(&resource_type, &payload, IdPolicy::Create) {
        Err(err) => {
            tracing::warn!(error.kind = "invalid-payload", message = %err);
            Err(ApiError::bad_request(err))
        }
        Ok(env) => {
            let id = env.id.clone();
            let rt = env.resource_type.clone();
            let body = json_from_envelope(&env);
            match state.storage.insert(&rt, env).await {
                Ok(()) => {
                    let mut headers = HeaderMap::new();
                    let loc = format!("/{resource_type}/{id}");
                    headers.insert(
                        header::LOCATION,
                        header::HeaderValue::from_str(&loc)
                            .unwrap_or_else(|_| header::HeaderValue::from_static("/")),
                    );
                    Ok((StatusCode::CREATED, headers, Json(body)))
                }
                Err(e) => {
                    tracing::error!(error.kind = "create-failed", message = %e);
                    Err(map_core_error(e))
                }
            }
        }
    }
}

pub async fn read_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.read", resource_type = %resource_type, id = %id);
    let _g = span.enter();
    let rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        ))),
    };
    match state.storage.get(&rt, &id).await {
        Ok(Some(env)) => {
            let body = json_from_envelope(&env);
            Ok((StatusCode::OK, Json(body)))
        }
        Ok(None) => Err(ApiError::not_found(format!(
            "{resource_type} with id '{id}' not found"
        ))),
        Err(e) => Err(map_core_error(e)),
    }
}

pub async fn update_resource(
    State(state): State<crate::server::AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    let span = tracing::info_span!("fhir.update", resource_type = %resource_type, id = %id);
    let _g = span.enter();
    if let Err(e) = crate::validation::validate_resource(&resource_type, &payload) {
        return Err(ApiError::bad_request(format!("Validation failed: {e}")));
    }
    let rt = match resource_type.parse::<ResourceType>() {
        Ok(rt) => rt,
        Err(_) => return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        ))),
    };
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
                let body = json_from_envelope(&env);
                Ok((StatusCode::OK, Json(body)))
            }
            Err(e) => Err(map_core_error(e)),
        },
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
        Err(_) => return Err(ApiError::bad_request(format!(
            "Unknown resourceType '{resource_type}'"
        ))),
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
        CoreError::InvalidResourceType(s) => ApiError::bad_request(format!(
            "Invalid resource type: {s}"
        )),
        other => ApiError::internal(other.to_string()),
    }
}
