use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::{
    Json,
    extract::{FromRef, State},
    http::{HeaderValue, header},
    response::IntoResponse,
};
use octofhir_search::parameters::{SearchModifier, SearchParameterType};
use octofhir_search::registry::SearchParameterRegistry;
use octofhir_storage::DynStorage;
use serde::Serialize;
use sqlx_postgres::PgPool;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::operations::definition::OperationDefinition;
use crate::operations::registry::OperationRegistry;
use crate::server::AppState;

const BASE_PATH: &str = "/fhir";
const API_VERSION: u8 = 1;

#[derive(Clone)]
pub struct RestConsoleState {
    registry: Arc<SearchParameterRegistry>,
    operation_registry: Arc<OperationRegistry>,
    fhir_version: String,
    db_pool: Arc<PgPool>,
    storage: DynStorage,
}

impl RestConsoleState {
    pub fn new(
        registry: Arc<SearchParameterRegistry>,
        operation_registry: Arc<OperationRegistry>,
        fhir_version: impl Into<String>,
        db_pool: Arc<PgPool>,
        storage: DynStorage,
    ) -> Self {
        Self {
            registry,
            operation_registry,
            fhir_version: fhir_version.into(),
            db_pool,
            storage,
        }
    }
}

impl FromRef<AppState> for RestConsoleState {
    fn from_ref(state: &AppState) -> Self {
        Self::new(
            state.search_cfg.registry.clone(),
            state.fhir_operations.clone(),
            state.fhir_version.clone(),
            state.db_pool.clone(),
            state.storage.clone(),
        )
    }
}

/// GET /api/__introspect/rest-console handler
pub async fn introspect(State(state): State<RestConsoleState>) -> impl IntoResponse {
    let payload = build_payload(&state).await;
    let etag = compute_etag(&payload);

    let mut response = Json(payload).into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag)
            .unwrap_or_else(|_| HeaderValue::from_static("W/\"rc-invalid\"")),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=60"),
    );
    response
}

