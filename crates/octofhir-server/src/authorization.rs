//! Authorization of interactions inside FHIR convenience wrappers.

use octofhir_api::ApiError;
use octofhir_auth::middleware::AuthContext;
use octofhir_auth::policy::compartment::CompartmentChecker;
use octofhir_auth::policy::{AccessDecision, PolicyContext, PolicyContextBuilder};
use octofhir_auth::smart::scopes::{FhirOperation, ScopeContext};
use octofhir_storage::{IncludeVisibility, SearchConstraint, SearchParams};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::server::AppState;

mod snapshots;
pub(crate) use snapshots::PreparedSnapshotSearch;

/// Admission for registered snapshot-returning routes. The response handler
/// consumes this trusted context and authorizes the exact saved JSON it returns.
#[derive(Clone, Debug)]
pub struct AuthorizedRead(pub(crate) PolicyContext);

#[derive(Clone, Debug)]
pub struct AuthorizedHistory(pub(crate) PolicyContext);

impl AuthorizedHistory {
    pub(crate) async fn read(
        &self,
        state: &AppState,
        types: &[String],
        id: Option<&str>,
        history: &octofhir_storage::HistoryParams,
    ) -> Result<octofhir_storage::RawHistoryResult, ApiError> {
        let mut contexts = HashMap::new();
        let mut constraints = HashMap::new();
        for resource_type in types {
            let mut context = self.0.clone();
            context.request.resource_type = resource_type.clone();
            context.request.resource_id = id.map(str::to_owned);
            if !context.scopes.parsed.permits(
                resource_type,
                context.request.operation,
                context.environment.patient_context.as_deref(),
            ) {
                continue;
            }
            constraints.insert(
                resource_type.clone(),
                query_visibility(&context, &state.compartment_registry, &state.base_url)?
                    .unwrap_or(SearchConstraint::And(Vec::new())),
            );
            contexts.insert(
                resource_type.clone(),
                PreparedRead {
                    context,
                    params: SearchParams::new(),
                    constrained: false,
                },
            );
        }
        if contexts.is_empty() {
            return Err(ApiError::forbidden("No grant allows the requested history"));
        }
        let mut types: Vec<_> = contexts.keys().cloned().collect();
        types.sort();
        let types: Vec<_> = types.iter().map(String::as_str).collect();
        let mut params = SearchParams::new();
        params.type_constraints = Some(constraints);
        if let Some(id) = id {
            params = params.with_param("_id", id);
        }
        if let Some(since) = history.since {
            let instant = since
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            params = params.with_param("_lastUpdated", format!("ge{instant}"));
        }
        let at = history
            .at
            .map(|at| at.format(&time::format_description::well_known::Rfc3339))
            .transpose()
            .map_err(|error| ApiError::internal(error.to_string()))?;
        let at_end = history
            .at_end
            .map(|at| at.format(&time::format_description::well_known::Rfc3339))
            .transpose()
            .map_err(|error| ApiError::internal(error.to_string()))?;
        let registry = state.search_config.config().registry.clone();
        let params = octofhir_db_postgres::queries::search::prepare_system_search_params(
            &types,
            &params,
            &registry,
            state.terminology_provider.as_ref(),
            state.config.search.max_valueset_expansion,
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        let count = history.count.unwrap_or(100) as usize;
        let offset = history.offset.unwrap_or(0) as usize;
        let want_total = count == 0 || history.total == Some(octofhir_storage::TotalMode::Accurate);
        let mut tx = state
            .db_pool
            .begin()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        sqlx_core::query::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let result = tokio::time::timeout(state.config.read_timeout(), async {
            let mut cursor: Option<(i64, String, String)> = None;
            let mut visible = 0usize;
            let mut entries = Vec::new();
            loop {
                let rows =
                    octofhir_db_postgres::queries::search::execute_history_search_raw_with_tx(
                        &mut tx,
                        &types,
                        &params,
                        &registry,
                        cursor.as_ref().map(|(version, resource_type, id)| {
                            (*version, resource_type.as_str(), id.as_str())
                        }),
                        256,
                        at.as_deref().map(|at| (at, at_end.as_deref())),
                    )
                    .await
                    .map_err(crate::handlers::map_storage_error)?;
                let exhausted = rows.len() < 256;
                for row in rows {
                    cursor =
                        Some((
                            row.resource.version_id.parse().map_err(
                                |e: std::num::ParseIntError| ApiError::internal(e.to_string()),
                            )?,
                            row.resource.resource_type.clone(),
                            row.resource.id.clone(),
                        ));
                    let body: Value = serde_json::from_str(&row.resource.resource_json)
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                    match contexts[&row.resource.resource_type]
                        .authorize_policy(state, &body)
                        .await
                    {
                        Ok(()) => {}
                        Err(ApiError::Forbidden(_)) => continue,
                        Err(error) => return Err(error),
                    }
                    if visible >= offset && entries.len() < count {
                        entries.push(row);
                    }
                    visible = visible
                        .checked_add(1)
                        .ok_or_else(|| ApiError::internal("History count overflow"))?;
                    if !want_total && visible > offset.saturating_add(count) {
                        return Ok(octofhir_storage::RawHistoryResult {
                            entries,
                            total: None,
                        });
                    }
                }
                if exhausted {
                    break;
                }
            }
            if id.is_some() && visible == 0 {
                return Err(ApiError::not_found("Resource history is unavailable"));
            }
            let total = if want_total {
                Some(
                    u32::try_from(visible)
                        .map_err(|_| ApiError::internal("History count exceeds supported range"))?,
                )
            } else {
                None
            };
            Ok(octofhir_storage::RawHistoryResult { entries, total })
        })
        .await
        .map_err(|_| {
            ApiError::internal("History authorization exceeded the configured read timeout")
        })?;
        tx.rollback()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        result
    }
}

pub(crate) struct PreparedRead {
    context: PolicyContext,
    params: SearchParams,
    constrained: bool,
}

impl AuthorizedRead {
    pub(crate) async fn prepare(self, state: &AppState) -> Result<PreparedRead, ApiError> {
        let resource_type = &self.0.request.resource_type;
        if !self.0.scopes.parsed.permits(
            resource_type,
            self.0.request.operation,
            self.0.environment.patient_context.as_deref(),
        ) {
            return Err(ApiError::forbidden(
                "Insufficient scope for this interaction",
            ));
        }
        let mut params = SearchParams::new().with_count(1);
        params.constraint =
            query_visibility(&self.0, &state.compartment_registry, &state.base_url)?;
        let constrained = params.constraint.is_some();
        if constrained {
            params = octofhir_db_postgres::queries::search::prepare_system_search_params(
                &[resource_type.as_str()],
                &params,
                &state.search_config.config().registry,
                state.terminology_provider.as_ref(),
                state.config.search.max_valueset_expansion,
            )
            .await
            .map_err(crate::handlers::map_storage_error)?;
        }
        Ok(PreparedRead {
            context: self.0,
            params,
            constrained,
        })
    }
}

impl PreparedRead {
    pub(crate) async fn authorize(&self, state: &AppState, body: &Value) -> Result<(), ApiError> {
        if self.constrained {
            let tx = state
                .storage
                .begin_transaction()
                .await
                .map_err(crate::handlers::map_storage_error)?;
            let result = self.authorize_with_tx(state, &*tx, body).await;
            tx.rollback()
                .await
                .map_err(crate::handlers::map_storage_error)?;
            result
        } else {
            self.authorize_policy(state, body).await
        }
    }

