//! Application search policies for exact native snapshots and inline traversal.

use std::collections::{BTreeSet, HashMap};

use octofhir_api::ApiError;
use octofhir_auth::policy::engine::OwnedPreparedPolicyEvaluation;
use octofhir_auth::policy::{AccessDecision, PolicyContext, ResourceContext};
use octofhir_auth::smart::scopes::FhirOperation;
use octofhir_db_postgres::queries::RawSearchOptions;
use octofhir_storage::{
    RawSearchAuthorization, RawSearchResult, RawStoredResource, SearchParams, StorageError,
};

use super::AuthorizedSearch;
use crate::server::AppState;

struct TypePolicies {
    evaluation: OwnedPreparedPolicyEvaluation,
    main: Vec<PolicyContext>,
    included: Option<PolicyContext>,
}

/// Holds prepared policies before a Bundle/read connection is acquired. Main
/// rows and inline rows retain their distinct request contracts.
pub(crate) struct PreparedSnapshotSearch {
    types: HashMap<String, TypePolicies>,
    main_required: bool,
    included_required: bool,
}

impl AuthorizedSearch {
    pub(crate) async fn prepare_snapshots(
        &self,
        state: &AppState,
        requested_types: &[&str],
        params: &SearchParams,
    ) -> Result<PreparedSnapshotSearch, ApiError> {
        let mut names: BTreeSet<String> =
            requested_types.iter().map(|name| (*name).into()).collect();
        if let Some(visibility) = &params.included_visibility {
            names.extend(visibility.constraints.keys().cloned());
        }
        let mut types = HashMap::new();
        let mut main_required = false;
        let mut included_required = false;
        for name in names {
            let evaluation = state
                .policy_evaluator
                .prepare_owned(&name)
                .await
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let mut main = Vec::new();
            if requested_types.contains(&name.as_str())
                && params
                    .type_constraints
                    .as_ref()
                    .is_none_or(|grants| grants.contains_key(&name))
            {
                let mut context = self.context.clone();
                context.resource = None;
                context.request.resource_type = name.clone();
                context.request.resource_id = None;
                context.request.query_params = params
                    .parameters
                    .iter()
                    .filter_map(|(key, values)| {
                        values.last().map(|value| (key.clone(), value.clone()))
                    })
                    .collect();
                main.push(context.clone());
                if self.deferred {
                    context.request.operation = FhirOperation::Search;
                    context.request.method = "GET".into();
                    context.request.path = format!("/fhir/{name}");
                    context.request.compartment_type = None;
                    context.request.compartment_id = None;
                    context.request.body = None;
                    main.push(context);
                }
                main_required |= main.iter().any(|context| {
                    evaluation.requires_snapshots(context)
                        || !matches!(
                            evaluation.evaluate_admission(context),
                            AccessDecision::Allow
                        )
                });
            }
            let included = params
                .included_visibility
                .as_ref()
                .filter(|visibility| visibility.constraints.contains_key(&name))
                .map(|_| {
                    let mut context = self.context.clone();
                    context.resource = None;
                    context.request.resource_type = name.clone();
                    context.request.resource_id = None;
                    context.request.compartment_type = None;
                    context.request.compartment_id = None;
                    context.request.body = None;
                    context.request.query_params.clear();
                    context.request.operation = FhirOperation::Search;
                    context.request.operation_id = None;
                    context.request.method = "GET".into();
                    context.request.path = format!("/fhir/{name}");
                    context
                });
            if let Some(context) = &included {
                included_required |= evaluation.requires_snapshots(context)
                    || !matches!(
                        evaluation.evaluate_admission(context),
                        AccessDecision::Allow
                    );
            }
            types.insert(
                name,
                TypePolicies {
                    evaluation,
                    main,
                    included,
                },
            );
        }
        Ok(PreparedSnapshotSearch {
            types,
            main_required,
            included_required,
        })
    }
}

impl PreparedSnapshotSearch {
    pub(crate) fn required(&self) -> bool {
        self.main_required || self.included_required
    }

    fn allows(&self, row: &RawStoredResource, included: bool) -> Result<bool, StorageError> {
        let Some(prepared) = self.types.get(&row.resource_type) else {
            return Ok(false);
        };
        let contexts = if included {
            prepared.included.as_slice()
        } else {
            prepared.main.as_slice()
        };
        if contexts.is_empty() {
            return Ok(false);
        }
        let body: serde_json::Value =
            serde_json::from_str(&row.resource_json).map_err(|error| {
                StorageError::internal(format!("Invalid saved resource JSON: {error}"))
            })?;
        let resource = ResourceContext::from_resource(body);
        for context in contexts {
            let mut context = context.clone();
            context.resource = Some(resource.clone());
            match prepared.evaluation.evaluate(&context) {
                AccessDecision::Allow => {}
                AccessDecision::Deny(reason)
                    if matches!(
                        reason.code.as_str(),
                        "policy-error" | "script-error" | "script-timeout" | "script-memory-limit"
                    ) =>
                {
                    tracing::error!(policy_id = ?reason.policy_id, code = %reason.code, "Search snapshot policy failed");
                    return Err(StorageError::internal("Resource policy evaluation failed"));
                }
                AccessDecision::Deny(_) | AccessDecision::Abstain => return Ok(false),
            }
        }
        Ok(true)
    }

    pub(crate) async fn execute(
        &self,
        state: &AppState,
        types: &[&str],
        params: &SearchParams,
        options: RawSearchOptions,
    ) -> Result<RawSearchResult, ApiError> {
        let main = |row: &RawStoredResource| self.allows(row, false);
        let included = |row: &RawStoredResource| self.allows(row, true);
        tokio::time::timeout(
            state.config.read_timeout(),
            octofhir_db_postgres::queries::search::execute_system_search_raw_authorized(
                &state.db_pool,
                types,
                params,
                &state.search_config.config().registry,
                state.terminology_provider.as_ref(),
                options,
                RawSearchAuthorization {
                    main: self.main_required.then_some(&main),
                    included: self.included_required.then_some(&included),
                },
            ),
        )
        .await
        .map_err(|_| {
            ApiError::internal("Search authorization exceeded the configured read timeout")
        })?
        .map_err(crate::handlers::map_storage_error)
    }

    pub(crate) async fn execute_with_tx(
        &self,
        state: &AppState,
        tx: &dyn octofhir_storage::Transaction,
        types: &[&str],
        params: &SearchParams,
        strict: bool,
    ) -> Result<RawSearchResult, ApiError> {
        let main = |row: &RawStoredResource| self.allows(row, false);
        let included = |row: &RawStoredResource| self.allows(row, true);
        tokio::time::timeout(
            state.config.read_timeout(),
            tx.search_system_raw_authorized(
                types,
                params,
                state.config.search.include_limits,
                strict,
                RawSearchAuthorization {
                    main: self.main_required.then_some(&main),
                    included: self.included_required.then_some(&included),
                },
            ),
        )
        .await
        .map_err(|_| {
            ApiError::internal("Search authorization exceeded the configured read timeout")
        })?
        .map_err(crate::handlers::map_storage_error)
    }
}
