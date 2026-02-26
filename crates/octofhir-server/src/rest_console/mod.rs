use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::{
    Json,
    extract::{FromRef, State},
    http::{HeaderValue, header},
    response::IntoResponse,
};
use octofhir_search::parameters::{SearchModifier, SearchParameter, SearchParameterType};
use octofhir_search::registry::SearchParameterRegistry;
use octofhir_storage::DynStorage;
use serde::Serialize;
use sqlx_postgres::PgPool;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::operations::definition::OperationDefinition;
use crate::operations::registry::OperationRegistry;
use crate::server::AppState;

const BASE_PATH: &str = "/fhir";
const SCHEMA_VERSION: u8 = 3;

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
            state.search_config.config().registry.clone(),
            state.fhir_operations.clone(),
            state.fhir_version.clone(),
            state.db_pool.clone(),
            state.storage.clone(),
        )
    }
}

/// GET /api/__introspect/rest-console handler (unified v3)
pub async fn introspect(State(state): State<RestConsoleState>) -> impl IntoResponse {
    let payload = build_unified_payload(&state).await;
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

pub async fn build_unified_payload(state: &RestConsoleState) -> RestConsoleResponse {
    let registry = &state.registry;
    let operations = load_all_operations_for_console(state).await;

    // === Build autocomplete suggestions ===
    let mut suggestions = Suggestions {
        resources: Vec::new(),
        system_operations: Vec::new(),
        type_operations: Vec::new(),
        instance_operations: Vec::new(),
        api_endpoints: Vec::new(),
    };

    for resource_type in registry.list_resource_types() {
        let param_count = registry.get_all_for_type(&resource_type).len();
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

    for op in &operations {
        let code = op.code.trim_start_matches('$');
        if op.system {
            suggestions.system_operations.push(AutocompleteSuggestion {
                id: format!("system-op:{}", code),
                kind: SuggestionKind::SystemOp,
                label: format!("${}", code),
                path_template: format!("{}/${}", BASE_PATH, code),
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
        if op.type_level {
            for resource_type in &op.resource_types {
                if resource_type == "Resource" {
                    for rt in registry.list_resource_types() {
                        suggestions.type_operations.push(AutocompleteSuggestion {
                            id: format!("type-op:{}:{}", rt, code),
                            kind: SuggestionKind::TypeOp,
                            label: format!("${}", code),
                            path_template: format!("{}/{{resourceType}}/${}", BASE_PATH, code),
                            methods: vec![op.method.clone()],
                            placeholders: vec!["resourceType".to_string()],
                            description: Some(format!("{} for {}", code, rt)),
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
                        id: format!("type-op:{}:{}", resource_type, code),
                        kind: SuggestionKind::TypeOp,
                        label: format!("${}", code),
                        path_template: format!("{}/{{resourceType}}/${}", BASE_PATH, code),
                        methods: vec![op.method.clone()],
                        placeholders: vec!["resourceType".to_string()],
                        description: Some(format!("{} for {}", code, resource_type)),
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
        if op.instance {
            for resource_type in &op.resource_types {
                if resource_type == "Resource" {
                    for rt in registry.list_resource_types() {
                        suggestions
                            .instance_operations
                            .push(AutocompleteSuggestion {
                                id: format!("instance-op:{}:{}", rt, code),
                                kind: SuggestionKind::InstanceOp,
                                label: format!("${}", code),
                                path_template: format!(
                                    "{}/{{resourceType}}/{{id}}/${}",
                                    BASE_PATH, code
                                ),
                                methods: vec![op.method.clone()],
                                placeholders: vec!["resourceType".to_string(), "id".to_string()],
                                description: Some(format!("{} for {} instance", code, rt)),
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
                            id: format!("instance-op:{}:{}", resource_type, code),
                            kind: SuggestionKind::InstanceOp,
                            label: format!("${}", code),
                            path_template: format!(
                                "{}/{{resourceType}}/{{id}}/${}",
                                BASE_PATH, code
                            ),
                            methods: vec![op.method.clone()],
                            placeholders: vec!["resourceType".to_string(), "id".to_string()],
                            description: Some(format!("{} for {} instance", code, resource_type)),
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

    // === Build search params by resource (flat map for autocomplete) ===
    let mut search_params: HashMap<String, Vec<SearchParamSuggestion>> = HashMap::new();
    for resource_type in registry.list_resource_types() {
        let params = registry.get_all_for_type(&resource_type);
        let mut param_suggestions: Vec<SearchParamSuggestion> = params
            .iter()
            .map(|param| SearchParamSuggestion {
                code: param.code.clone(),
                search_type: format_param_type(&param.param_type),
                description: if param.description.is_empty() {
                    None
                } else {
                    Some(param.description.clone())
                },
                modifiers: get_modifier_suggestions(param, &state.fhir_version),
                comparators: param.comparator.clone(),
                targets: param.target.clone(),
                is_common: param.is_common(),
            })
            .collect();
        param_suggestions.sort_by(|a, b| a.code.cmp(&b.code));
        search_params.insert(resource_type.to_string(), param_suggestions);
    }

    // === Build enriched resource capabilities ===
    let mut resources = Vec::new();
    for resource_type in registry.list_resource_types() {
        let params = registry.get_all_for_type(&resource_type);

        let enriched_params: Vec<EnrichedSearchParam> = params
            .iter()
            .map(|param| {
                let chains = if param.param_type == SearchParameterType::Reference
                    && !param.target.is_empty()
                {
                    compute_chains(registry, &param.code, &param.target)
                } else {
                    Vec::new()
                };

                EnrichedSearchParam {
                    code: param.code.clone(),
                    param_type: format_param_type(&param.param_type),
                    description: if param.description.is_empty() {
                        None
                    } else {
                        Some(param.description.clone())
                    },
                    modifiers: get_enriched_modifier_suggestions(param, &state.fhir_version),
                    comparators: param.comparator.clone(),
                    targets: param.target.clone(),
                    chains,
                    is_common: param.is_common(),
                }
            })
            .collect();

        let includes = compute_includes(&params);
        let rev_includes = compute_rev_includes(registry, &resource_type);
        let sort_params = compute_sort_params(&params);
        let type_operations = filter_operations_enriched(&operations, &resource_type, true, false);
        let instance_operations =
            filter_operations_enriched(&operations, &resource_type, false, true);

        resources.push(ResourceCapability {
            resource_type: resource_type.to_string(),
            search_params: enriched_params,
            includes,
            rev_includes,
            sort_params,
            type_operations,
            instance_operations,
        });
    }
    resources.sort_by(|a, b| a.resource_type.cmp(&b.resource_type));

    let system_operations: Vec<OperationCapability> = operations
        .iter()
        .filter(|op| op.system)
        .map(|op| OperationCapability {
            code: op.code.clone(),
            method: op.method.clone(),
            description: op.path_templates.first().cloned(),
            affects_state: op.affects_state,
            resource_types: op.resource_types.clone(),
        })
        .collect();

    RestConsoleResponse {
        schema_version: SCHEMA_VERSION,
        fhir_version: state.fhir_version.clone(),
        base_path: BASE_PATH.to_string(),
        generated_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
        suggestions,
        search_params,
        resources,
        system_operations,
        special_params: build_special_params(),
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
            .filter(|app: &App| app.is_active())
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

        let full_path = if let Some(base_path) = &app.base_path {
            format!("{}{}", base_path, custom_op.path)
        } else {
            custom_op.path.clone()
        };

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
    payload.schema_version.hash(&mut hasher);
    payload.fhir_version.hash(&mut hasher);
    payload.base_path.hash(&mut hasher);
    payload.suggestions.resources.len().hash(&mut hasher);
    payload.resources.len().hash(&mut hasher);
    for res in &payload.resources {
        res.resource_type.hash(&mut hasher);
        res.search_params.len().hash(&mut hasher);
        res.includes.len().hash(&mut hasher);
    }
    format!("W/\"rc-{hash:x}\"", hash = hasher.finish())
}

/// All known modifiers (excluding `Type` which is dynamic).
const ALL_MODIFIERS: &[SearchModifier] = &[
    SearchModifier::Missing,
    SearchModifier::Exact,
    SearchModifier::Contains,
    SearchModifier::Text,
    SearchModifier::Not,
    SearchModifier::In,
    SearchModifier::NotIn,
    SearchModifier::Below,
    SearchModifier::Above,
    SearchModifier::Identifier,
    SearchModifier::OfType,
    // R5+
    SearchModifier::CodeText,
    SearchModifier::TextAdvanced,
];

/// Whether the FHIR version is R5 or later.
fn is_r5_or_later(fhir_version: &str) -> bool {
    let v = fhir_version.to_ascii_uppercase();
    matches!(v.as_str(), "R5" | "R6" | "5.0.0" | "6.0.0")
}

/// Return modifier suggestions for a search parameter, respecting FHIR version.
/// If the parameter has explicit modifiers, use those; otherwise infer from the parameter type.
fn get_modifier_suggestions(
    param: &SearchParameter,
    fhir_version: &str,
) -> Vec<ModifierSuggestion> {
    if !param.modifier.is_empty() {
        return param
            .modifier
            .iter()
            .filter(|m| !m.is_r5_only() || is_r5_or_later(fhir_version))
            .map(|m| ModifierSuggestion {
                code: modifier_to_string(m),
                description: None,
            })
            .collect();
    }

    let r5 = is_r5_or_later(fhir_version);
    ALL_MODIFIERS
        .iter()
        .filter(|m| m.applicable_to(&param.param_type) && (!m.is_r5_only() || r5))
        .map(|m| ModifierSuggestion {
            code: modifier_to_string(m),
            description: None,
        })
        .collect()
}

/// Same as `get_modifier_suggestions` but returns `EnrichedModifierSuggestion`.
fn get_enriched_modifier_suggestions(
    param: &SearchParameter,
    fhir_version: &str,
) -> Vec<EnrichedModifierSuggestion> {
    if !param.modifier.is_empty() {
        return param
            .modifier
            .iter()
            .filter(|m| !m.is_r5_only() || is_r5_or_later(fhir_version))
            .map(|m| EnrichedModifierSuggestion {
                code: modifier_to_string(m),
                description: None,
            })
            .collect();
    }

    let r5 = is_r5_or_later(fhir_version);
    ALL_MODIFIERS
        .iter()
        .filter(|m| m.applicable_to(&param.param_type) && (!m.is_r5_only() || r5))
        .map(|m| EnrichedModifierSuggestion {
            code: modifier_to_string(m),
            description: None,
        })
        .collect()
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
        SearchModifier::CodeText => "code-text",
        SearchModifier::TextAdvanced => "text-advanced",
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
    schema_version: u8,
    fhir_version: String,
    base_path: String,
    generated_at: String,
    suggestions: Suggestions,
    search_params: HashMap<String, Vec<SearchParamSuggestion>>,
    resources: Vec<ResourceCapability>,
    system_operations: Vec<OperationCapability>,
    special_params: Vec<SpecialParamInfo>,
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

// === Enriched Capabilities helpers ===

fn compute_chains(
    registry: &SearchParameterRegistry,
    _param_code: &str,
    targets: &[String],
) -> Vec<ChainInfo> {
    let mut chains = Vec::new();
    for target_type in targets {
        // Don't expand wildcard "Resource" references — too many targets
        if target_type == "Resource" {
            chains.push(ChainInfo {
                target_type: target_type.clone(),
                target_params: Vec::new(),
            });
            continue;
        }
        let target_params: Vec<String> = registry
            .get_all_for_type(target_type)
            .iter()
            .filter(|p| !p.is_common())
            .map(|p| p.code.clone())
            .collect();
        if !target_params.is_empty() {
            chains.push(ChainInfo {
                target_type: target_type.clone(),
                target_params,
            });
        }
    }
    chains
}

fn compute_includes(
    params: &[Arc<octofhir_search::parameters::SearchParameter>],
) -> Vec<IncludeCapability> {
    params
        .iter()
        .filter(|p| p.param_type == SearchParameterType::Reference && !p.target.is_empty())
        .map(|p| IncludeCapability {
            param_code: p.code.clone(),
            target_types: p.target.clone(),
        })
        .collect()
}

fn compute_rev_includes(
    registry: &SearchParameterRegistry,
    resource_type: &str,
) -> Vec<IncludeCapability> {
    let mut rev_includes = Vec::new();
    for other_type in registry.list_resource_types() {
        for param in registry.get_all_for_type(&other_type) {
            if param.param_type == SearchParameterType::Reference
                && param.target.contains(&resource_type.to_string())
            {
                rev_includes.push(IncludeCapability {
                    param_code: format!("{}:{}", other_type, param.code),
                    target_types: vec![resource_type.to_string()],
                });
            }
        }
    }
    rev_includes
}

fn compute_sort_params(
    params: &[Arc<octofhir_search::parameters::SearchParameter>],
) -> Vec<String> {
    let mut sort_params = vec!["_id".to_string(), "_lastUpdated".to_string()];
    for param in params {
        match param.param_type {
            SearchParameterType::Date
            | SearchParameterType::String
            | SearchParameterType::Number => {
                if !param.is_common() {
                    sort_params.push(param.code.clone());
                }
            }
            _ => {}
        }
    }
    sort_params.sort();
    sort_params.dedup();
    sort_params
}

fn filter_operations_enriched(
    operations: &[OperationMetadata],
    resource_type: &str,
    type_level: bool,
    instance: bool,
) -> Vec<OperationCapability> {
    operations
        .iter()
        .filter(|op| {
            let level_match = if type_level {
                op.type_level
            } else if instance {
                op.instance
            } else {
                false
            };
            if !level_match {
                return false;
            }
            op.resource_types.contains(&resource_type.to_string())
                || op.resource_types.contains(&"Resource".to_string())
        })
        .map(|op| OperationCapability {
            code: op.code.clone(),
            method: op.method.clone(),
            description: op.path_templates.first().cloned(),
            affects_state: op.affects_state,
            resource_types: op.resource_types.clone(),
        })
        .collect()
}

fn build_special_params() -> Vec<SpecialParamInfo> {
    vec![
        SpecialParamInfo {
            name: "_count".into(),
            description: Some("Max results per page".into()),
            supported: true,
            examples: vec!["10".into(), "50".into(), "100".into()],
        },
        SpecialParamInfo {
            name: "_offset".into(),
            description: Some("Skip N results".into()),
            supported: true,
            examples: vec!["0".into(), "10".into()],
        },
        SpecialParamInfo {
            name: "_sort".into(),
            description: Some("Sort results by parameter (prefix with - for desc)".into()),
            supported: true,
            examples: vec!["-_lastUpdated".into(), "_id".into()],
        },
        SpecialParamInfo {
            name: "_summary".into(),
            description: Some("Return summary of results".into()),
            supported: true,
            examples: vec![
                "true".into(),
                "false".into(),
                "count".into(),
                "text".into(),
                "data".into(),
            ],
        },
        SpecialParamInfo {
            name: "_elements".into(),
            description: Some("Include only specific elements".into()),
            supported: true,
            examples: vec!["id,name".into()],
        },
        SpecialParamInfo {
            name: "_include".into(),
            description: Some("Include referenced resources in results".into()),
            supported: true,
            examples: vec![],
        },
        SpecialParamInfo {
            name: "_revinclude".into(),
            description: Some("Include resources that reference current results".into()),
            supported: true,
            examples: vec![],
        },
        SpecialParamInfo {
            name: "_total".into(),
            description: Some("Request total count mode".into()),
            supported: true,
            examples: vec!["none".into(), "estimate".into(), "accurate".into()],
        },
        SpecialParamInfo {
            name: "_has".into(),
            description: Some(
                "Reverse chaining — filter by resources that reference this one".into(),
            ),
            supported: true,
            examples: vec!["Observation:patient:code=1234-5".into()],
        },
        SpecialParamInfo {
            name: "_type".into(),
            description: Some("Filter by resource type (system-level search)".into()),
            supported: true,
            examples: vec!["Patient".into(), "Patient,Observation".into()],
        },
        SpecialParamInfo {
            name: "_filter".into(),
            description: Some("Advanced filter expression (FHIR _filter syntax)".into()),
            supported: true,
            examples: vec![
                "name eq \"Smith\"".into(),
                "birthdate ge 1990-01-01".into(),
                "status ne \"cancelled\" or priority eq \"urgent\"".into(),
            ],
        },
        SpecialParamInfo {
            name: "_contained".into(),
            description: Some("Search contained resources".into()),
            supported: false,
            examples: vec![],
        },
        SpecialParamInfo {
            name: "_containedType".into(),
            description: Some("Type of contained resource search".into()),
            supported: false,
            examples: vec![],
        },
    ]
}

// === Enriched Resource Types ===

#[derive(Clone, Serialize)]
pub struct ResourceCapability {
    pub resource_type: String,
    pub search_params: Vec<EnrichedSearchParam>,
    pub includes: Vec<IncludeCapability>,
    pub rev_includes: Vec<IncludeCapability>,
    pub sort_params: Vec<String>,
    pub type_operations: Vec<OperationCapability>,
    pub instance_operations: Vec<OperationCapability>,
}

#[derive(Clone, Serialize)]
pub struct EnrichedSearchParam {
    pub code: String,
    pub param_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub modifiers: Vec<EnrichedModifierSuggestion>,
    pub comparators: Vec<String>,
    pub targets: Vec<String>,
    pub chains: Vec<ChainInfo>,
    pub is_common: bool,
}

#[derive(Clone, Serialize)]
pub struct EnrichedModifierSuggestion {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ChainInfo {
    pub target_type: String,
    pub target_params: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct IncludeCapability {
    pub param_code: String,
    pub target_types: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct OperationCapability {
    pub code: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub affects_state: bool,
    pub resource_types: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct SpecialParamInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub supported: bool,
    pub examples: Vec<String>,
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