    pub(crate) async fn authorize_with_tx(
        &self,
        state: &AppState,
        tx: &dyn octofhir_storage::Transaction,
        body: &Value,
    ) -> Result<(), ApiError> {
        if self.constrained {
            let id = body["id"]
                .as_str()
                .ok_or_else(|| ApiError::internal("Saved resource has no id"))?;
            let mut params = self.params.clone();
            params.parameters.insert("_id".into(), vec![id.to_owned()]);
            let resource_type = self.context.request.resource_type.clone();
            let matches = tx
                .search_proposed_conditional_batch(
                    &[(resource_type.clone(), params)],
                    &[octofhir_storage::ProposedResource {
                        resource_type,
                        id: id.into(),
                        resource: Some(body.clone()),
                    }],
                )
                .await
                .map_err(crate::handlers::map_storage_error)?;
            if !matches
                .first()
                .is_some_and(|rows| rows.iter().any(|row| row.resource == *body))
            {
                return Err(ApiError::not_found(
                    "Resource is unavailable for this interaction",
                ));
            }
        }
        self.authorize_policy(state, body).await
    }

    async fn authorize_policy(&self, state: &AppState, body: &Value) -> Result<(), ApiError> {
        let mut context = self.context.clone();
        context.resource = Some(octofhir_auth::policy::ResourceContext::from_resource(
            body.clone(),
        ));
        match state.policy_evaluator.evaluate(&context).await {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny(reason)
                if matches!(
                    reason.code.as_str(),
                    "policy-error" | "script-error" | "script-timeout" | "script-memory-limit"
                ) =>
            {
                tracing::error!(policy_id = ?reason.policy_id, code = %reason.code, "Read snapshot policy failed");
                Err(ApiError::internal("Resource policy evaluation failed"))
            }
            AccessDecision::Deny(reason) => Err(ApiError::forbidden(reason.message)),
            AccessDecision::Abstain => Err(ApiError::forbidden(
                "No policy allows this resource snapshot",
            )),
        }
    }
}

/// Trusted visibility prepared by authorization middleware for a type search.
#[derive(Clone, Debug)]
pub struct AuthorizedSearch {
    constraint: Option<SearchConstraint>,
    context: PolicyContext,
    deferred: bool,
}

impl AuthorizedSearch {
    pub(crate) fn for_prepared_context(context: PolicyContext) -> Self {
        Self {
            context,
            constraint: None,
            deferred: false,
        }
    }

    pub(crate) async fn apply(
        &self,
        state: &AppState,
        params: &mut SearchParams,
    ) -> Result<(), ApiError> {
        if let Some(constraint) = self.constraint.clone() {
            params.constraint = Some(match params.constraint.take() {
                Some(existing) => SearchConstraint::And(vec![existing, constraint]),
                None => constraint,
            });
        }
        self.apply_includes(state, params).await
    }

    /// Aggregate admission is deferred until the handler knows the actual types,
    /// including `_type` values submitted in a POST form body.
    pub(crate) fn is_deferred(&self) -> bool {
        self.deferred
    }