pub async fn build_payload(state: &RestConsoleState) -> RestConsoleResponse {
    let registry = state.registry.clone();
    let mut suggestions = Suggestions {
        resources: Vec::new(),
        system_operations: Vec::new(),
        type_operations: Vec::new(),
        instance_operations: Vec::new(),
        api_endpoints: Vec::new(),
    };

    // Build resource suggestions
    for resource_type in registry.list_resource_types() {
        let params = registry.get_all_for_type(resource_type);
        let param_count = params.len();

        suggestions.resources.push(AutocompleteSuggestion {
            id: format!("resource:{}", resource_type),
            kind: SuggestionKind::Resource,
            label: resource_type.to_string(),
            path_template: format!("{}/{}", BASE_PATH, resource_type),
            methods: vec!["GET".to_string(), "POST".to_string()],
            placeholders: vec![],
            description: Some(format!("{} search parameters", param_count)),
            metadata: SuggestionMetadata {
                resource_type: Some(resource_type.to_string()),
                affects_state: false,
                requires_body: false,
                category: Some("FHIR Resource".to_string()),
            },
        });
    }
    suggestions.resources.sort_by(|a, b| a.label.cmp(&b.label));

    // Load all operations
    let operations = load_all_operations_for_console(state).await;

    // Build operation suggestions
    for op in operations {
        // System-level operations
        if op.system {
            suggestions.system_operations.push(AutocompleteSuggestion {
                id: format!("system-op:{}", op.code),
                kind: SuggestionKind::SystemOp,
                label: format!("${}", op.code),
                path_template: format!("{}/${}", BASE_PATH, op.code),
                methods: vec![op.method.clone()],
                placeholders: vec![],
                description: op.path_templates.first().cloned(),
                metadata: SuggestionMetadata {
                    resource_type: None,
                    affects_state: op.affects_state,
                    requires_body: op.body_required,
                    category: Some("System Operation".to_string()),
                },
            });
        }

        // Type-level operations
        if op.type_level {
            for resource_type in &op.resource_types {
                if resource_type == "Resource" {
                    // Generic operation - add for all resource types
                    for rt in registry.list_resource_types() {
                        suggestions.type_operations.push(AutocompleteSuggestion {
                            id: format!("type-op:{}:{}", rt, op.code),
                            kind: SuggestionKind::TypeOp,
                            label: format!("${}", op.code),
                            path_template: format!("{}/{{resourceType}}/${}", BASE_PATH, op.code),
                            methods: vec![op.method.clone()],
                            placeholders: vec!["resourceType".to_string()],
                            description: Some(format!("{} for {}", op.code, rt)),
                            metadata: SuggestionMetadata {
                                resource_type: Some(rt.to_string()),
                                affects_state: op.affects_state,
                                requires_body: op.body_required,
                                category: Some("Type Operation".to_string()),
                            },
                        });
                    }
                } else {
                    suggestions.type_operations.push(AutocompleteSuggestion {
                        id: format!("type-op:{}:{}", resource_type, op.code),
                        kind: SuggestionKind::TypeOp,
                        label: format!("${}", op.code),
                        path_template: format!("{}/{{resourceType}}/${}", BASE_PATH, op.code),
                        methods: vec![op.method.clone()],
                        placeholders: vec!["resourceType".to_string()],
                        description: Some(format!("{} for {}", op.code, resource_type)),
                        metadata: SuggestionMetadata {
                            resource_type: Some(resource_type.clone()),
                            affects_state: op.affects_state,
                            requires_body: op.body_required,
                            category: Some("Type Operation".to_string()),
                        },
                    });
                }
            }
        }

        // Instance-level operations
        if op.instance {
            for resource_type in &op.resource_types {
                if resource_type == "Resource" {
                    // Generic operation - add for all resource types
                    for rt in registry.list_resource_types() {
                        suggestions
                            .instance_operations
                            .push(AutocompleteSuggestion {
                                id: format!("instance-op:{}:{}", rt, op.code),
                                kind: SuggestionKind::InstanceOp,
                                label: format!("${}", op.code),
                                path_template: format!(
                                    "{}/{{resourceType}}/{{id}}/${}",
                                    BASE_PATH, op.code
                                ),
                                methods: vec![op.method.clone()],
                                placeholders: vec!["resourceType".to_string(), "id".to_string()],
                                description: Some(format!("{} for {} instance", op.code, rt)),
                                metadata: SuggestionMetadata {
                                    resource_type: Some(rt.to_string()),
                                    affects_state: op.affects_state,
                                    requires_body: op.body_required,
                                    category: Some("Instance Operation".to_string()),
                                },
                            });
                    }
                } else {
                    suggestions
                        .instance_operations
                        .push(AutocompleteSuggestion {
                            id: format!("instance-op:{}:{}", resource_type, op.code),
                            kind: SuggestionKind::InstanceOp,
                            label: format!("${}", op.code),
                            path_template: format!(
                                "{}/{{resourceType}}/{{id}}/${}",
                                BASE_PATH, op.code
                            ),
                            methods: vec![op.method.clone()],
                            placeholders: vec!["resourceType".to_string(), "id".to_string()],
                            description: Some(format!(
                                "{} for {} instance",
                                op.code, resource_type
                            )),
                            metadata: SuggestionMetadata {
                                resource_type: Some(resource_type.clone()),
                                affects_state: op.affects_state,
                                requires_body: op.body_required,
                                category: Some("Instance Operation".to_string()),
                            },
                        });
                }
            }
        }
    }

    // Sort operations
    suggestions
        .system_operations
        .sort_by(|a, b| a.label.cmp(&b.label));
    suggestions.type_operations.sort_by(|a, b| {
        let rt_cmp = a.metadata.resource_type.cmp(&b.metadata.resource_type);
        if rt_cmp == std::cmp::Ordering::Equal {
            a.label.cmp(&b.label)
        } else {
            rt_cmp
        }
    });
    suggestions.instance_operations.sort_by(|a, b| {
        let rt_cmp = a.metadata.resource_type.cmp(&b.metadata.resource_type);
        if rt_cmp == std::cmp::Ordering::Equal {
            a.label.cmp(&b.label)
        } else {
            rt_cmp
        }
    });

    // Build API endpoint suggestions
    suggestions.api_endpoints = vec![
        AutocompleteSuggestion {
            id: "api:introspect".to_string(),
            kind: SuggestionKind::ApiEndpoint,
            label: "REST Console Metadata".to_string(),
            path_template: "/api/__introspect/rest-console".to_string(),
            methods: vec!["GET".to_string()],
            placeholders: vec![],
            description: Some("REST console metadata".to_string()),
            metadata: SuggestionMetadata {
                resource_type: None,
                affects_state: false,
                requires_body: false,
                category: Some("API".to_string()),
            },
        },
        AutocompleteSuggestion {
            id: "api:graphql".to_string(),
            kind: SuggestionKind::ApiEndpoint,
            label: "GraphQL".to_string(),
            path_template: "/api/graphql".to_string(),
            methods: vec!["POST".to_string()],
            placeholders: vec![],
            description: Some("GraphQL endpoint".to_string()),
            metadata: SuggestionMetadata {
                resource_type: None,
                affects_state: false,
                requires_body: true,
                category: Some("GraphQL".to_string()),
            },
        },
        AutocompleteSuggestion {
            id: "api:health".to_string(),
            kind: SuggestionKind::ApiEndpoint,
            label: "Health Check".to_string(),
            path_template: "/api/health".to_string(),
            methods: vec!["GET".to_string()],
            placeholders: vec![],
            description: Some("Server health".to_string()),
            metadata: SuggestionMetadata {
                resource_type: None,
                affects_state: false,
                requires_body: false,
                category: Some("System".to_string()),
            },
        },
    ];

    // Build search params by resource
    let mut search_params: HashMap<String, Vec<SearchParamSuggestion>> = HashMap::new();
    for resource_type in registry.list_resource_types() {
        let params = registry.get_all_for_type(resource_type);
        let mut param_suggestions: Vec<SearchParamSuggestion> = params
            .into_iter()
            .map(|param| SearchParamSuggestion {
                code: param.code.clone(),
                search_type: format_param_type(&param.param_type),
                description: if param.description.is_empty() {
                    None
                } else {
                    Some(param.description.clone())
                },
                modifiers: param
                    .modifier
                    .iter()
                    .map(|m| ModifierSuggestion {
                        code: modifier_to_string(m),
                        description: None,
                    })
                    .collect(),
                comparators: param.comparator.clone(),
                targets: param.target.clone(),
                is_common: param.is_common(),
            })
            .collect();

        param_suggestions.sort_by(|a, b| a.code.cmp(&b.code));
        search_params.insert(resource_type.to_string(), param_suggestions);
    }

    RestConsoleResponse {
        api_version: API_VERSION,
        fhir_version: state.fhir_version.clone(),
        base_path: BASE_PATH.to_string(),
        generated_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
        suggestions,
        search_params,
    }
}

/// Load all operations for the REST console:
/// 1. Operations from PostgreSQL (FHIR, UI, Auth, System providers)
/// 2. FHIR package operations from canonical manager (OperationDefinition resources)
/// 3. Gateway CustomOperations (loaded dynamically)
async fn load_all_operations_for_console(state: &RestConsoleState) -> Vec<OperationMetadata> {
    use crate::operation_registry::{OperationStorage, PostgresOperationStorage};
    use std::collections::HashSet;

    let mut all_operations: Vec<OperationMetadata> = Vec::new();
    let mut seen_codes: HashSet<String> = HashSet::new();

    // 1. Load operations from PostgreSQL (FHIR, UI, Auth, System providers)
    let op_storage = PostgresOperationStorage::new(state.db_pool.as_ref().clone());
    let postgres_ops = op_storage.list_all().await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Failed to load operations from PostgreSQL");
        Vec::new()
    });

    tracing::debug!(
        count = postgres_ops.len(),
        "Loaded operations from PostgreSQL"
    );

    for pg_op in postgres_ops {
        let metadata = OperationMetadata::from_core_definition(&pg_op);
        seen_codes.insert(pg_op.id.clone());
        all_operations.push(metadata);
    }

    // 2. Include FHIR package operations (OperationDefinition resources from canonical manager)
    for op in state.operation_registry.all() {
        if !seen_codes.contains(&op.code) {
            all_operations.push(OperationMetadata::from_definition(&op));
            seen_codes.insert(op.code.to_string());
        }
    }

    // 3. Load Gateway CustomOperations dynamically
    let gateway_ops = load_gateway_custom_operations(&state.storage).await;
    tracing::debug!(count = gateway_ops.len(), "Loaded Gateway CustomOperations");

    for gw_op in gateway_ops {
        let metadata = OperationMetadata::from_core_definition(&gw_op);
        if !seen_codes.contains(&gw_op.id) {
            all_operations.push(metadata);
            seen_codes.insert(gw_op.id);
        }
    }

    all_operations.sort_by(|a, b| a.code.cmp(&b.code));

    tracing::info!(
        total = all_operations.len(),
        "Loaded all operations for REST console"
    );

    all_operations
}