    pub(crate) async fn apply_types(
        &self,
        state: &AppState,
        types: &[String],
        params: &mut SearchParams,
    ) -> Result<(), ApiError> {
        let mut constraints = HashMap::new();
        for resource_type in types {
            let mut context = self.context.clone();
            context.resource = None;
            context.request.resource_type = resource_type.clone();
            context.request.resource_id = None;
            context.request.query_params = params
                .parameters
                .iter()
                .filter_map(|(key, values)| values.last().map(|value| (key.clone(), value.clone())))
                .collect();
            let policies = state
                .policy_evaluator
                .prepare(resource_type)
                .await
                .map_err(|error| ApiError::internal(error.to_string()))?;
            if !context.scopes.parsed.permits(
                resource_type,
                context.request.operation,
                context.environment.patient_context.as_deref(),
            ) || !matches!(policies.evaluate_admission(&context), AccessDecision::Allow)
            {
                continue;
            }
            // Expanding a system or wildcard request cannot bypass a policy
            // that restricts the corresponding ordinary type search.
            let mut type_context = context.clone();
            type_context.request.operation = FhirOperation::Search;
            type_context.request.method = "GET".into();
            type_context.request.path = format!("/fhir/{resource_type}");
            type_context.request.compartment_type = None;
            type_context.request.compartment_id = None;
            type_context.request.body = None;
            if !matches!(
                policies.evaluate_admission(&type_context),
                AccessDecision::Allow
            ) {
                continue;
            }
            constraints.insert(
                resource_type.clone(),
                query_visibility(&context, &state.compartment_registry, &state.base_url)?
                    .unwrap_or(SearchConstraint::And(Vec::new())),
            );
        }
        if constraints.is_empty() {
            return Err(ApiError::forbidden(
                "No search grant and policy allow the requested types",
            ));
        }
        params.type_constraints = Some(constraints);
        self.apply_includes(state, params).await
    }

    async fn apply_includes(
        &self,
        state: &AppState,
        params: &mut SearchParams,
    ) -> Result<(), ApiError> {
        let registry = state.search_config.config().registry.clone();
        let types = inline_resource_types(params, &registry);
        if types.is_empty() {
            return Ok(());
        }
        let mut visibility = IncludeVisibility {
            fhir_base_url: Some(format!("{}/fhir", state.base_url.trim_end_matches('/'))),
            ..Default::default()
        };
        for resource_type in types {
            let mut context = self.context.clone();
            context.resource = None;
            context.request.resource_type = resource_type.clone();
            context.request.resource_id = None;
            context.request.compartment_type = None;
            context.request.compartment_id = None;
            context.request.body = None;
            context.request.query_params.clear();
            context.request.operation = FhirOperation::Search;
            context.request.operation_id = None;
            context.request.method = "GET".into();
            context.request.path = format!("/fhir/{resource_type}");
            let policies = state
                .policy_evaluator
                .prepare(&resource_type)
                .await
                .map_err(|error| ApiError::internal(error.to_string()))?;
            // Inline requests are searches of the included resource type. Scopes
            // and underlying access policies both apply, including at each iterate hop.
            if !context.scopes.parsed.permits(
                &resource_type,
                FhirOperation::Search,
                context.environment.patient_context.as_deref(),
            ) || !matches!(policies.evaluate_admission(&context), AccessDecision::Allow)
            {
                continue;
            }
            let constraint =
                query_visibility(&context, &state.compartment_registry, &state.base_url)?
                    .unwrap_or(SearchConstraint::And(Vec::new()));
            visibility.constraints.insert(resource_type, constraint);
        }
        params.included_visibility = Some(visibility);
        Ok(())
    }
}

fn inline_resource_types(
    params: &SearchParams,
    registry: &octofhir_search::SearchParameterRegistry,
) -> HashSet<String> {
    let mut types = HashSet::new();
    for (name, values) in &params.parameters {
        let reverse = octofhir_search::include::is_revinclude_parameter(name);
        if !reverse && !octofhir_search::include::is_include_parameter(name) {
            continue;
        }
        for value in values {
            let parts: Vec<_> = value.split(':').collect();
            if parts.len() < 2 {
                continue;
            }
            if reverse {
                types.insert(parts[0].into());
            } else if let Some(target) = parts.get(2) {
                types.insert((*target).into());
            } else if let Some(parameter) = registry.get(parts[0], parts[1]) {
                types.extend(parameter.target.iter().cloned());
            }
        }
    }
    types
}

fn search_visibility(
    context: &PolicyContext,
    compartments: &crate::compartments::CompartmentRegistry,
    base_url: &str,
) -> Result<AuthorizedSearch, ApiError> {
    if !context.scopes.parsed.permits(
        &context.request.resource_type,
        FhirOperation::Search,
        context.environment.patient_context.as_deref(),
    ) {
        return Err(ApiError::forbidden("Insufficient scope for search"));
    }
    Ok(AuthorizedSearch {
        constraint: query_visibility(context, compartments, base_url)?,
        context: context.clone(),
        deferred: false,
    })
}

pub(crate) fn query_visibility(
    context: &PolicyContext,
    compartments: &crate::compartments::CompartmentRegistry,
    base_url: &str,
) -> Result<Option<SearchConstraint>, ApiError> {
    let resource_type = &context.request.resource_type;
    let patient = context.environment.patient_context.as_deref();
    let grants: Vec<_> = context
        .scopes
        .parsed
        .resource_scopes
        .iter()
        .filter(|scope| scope.matches(resource_type, context.request.operation, patient))
        .collect();
    if grants.is_empty() {
        return Ok(Some(SearchConstraint::Or(Vec::new())));
    }
    let unrestricted = grants
        .iter()
        .any(|scope| scope.context != ScopeContext::Patient && scope.filter.is_none());
    let constraint = if unrestricted {
        None
    } else {
        let mut alternatives = Vec::new();
        for grant in grants {
            let mut predicates = Vec::new();
            if grant.context == ScopeContext::Patient {
                let patient =
                    patient.ok_or_else(|| ApiError::forbidden("Patient context required"))?;
                if patient.is_empty()
                    || patient.len() > 64
                    || !patient
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.'))
                {
                    return Err(ApiError::forbidden("Invalid Patient context ID"));
                }
                let compartment = if resource_type == "Patient" {
                    SearchConstraint::Query(HashMap::from([("_id".into(), vec![patient.into()])]))
                } else {
                    let definition = compartments.get("Patient").map_err(|_| {
                        ApiError::forbidden("Patient compartment definition unavailable")
                    })?;
                    definition.membership_constraint(patient, resource_type, base_url)
                };
                predicates.push(compartment);
            }
            if let Some(filter) = &grant.filter {
                // Preserve repeated parameters (AND), comma alternatives (OR), and
                // percent-encoded values from the SMART query suffix.
                let query = format!("{}={}", filter.parameter, filter.value);
                let mut parameters: HashMap<String, Vec<String>> = HashMap::new();
                for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
                    if name.is_empty() {
                        return Err(ApiError::forbidden("Invalid scope filter"));
                    }
                    parameters
                        .entry(name.into_owned())
                        .or_default()
                        .push(value.into_owned());
                }
                if parameters.is_empty() {
                    return Err(ApiError::forbidden("Empty scope filter"));
                }
                predicates.push(SearchConstraint::Query(parameters));
            }
            alternatives.push(SearchConstraint::And(predicates));
        }
        Some(SearchConstraint::Or(alternatives))
    };
    Ok(constraint)
}