/// Load Gateway CustomOperations and convert to octofhir_core::OperationDefinition
async fn load_gateway_custom_operations(
    storage: &DynStorage,
) -> Vec<octofhir_core::OperationDefinition> {
    use crate::gateway::types::{App, CustomOperation};
    use octofhir_core::{OperationDefinition as CoreOpDef, categories};
    use octofhir_storage::SearchParams;
    use std::collections::HashMap;

    let search_params = SearchParams::new().with_count(1000);
    let apps_result = storage.search("App", &search_params).await;

    let apps: Vec<App> = match apps_result {
        Ok(result) => result
            .entries
            .into_iter()
            .filter_map(|stored| serde_json::from_value(stored.resource).ok())
            .filter(|app: &App| app.active)
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load Apps for Gateway operations");
            return Vec::new();
        }
    };

    let app_map: HashMap<String, App> = apps
        .into_iter()
        .filter_map(|app| app.id.clone().map(|id| (id, app)))
        .collect();

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

    let mut operations = Vec::new();

    for custom_op in custom_operations {
        let app_ref = match custom_op.app.reference.as_ref() {
            Some(r) => r,
            None => continue,
        };

        let app_id = match app_ref.split('/').next_back() {
            Some(id) => id,
            None => continue,
        };

        let app = match app_map.get(app_id) {
            Some(a) => a,
            None => continue,
        };

        let full_path = format!("{}{}", app.base_path, custom_op.path);

        let operation_id = format!(
            "gateway.{}.{}",
            app.name.to_lowercase().replace(' ', "_"),
            custom_op
                .id
                .as_ref()
                .unwrap_or(&"unknown".to_string())
                .to_lowercase()
        );

        let description = format!(
            "Custom {} operation: {} (Type: {})",
            custom_op.method, full_path, custom_op.operation_type
        );

        let op_def = CoreOpDef::new(
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
        .with_public(false);

        operations.push(op_def);
    }

    operations
}

fn compute_etag(payload: &RestConsoleResponse) -> String {
    let mut hasher = DefaultHasher::new();
    payload.fhir_version.hash(&mut hasher);
    payload.base_path.hash(&mut hasher);
    payload.api_version.hash(&mut hasher);
    payload.suggestions.resources.len().hash(&mut hasher);
    payload
        .suggestions
        .system_operations
        .len()
        .hash(&mut hasher);
    payload.suggestions.type_operations.len().hash(&mut hasher);
    payload
        .suggestions
        .instance_operations
        .len()
        .hash(&mut hasher);
    payload.suggestions.api_endpoints.len().hash(&mut hasher);

    // Hash search params count per resource
    for (resource_type, params) in &payload.search_params {
        resource_type.hash(&mut hasher);
        params.len().hash(&mut hasher);
    }

    format!("W/\"rc-{hash:x}\"", hash = hasher.finish())
}

fn modifier_to_string(modifier: &SearchModifier) -> String {
    match modifier {
        SearchModifier::Exact => "exact",
        SearchModifier::Contains => "contains",
        SearchModifier::Text => "text",
        SearchModifier::In => "in",
        SearchModifier::NotIn => "not-in",
        SearchModifier::Below => "below",
        SearchModifier::Above => "above",
        SearchModifier::Not => "not",
        SearchModifier::Identifier => "identifier",
        SearchModifier::Type(_) => "type",
        SearchModifier::Missing => "missing",
        SearchModifier::OfType => "ofType",
    }
    .to_string()
}

fn format_param_type(param_type: &SearchParameterType) -> String {
    match param_type {
        SearchParameterType::Number => "number",
        SearchParameterType::Date => "date",
        SearchParameterType::String => "string",
        SearchParameterType::Token => "token",
        SearchParameterType::Reference => "reference",
        SearchParameterType::Composite => "composite",
        SearchParameterType::Quantity => "quantity",
        SearchParameterType::Uri => "uri",
        SearchParameterType::Special => "special",
    }
    .to_string()
}

fn operation_method(op: &OperationDefinition) -> &'static str {
    if op.affects_state { "POST" } else { "GET" }
}

// === Type Definitions ===