pub(crate) async fn hydrate_scope_context(
    state: &AppState,
    request: &mut axum::http::Request<axum::body::Body>,
    context: &mut PolicyContext,
) -> Result<(), ApiError> {
    let operation = context.request.operation;
    if matches!(
        operation,
        FhirOperation::HistoryInstance | FhirOperation::HistoryType | FhirOperation::HistorySystem
    ) && matches!(request.method().as_str(), "GET" | "HEAD")
    {
        let path = match (operation, &context.request.resource_id) {
            (FhirOperation::HistorySystem, _) => "/fhir/_history".to_owned(),
            (FhirOperation::HistoryInstance, Some(id)) => {
                format!("/fhir/{}/{id}/_history", context.request.resource_type)
            }
            (FhirOperation::HistoryType, _) => {
                format!("/fhir/{}/_history", context.request.resource_type)
            }
            _ => String::new(),
        };
        if request.uri().path() == path {
            if operation != FhirOperation::HistorySystem
                && !context.scopes.parsed.permits(
                    &context.request.resource_type,
                    operation,
                    context.environment.patient_context.as_deref(),
                )
            {
                return Err(ApiError::forbidden("Insufficient scope for history"));
            }
            request
                .extensions_mut()
                .insert(AuthorizedHistory(context.clone()));
            return Ok(());
        }
    }
    if (operation == FhirOperation::SearchSystem
        && matches!(request.uri().path(), "/fhir" | "/fhir/" | "/fhir/_search"))
        || (matches!(operation, FhirOperation::Search | FhirOperation::SearchType)
            && context.request.resource_type == "*"
            && is_compartment_search_path(request.uri().path(), context))
    {
        request.extensions_mut().insert(AuthorizedSearch {
            constraint: None,
            context: context.clone(),
            deferred: true,
        });
        return Ok(());
    }
    if operation.required_permission().is_some()
        && !context.scopes.parsed.permits(
            &context.request.resource_type,
            operation,
            context.environment.patient_context.as_deref(),
        )
    {
        return Err(ApiError::forbidden(
            "Insufficient scope for this interaction",
        ));
    }
    let primary_path = match (&context.request.resource_id, operation) {
        (None, FhirOperation::Create) => Some(format!("/fhir/{}", context.request.resource_type)),
        (None, FhirOperation::Update | FhirOperation::Patch | FhirOperation::Delete)
            if matches!(request.method().as_str(), "PUT" | "PATCH" | "DELETE") =>
        {
            Some(format!("/fhir/{}", context.request.resource_type))
        }
        (Some(id), FhirOperation::Update | FhirOperation::Patch | FhirOperation::Delete)
            if matches!(request.method().as_str(), "PUT" | "PATCH" | "DELETE") =>
        {
            Some(format!("/fhir/{}/{id}", context.request.resource_type))
        }
        _ => None,
    };
    if primary_path.as_deref() == Some(request.uri().path()) {
        request
            .extensions_mut()
            .insert(crate::write::AuthorizedWrite(context.clone()));
        return Ok(());
    }
    if matches!(operation, FhirOperation::Read | FhirOperation::VRead)
        && let Some(id) = &context.request.resource_id
    {
        let base = format!("/fhir/{}/{id}", context.request.resource_type);
        let is_registered = match operation {
            FhirOperation::Read => request.uri().path() == base,
            FhirOperation::VRead => request
                .uri()
                .path()
                .strip_prefix(&format!("{base}/_history/"))
                .is_some_and(|version| !version.is_empty() && !version.contains('/')),
            _ => false,
        };
        if is_registered && matches!(request.method().as_str(), "GET" | "HEAD") {
            request
                .extensions_mut()
                .insert(AuthorizedRead(context.clone()));
            return Ok(());
        }
    }
    if matches!(operation, FhirOperation::Search | FhirOperation::SearchType)
        && (is_type_search_path(request.uri().path(), &context.request.resource_type)
            || is_compartment_search_path(request.uri().path(), context))
    {
        let visibility = search_visibility(context, &state.compartment_registry, &state.base_url)?;
        request.extensions_mut().insert(visibility);
        return Ok(());
    }
    if operation.required_permission().is_none()
        || context.scopes.parsed.resource_scopes.iter().any(|scope| {
            scope.context != ScopeContext::Patient
                && scope.filter.is_none()
                && scope.matches(
                    &context.request.resource_type,
                    operation,
                    context.environment.patient_context.as_deref(),
                )
        })
    {
        return Ok(());
    }
    if !context.scopes.parsed.resource_scopes.iter().any(|scope| {
        scope.context == ScopeContext::Patient
            && scope.filter.is_none()
            && scope.matches(
                &context.request.resource_type,
                operation,
                context.environment.patient_context.as_deref(),
            )
    }) {
        return check_resource_scope(context, &state.base_url);
    }
    if let Some(id) = &context.request.resource_id {
        let stored = if operation == FhirOperation::VRead {
            let version = request.uri().path().rsplit('/').next().unwrap_or("");
            state
                .storage
                .vread(&context.request.resource_type, id, version)
                .await
        } else {
            state.storage.read(&context.request.resource_type, id).await
        }
        .map_err(|e| ApiError::Internal(e.to_string()))?;
        context.resource =
            stored.map(|r| octofhir_auth::policy::ResourceContext::from_resource(r.resource));
        // Check the current resource before interpreting a mutation document.
        check_resource_scope(context, &state.base_url)?;
    }
    if matches!(request.method().as_str(), "POST" | "PUT" | "PATCH") {
        let body = std::mem::replace(request.body_mut(), axum::body::Body::empty());
        let bytes = axum::body::to_bytes(body, state.config.server.body_limit_bytes)
            .await
            .map_err(|e| ApiError::bad_request(format!("Cannot read request body: {e}")))?;
        let mut resource: Value = serde_json::from_slice(&bytes)
            .map_err(|e| ApiError::bad_request(format!("Invalid JSON body: {e}")))?;
        if operation == FhirOperation::Patch {
            let existing = context
                .resource
                .as_ref()
                .ok_or_else(|| ApiError::forbidden("No resource available to authorize PATCH"))?;
            resource = if resource.is_array() {
                crate::patch::apply_json_patch(&existing.resource, &bytes)?
            } else {
                let model_provider: crate::server::SharedModelProvider =
                    state.model_provider.clone();
                crate::patch::apply_fhirpath_patch(
                    &state.fhirpath_engine,
                    &model_provider,
                    &existing.resource,
                    &bytes,
                )
                .await?
            };
        } else if operation == FhirOperation::Update
            && resource.get("id").is_none()
            && let Some(id) = &context.request.resource_id
        {
            resource["id"] = Value::String(id.clone());
        }
        context.request.body = Some(resource);
        *request.body_mut() = axum::body::Body::from(bytes);
    }
    Ok(())
}