#[derive(Clone, Serialize)]
pub struct RestConsoleResponse {
    api_version: u8,
    fhir_version: String,
    base_path: String,
    generated_at: String,
    suggestions: Suggestions,
    search_params: HashMap<String, Vec<SearchParamSuggestion>>,
}

#[derive(Clone, Serialize, Hash)]
struct Suggestions {
    resources: Vec<AutocompleteSuggestion>,
    system_operations: Vec<AutocompleteSuggestion>,
    type_operations: Vec<AutocompleteSuggestion>,
    instance_operations: Vec<AutocompleteSuggestion>,
    api_endpoints: Vec<AutocompleteSuggestion>,
}

#[derive(Clone, Serialize, Hash, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SuggestionKind {
    Resource,
    SystemOp,
    TypeOp,
    InstanceOp,
    ApiEndpoint,
}

#[derive(Clone, Serialize, Hash)]
struct AutocompleteSuggestion {
    id: String,
    kind: SuggestionKind,
    label: String,
    path_template: String,
    methods: Vec<String>,
    placeholders: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    metadata: SuggestionMetadata,
}

#[derive(Clone, Serialize, Hash)]
struct SuggestionMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_type: Option<String>,
    affects_state: bool,
    requires_body: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
}

#[derive(Clone, Serialize, Hash)]
struct SearchParamSuggestion {
    code: String,
    #[serde(rename = "type")]
    search_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    modifiers: Vec<ModifierSuggestion>,
    comparators: Vec<String>,
    targets: Vec<String>,
    is_common: bool,
}

#[derive(Clone, Serialize, Hash)]
struct ModifierSuggestion {
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

// Internal operation metadata (for building suggestions)
struct OperationMetadata {
    code: String,
    method: String,
    affects_state: bool,
    body_required: bool,
    system: bool,
    type_level: bool,
    instance: bool,
    resource_types: Vec<String>,
    path_templates: Vec<String>,
}

impl OperationMetadata {
    fn from_definition(op: &OperationDefinition) -> Self {
        let method = operation_method(op).to_string();
        let mut templates = Vec::new();
        if op.system {
            templates.push(format!("${}", op.code));
        }
        if op.type_level {
            templates.push(format!("{{resourceType}}/${}", op.code));
        }
        if op.instance {
            templates.push(format!("{{resourceType}}/{{id}}/${}", op.code));
        }

        Self {
            code: op.code.clone(),
            method,
            affects_state: op.affects_state,
            body_required: op.affects_state,
            system: op.system,
            type_level: op.type_level,
            instance: op.instance,
            resource_types: if op.resource.is_empty() {
                vec!["Resource".to_string()]
            } else {
                op.resource.clone()
            },
            path_templates: templates,
        }
    }

    fn from_core_definition(op: &octofhir_core::OperationDefinition) -> Self {
        let method = op
            .methods
            .first()
            .cloned()
            .unwrap_or_else(|| "GET".to_string());
        let templates = vec![op.path_pattern.clone()];
        let is_fhir_operation = op.path_pattern.contains('$');

        Self {
            code: op.name.clone(),
            method,
            affects_state: op
                .methods
                .iter()
                .any(|m| m == "POST" || m == "PUT" || m == "PATCH" || m == "DELETE"),
            body_required: op
                .methods
                .iter()
                .any(|m| m == "POST" || m == "PUT" || m == "PATCH"),
            system: is_fhir_operation && op.path_pattern.starts_with("/fhir/$"),
            type_level: is_fhir_operation && op.path_pattern.contains("/{type}/$"),
            instance: is_fhir_operation && op.path_pattern.contains("/{id}/$"),
            resource_types: vec![],
            path_templates: templates,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_conversion_handles_all_variants() {
        assert_eq!(modifier_to_string(&SearchModifier::Exact), "exact");
        assert_eq!(modifier_to_string(&SearchModifier::Contains), "contains");
        assert_eq!(modifier_to_string(&SearchModifier::OfType), "ofType");
    }

    #[test]
    fn suggestion_kind_serializes_as_kebab_case() {
        assert_eq!(
            serde_json::to_string(&SuggestionKind::SystemOp).unwrap(),
            "\"system-op\""
        );
        assert_eq!(
            serde_json::to_string(&SuggestionKind::TypeOp).unwrap(),
            "\"type-op\""
        );
    }
}