fn is_type_search_path(path: &str, resource_type: &str) -> bool {
    let path = path
        .strip_prefix("/fhir/")
        .unwrap_or_else(|| path.trim_start_matches('/'));
    resource_type != "*" && (path == resource_type || path == format!("{resource_type}/_search"))
}

fn is_compartment_search_path(path: &str, context: &PolicyContext) -> bool {
    let path = path
        .strip_prefix("/fhir/")
        .unwrap_or(path.trim_start_matches('/'));
    let path = path.strip_suffix("/_search").unwrap_or(path);
    let parts: Vec<_> = path.split('/').collect();
    matches!(parts.as_slice(), [compartment, id, resource_type]
        if matches!(*compartment, "Patient" | "Encounter" | "Practitioner" | "RelatedPerson" | "Device")
            && context.request.compartment_type.as_deref() == Some(*compartment)
            && context.request.compartment_id.as_deref() == Some(*id)
            && *resource_type == context.request.resource_type)
}

fn entry_context(auth: &AuthContext, entry: &Value) -> Result<PolicyContext, ApiError> {
    let request = &entry["request"];
    let method = request["method"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.method"))?;
    let url = request["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.url"))?;
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if path.contains("://") || path.starts_with("//") || url.contains('#') {
        return Err(ApiError::bad_request(
            "Bundle request.url must be relative to the FHIR base",
        ));
    }
    let mut context = PolicyContextBuilder::new()
        .with_auth_context(auth)
        .with_request(
            method,
            path,
            url::form_urlencoded::parse(query.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
            entry.get("resource").cloned(),
        )
        .with_environment("bundle-entry".into(), None)
        .with_launch_context(auth.patient.clone(), auth.encounter.clone())
        .build()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    context.request.path = if path.trim_matches('/').is_empty() {
        "/fhir".into()
    } else {
        format!("/fhir/{}", path.trim_start_matches('/'))
    };
    Ok(context)
}

/// Batch mutations enter the same transaction protocol as primary writes.
/// Build identity exclusively from authenticated server context; actual grants
/// and policies are enforced against locked resources by the write handlers.
pub(crate) fn authorize_batch_write(
    state: &AppState,
    auth: &AuthContext,
    entry: &Value,
) -> Result<Option<crate::write::AuthorizedWrite>, ApiError> {
    let context = entry_context(auth, entry)?;
    if state.is_anonymous_context(auth) {
        return Ok(None);
    }
    if !matches!(
        context.request.operation,
        FhirOperation::Create
            | FhirOperation::Update
            | FhirOperation::Patch
            | FhirOperation::Delete
    ) {
        return Err(ApiError::bad_request("Unsupported batch write interaction"));
    }
    if !context.scopes.parsed.permits(
        &context.request.resource_type,
        context.request.operation,
        context.environment.patient_context.as_deref(),
    ) {
        return Err(ApiError::forbidden(
            "Insufficient scope for this Bundle entry",
        ));
    }
    Ok(Some(crate::write::AuthorizedWrite(context)))
}

pub(crate) async fn authorize_entry(
    state: &AppState,
    auth: &AuthContext,
    entry: &Value,
) -> Result<Option<AuthorizedSearch>, ApiError> {
    if state.is_anonymous_context(auth) {
        return Ok(None);
    }
    let request = &entry["request"];
    let method = request["method"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.method"))?;
    let url = request["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.url"))?;
    let path = url.split('?').next().unwrap_or(url);
    let mut context = entry_context(auth, entry)?;
    if method.eq_ignore_ascii_case("GET")
        && ((context.request.operation == FhirOperation::SearchSystem
            && path.trim_matches('/').is_empty())
            || (context.request.resource_type == "*" && is_compartment_search_path(path, &context)))
    {
        return Ok(Some(AuthorizedSearch {
            constraint: None,
            context,
            deferred: true,
        }));
    }
    if context.request.operation.required_permission().is_some()
        && !context.scopes.parsed.permits(
            &context.request.resource_type,
            context.request.operation,
            context.environment.patient_context.as_deref(),
        )
    {
        return Err(ApiError::forbidden(
            "Insufficient scope for this Bundle entry",
        ));
    }
    if matches!(
        context.request.operation,
        FhirOperation::Read | FhirOperation::VRead
    ) && matches!(context.request.method.as_str(), "GET" | "HEAD")
    {
        // Exact snapshot policies run when the Bundle read is executed, after
        // preceding transaction mutations and before projecting its response.
        return Ok(None);
    }
    if method.eq_ignore_ascii_case("GET")
        && matches!(
            context.request.operation,
            FhirOperation::Search | FhirOperation::SearchType
        )
        && (is_type_search_path(path, &context.request.resource_type)
            || is_compartment_search_path(path, &context))
    {
        let policies = state
            .policy_evaluator
            .prepare(&context.request.resource_type)
            .await
            .map_err(|error| ApiError::internal(error.to_string()))?;
        return match policies.evaluate_admission(&context) {
            AccessDecision::Allow => {
                search_visibility(&context, &state.compartment_registry, &state.base_url).map(Some)
            }
            AccessDecision::Deny(reason)
                if matches!(
                    reason.code.as_str(),
                    "policy-error" | "script-error" | "script-timeout" | "script-memory-limit"
                ) =>
            {
                Err(ApiError::internal("Resource policy evaluation failed"))
            }
            AccessDecision::Deny(reason) => Err(ApiError::forbidden(reason.message)),
            AccessDecision::Abstain => {
                Err(ApiError::forbidden("No policy allows this Bundle entry"))
            }
        };
    }
    if let Some(id) = &context.request.resource_id {
        let stored = if context.request.operation == FhirOperation::VRead {
            let version = path.rsplit('/').next().unwrap_or("");
            state
                .storage
                .vread(&context.request.resource_type, id, version)
                .await
        } else {
            state.storage.read(&context.request.resource_type, id).await
        }
        .map_err(|e| ApiError::Internal(e.to_string()))?;
        context.resource =
            stored.map(|r| octofhir_auth::policy::ResourceContext::from_resource(r.resource));
    }
    if context.request.operation == FhirOperation::Patch
        && let (Some(existing), Some(patch)) = (&context.resource, &context.request.body)
    {
        let model_provider: crate::server::SharedModelProvider = state.model_provider.clone();
        let patched = crate::patch::apply_bundle_patch(
            &state.fhirpath_engine,
            &model_provider,
            &existing.resource,
            patch,
        )
        .await?;
        context.request.body = Some(patched);
    }
    match state.policy_evaluator.evaluate(&context).await {
        AccessDecision::Allow => check_resource_scope(&context, &state.base_url).map(|()| None),
        AccessDecision::Deny(reason) => Err(ApiError::forbidden(reason.message)),
        AccessDecision::Abstain => Err(ApiError::forbidden("No policy allows this Bundle entry")),
    }
}

pub(crate) async fn prepare_entry_read(
    state: &AppState,
    auth: &AuthContext,
    entry: &Value,
) -> Result<Option<PreparedRead>, ApiError> {
    if state.is_anonymous_context(auth) {
        return Ok(None);
    }
    let context = entry_context(auth, entry)?;
    if matches!(
        context.request.operation,
        FhirOperation::Read | FhirOperation::VRead
    ) && matches!(context.request.method.as_str(), "GET" | "HEAD")
    {
        AuthorizedRead(context).prepare(state).await.map(Some)
    } else {
        Ok(None)
    }
}

/// Do not treat a patient or filtered grant as unrestricted resource access.
/// Aggregate paths need an authorization-aware query before they can use such
/// grants; until then they fail closed rather than returning other patients.
pub(crate) fn check_resource_scope(
    context: &PolicyContext,
    base_url: &str,
) -> Result<(), ApiError> {
    let operation = context.request.operation;
    if operation.required_permission().is_none() {
        return Ok(());
    }
    let grants: Vec<_> = context
        .scopes
        .parsed
        .resource_scopes
        .iter()
        .filter(|scope| {
            scope.matches(
                &context.request.resource_type,
                operation,
                context.environment.patient_context.as_deref(),
            )
        })
        .collect();
    if grants
        .iter()
        .any(|scope| scope.context != ScopeContext::Patient && scope.filter.is_none())
    {
        return Ok(());
    }
    if grants
        .iter()
        .any(|scope| scope.context == ScopeContext::Patient && scope.filter.is_none())
    {
        let patient = context
            .environment
            .patient_context
            .as_deref()
            .ok_or_else(|| ApiError::forbidden("Patient context required"))?;
        let checker = CompartmentChecker::new();
        let mut checked = false;
        for resource in context
            .resource
            .as_ref()
            .map(|r| &r.resource)
            .into_iter()
            .chain(
                context
                    .request
                    .body
                    .as_ref()
                    .filter(|v| v.get("resourceType").is_some()),
            )
        {
            let mut resource = resource.clone();
            normalize_local_references(&mut resource, base_url);
            if !checker.is_in_compartment("Patient", patient, &resource) {
                return Err(ApiError::forbidden(
                    "Resource is outside the authorized Patient compartment",
                ));
            }
            checked = true;
        }
        if checked && (operation.is_instance_level() || operation == FhirOperation::Create) {
            if operation == FhirOperation::HistoryInstance
                && context.request.resource_type != "Patient"
            {
                return Err(ApiError::forbidden(
                    "Instance history requires authorization of every historical resource version",
                ));
            }
            return Ok(());
        }
        return Err(ApiError::forbidden(
            "This interaction requires an authorization-aware Patient compartment query",
        ));
    }
    Err(ApiError::forbidden(
        "The granted scope constraints cannot authorize this interaction",
    ))
}

fn normalize_local_references(value: &mut Value, base_url: &str) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("reference") {
                let base = format!("{}/fhir", base_url.trim_end_matches('/'));
                if let Ok(parsed) = octofhir_core::parse_reference(reference, Some(&base)) {
                    *reference = parsed.to_relative();
                }
            }
            for value in map.values_mut() {
                normalize_local_references(value, base_url);
            }
        }
        Value::Array(items) => {
            for value in items {
                normalize_local_references(value, base_url);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_auth::policy::{ResourceContext, ScopeSummary};
    use octofhir_auth::smart::scopes::SmartScopes;
    use serde_json::json;

    fn context(scope: &str, resource: Value) -> PolicyContext {
        let mut auth = AuthContext::system_anonymous();
        Arc::make_mut(&mut auth.token_claims).scope = scope.into();
        PolicyContextBuilder::new()
            .with_auth_context(&auth)
            .with_request("GET", "/Observation/o1", Default::default(), None)
            .with_resource(resource)
            .with_environment("test".into(), None)
            .with_launch_context(Some("p1".into()), None)
            .build()
            .unwrap()
    }

    use std::sync::Arc;

    #[test]
    fn bundle_query_uris_do_not_become_absolute_request_paths() {
        let auth = AuthContext::system_anonymous();
        for method in ["GET", "PUT", "DELETE"] {
            for url in [
                "Patient?identifier=http://example.org/mrn|123",
                "Patient?identifier=https://example.org/mrn|123",
                "Patient?identifier=https%3A%2F%2Fexample.org%2Fmrn%231%7C123",
            ] {
                let context =
                    entry_context(&auth, &json!({"request":{"method":method,"url":url}})).unwrap();
                assert_eq!(context.request.path, "/fhir/Patient");
                assert_eq!(context.request.resource_type, "Patient");
                assert!(context.request.resource_id.is_none());
                assert!(context.request.query_params["identifier"].starts_with("http"));
            }
        }
        for url in [
            "https://foreign.test/fhir/Patient?identifier=http://example.org/mrn|123",
            "//foreign.test/Patient?identifier=123",
            "Patient?identifier=123#fragment",
        ] {
            assert!(
                entry_context(&auth, &json!({"request":{"method":"PUT","url":url}})).is_err(),
                "{url}"
            );
        }
    }

    #[test]
    fn patient_grant_cannot_cross_compartments_or_resource_types() {
        for reference in [
            "Patient/p2",
            "Practitioner/p1",
            "https://foreign.test/fhir/Patient/p1",
        ] {
            let context = context(
                "patient/Observation.r",
                json!({"resourceType":"Observation","subject":{"reference":reference}}),
            );
            assert!(
                check_resource_scope(&context, "https://local.test").is_err(),
                "{reference}"
            );
        }
        for reference in [
            "Patient/p1",
            "Patient/p1/_history/7",
            "https://local.test/fhir/Patient/p1",
        ] {
            let context = context(
                "patient/Observation.r",
                json!({"resourceType":"Observation","subject":{"reference":reference}}),
            );
            assert!(
                check_resource_scope(&context, "https://local.test").is_ok(),
                "{reference}"
            );
        }
    }

    #[test]
    fn both_existing_and_replacement_must_be_in_patient_compartment() {
        let mut context = context(
            "patient/Observation.u",
            json!({"resourceType":"Observation","subject":{"reference":"Patient/p1"}}),
        );
        context.request.operation = FhirOperation::Update;
        context.request.body =
            Some(json!({"resourceType":"Observation","subject":{"reference":"Patient/p2"}}));
        assert!(check_resource_scope(&context, "https://local.test").is_err());
        context.resource = Some(ResourceContext::from_resource(
            json!({"resourceType":"Observation","subject":{"reference":"Patient/p2"}}),
        ));
        context.request.body =
            Some(json!({"resourceType":"Observation","subject":{"reference":"Patient/p1"}}));
        assert!(check_resource_scope(&context, "https://local.test").is_err());
    }

    #[test]
    fn filtered_grant_cannot_become_unrestricted_access() {
        let mut context = context(
            "system/Observation.r?category=laboratory",
            json!({"resourceType":"Observation","subject":{"reference":"Patient/p1"}}),
        );
        assert!(check_resource_scope(&context, "https://local.test").is_err());
        let scopes = "system/Observation.r?category=laboratory system/Observation.r";
        context.scopes =
            ScopeSummary::from_smart_scopes(scopes, &SmartScopes::parse(scopes).unwrap());
        assert!(check_resource_scope(&context, "https://local.test").is_ok());
    }

    #[test]
    fn search_grants_preserve_union_and_query_semantics() {
        let mut context = context(
            "system/Observation.s?code=lab%7Cone&code=lab%7Ctwo system/Observation.s?_id=other",
            json!({"resourceType":"Observation"}),
        );
        context.request.operation = FhirOperation::Search;
        let visibility = search_visibility(
            &context,
            &crate::compartments::CompartmentRegistry::new(),
            "https://local.test",
        )
        .unwrap();
        let Some(SearchConstraint::Or(grants)) = &visibility.constraint else {
            panic!("Expected union");
        };
        assert_eq!(grants.len(), 2);
        let SearchConstraint::And(predicates) = &grants[0] else {
            panic!("Expected grant predicates");
        };
        let SearchConstraint::Query(query) = &predicates[0] else {
            panic!("Expected query");
        };
        assert_eq!(query["code"], ["lab|one", "lab|two"]);
    }

    #[test]
    fn patient_search_limits_patient_identity_and_rejects_injected_ids() {
        let mut context = context("patient/Patient.s", json!({"resourceType":"Patient"}));
        context.request.resource_type = "Patient".into();
        context.request.operation = FhirOperation::Search;
        let compartments = crate::compartments::CompartmentRegistry::new();
        let visibility = search_visibility(&context, &compartments, "https://local.test").unwrap();
        let Some(SearchConstraint::Or(grants)) = visibility.constraint else {
            panic!("Expected union");
        };
        let SearchConstraint::And(predicates) = &grants[0] else {
            panic!("Expected intersection");
        };
        let SearchConstraint::Query(query) = &predicates[0] else {
            panic!("Expected ID restriction");
        };
        assert_eq!(query["_id"], ["p1"]);
        for invalid in ["p1,p2", "Patient/p1", "", "p1&active=true"] {
            context.environment.patient_context = Some(invalid.into());
            assert!(search_visibility(&context, &compartments, "https://local.test").is_err());
        }
    }

    #[test]
    fn broad_development_grant_remains_unrestricted() {
        let mut context = context("system/*.cruds", json!({"resourceType":"Observation"}));
        context.request.operation = FhirOperation::Search;
        let visibility = search_visibility(
            &context,
            &crate::compartments::CompartmentRegistry::new(),
            "https://local.test",
        )
        .unwrap();
        assert!(visibility.constraint.is_none());
    }

    #[test]
    fn query_visibility_marker_is_limited_to_handlers_that_apply_it() {
        for path in [
            "/fhir/Observation",
            "/fhir/Observation/_search",
            "/Observation",
        ] {
            assert!(is_type_search_path(path, "Observation"), "{path}");
        }
        for path in [
            "/fhir/Patient/p1/Observation",
            "/fhir/Observation/_history",
            "/fhir/Observation/$export",
            "/fhir",
        ] {
            assert!(!is_type_search_path(path, "Observation"), "{path}");
        }
    }
}
