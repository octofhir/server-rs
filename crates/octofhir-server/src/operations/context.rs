//! Trusted identity and resource access for a named operation invocation.

use std::{collections::HashMap, sync::Arc};

use axum::{extract::FromRequestParts, http::request::Parts};
use octofhir_api::ApiError;
use octofhir_auth::{
    middleware::AuthContext,
    policy::{AccessDecision, PolicyContext, PolicyContextBuilder, ResourceContext},
    smart::scopes::FhirOperation,
};
use octofhir_storage::{
    RawStoredResource, SearchConstraint, SearchParams, SearchResult, StoredResource,
};

use super::OperationError;
use crate::server::AppState;

/// Captured from request extensions, never from operation Parameters. Cloning
/// this context preserves the caller when an operation delegates work.
#[derive(Clone)]
pub struct OperationContext {
    auth: Option<Arc<AuthContext>>,
    request_id: String,
    if_match: Option<String>,
}

impl FromRequestParts<AppState> for OperationContext {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self, ApiError> {
        Ok(Self {
            auth: parts.extensions.get::<Arc<AuthContext>>().cloned(),
            request_id: parts
                .headers
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("operation")
                .into(),
            if_match: parts
                .headers
                .get(axum::http::header::IF_MATCH)
                .map(|value| value.to_str().map(String::from))
                .transpose()
                .map_err(|_| ApiError::bad_request("Invalid If-Match header"))?,
        })
    }
}

impl OperationContext {
    pub fn auth(&self) -> Option<&AuthContext> {
        self.auth.as_deref()
    }

    pub(super) fn policy_context(
        &self,
        state: &AppState,
        operation: FhirOperation,
        resource_type: &str,
        id: Option<&str>,
        params: &SearchParams,
    ) -> Result<Option<PolicyContext>, OperationError> {
        if state.config.auth.policy.anonymous_access {
            return Ok(None);
        }
        let auth = self
            .auth()
            .ok_or_else(|| ApiError::unauthorized("Authentication required"))?;
        let path = match id {
            Some(id) => format!("/{resource_type}/{id}"),
            None => format!("/{resource_type}"),
        };
        let query = params
            .parameters
            .iter()
            .filter_map(|(key, values)| values.last().map(|value| (key.clone(), value.clone())))
            .collect::<HashMap<_, _>>();
        let mut context = PolicyContextBuilder::new()
            .with_auth_context(auth)
            .with_request(
                match operation {
                    FhirOperation::Create => "POST",
                    FhirOperation::Update => "PUT",
                    FhirOperation::Delete => "DELETE",
                    _ => "GET",
                },
                &path,
                query,
                None,
            )
            .with_environment(self.request_id.clone(), None)
            .with_launch_context(auth.patient.clone(), auth.encounter.clone())
            .build()
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        context.request.operation = operation;
        context.request.path = format!("/fhir{path}");
        if !context.scopes.parsed.permits(
            resource_type,
            operation,
            context.environment.patient_context.as_deref(),
        ) {
            return Err(
                ApiError::forbidden("Insufficient scope for operation resource access").into(),
            );
        }
        Ok(Some(context))
    }

    pub(super) async fn evaluate(
        state: &AppState,
        context: &PolicyContext,
    ) -> Result<(), OperationError> {
        match state.policy_evaluator.evaluate(context).await {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny(reason) => Err(ApiError::forbidden(reason.message).into()),
            AccessDecision::Abstain => {
                Err(ApiError::forbidden("No policy allows operation resource access").into())
            }
        }
    }

    pub(super) fn if_match(&self) -> Option<&str> {
        self.if_match.as_deref()
    }

    async fn query(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, OperationError> {
        let cfg = state.search_config.config();
        let result = octofhir_db_postgres::queries::execute_search_raw_with_terminology_options(
            &state.read_db_pool,
            resource_type,
            params,
            Some(&cfg.registry),
            state.query_cache.as_deref(),
            state.terminology_provider.as_ref(),
            octofhir_db_postgres::queries::RawSearchOptions {
                include_limits: state.config.search.include_limits,
                unknown_param_handling: Some(octofhir_search::UnknownParamHandling::Strict),
                max_valueset_expansion: Some(state.config.search.max_valueset_expansion),
                ..Default::default()
            },
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        Ok(SearchResult {
            entries: result
                .entries
                .into_iter()
                .map(stored_resource)
                .collect::<Result<_, _>>()?,
            total: result.total,
            has_more: result.has_more,
        })
    }

    /// Fetch the authorized snapshot with grant predicates in SQL. The resource
    /// policy then evaluates that same snapshot; there is no separate read of a
    /// resource whose compartment or grant attributes might have changed.
    pub async fn read(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, OperationError> {
        self.read_for(state, resource_type, id, FhirOperation::Read, false)
            .await
    }

    /// Reference validation cannot conclude that an invisible target is
    /// nonexistent. Preserve that uncertainty as a masked access error.
    pub(crate) async fn read_reference(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, OperationError> {
        self.read_for(state, resource_type, id, FhirOperation::Read, true)
            .await
    }

    /// Use the requested historical snapshot, never substitute the current
    /// version. Historical SQL grant predicates need their own search source.
    pub(crate) async fn vread_reference(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<StoredResource>, OperationError> {
        octofhir_core::validate_id(id).map_err(|error| ApiError::bad_request(error.to_string()))?;
        octofhir_core::validate_id(version)
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        let params = SearchParams::new().with_param("_id", id);
        let mut context = self.policy_context(
            state,
            FhirOperation::VRead,
            resource_type,
            Some(id),
            &params,
        )?;
        if let Some(context) = &mut context {
            context.request.path = format!("/fhir/{resource_type}/{id}/_history/{version}");
            if crate::authorization::query_visibility(
                context,
                &state.compartment_registry,
                &state.base_url,
            )?
            .is_some()
            {
                return Err(ApiError::NotImplemented(
                    "Historical reference validation with constrained grants is not implemented"
                        .into(),
                )
                .into());
            }
        }
        let stored = state
            .storage
            .vread(resource_type, id, version)
            .await
            .map_err(crate::handlers::map_storage_error)?;
        if let (Some(context), Some(stored)) = (&mut context, &stored) {
            context.resource = Some(ResourceContext::from_resource(stored.resource.clone()));
            Self::evaluate(state, context).await?;
        }
        Ok(stored)
    }

    /// A write operation may load its target without acquiring read permission.
    /// The current and resulting resource are checked before committing update.
    pub async fn read_for_update(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, OperationError> {
        self.read_for(state, resource_type, id, FhirOperation::Update, false)
            .await
    }

    async fn read_for(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        operation: FhirOperation,
        error_on_hidden_reference: bool,
    ) -> Result<Option<StoredResource>, OperationError> {
        octofhir_core::validate_id(id).map_err(|error| ApiError::bad_request(error.to_string()))?;
        let mut params = SearchParams::new().with_count(1).with_param("_id", id);
        let Some(mut context) =
            self.policy_context(state, operation, resource_type, Some(id), &params)?
        else {
            return state
                .storage
                .read(resource_type, id)
                .await
                .map_err(crate::handlers::map_storage_error)
                .map_err(Into::into);
        };
        params.constraint = crate::authorization::query_visibility(
            &context,
            &state.compartment_registry,
            &state.base_url,
        )?;
        let stored = self
            .query(state, resource_type, &params)
            .await?
            .entries
            .into_iter()
            .next();
        if error_on_hidden_reference && params.constraint.is_some() && stored.is_none() {
            return Err(ApiError::not_found("Referenced resource is unavailable").into());
        }
        if operation == FhirOperation::Read
            && let Some(stored) = &stored
        {
            context.resource = Some(ResourceContext::from_resource(stored.resource.clone()));
            Self::evaluate(state, &context).await?;
        }
        Ok(stored)
    }

    /// Validate a metadata edit after policy checks, pin the selected version,
    /// and authorize the new grant attributes inside the write transaction.
    pub(super) async fn update_metadata(
        &self,
        state: &AppState,
        previous: &StoredResource,
        resource: &serde_json::Value,
        metadata: &serde_json::Value,
    ) -> Result<StoredResource, OperationError> {
        let mut params = SearchParams::new()
            .with_count(1)
            .with_param("_id", &previous.id);
        let mut context = self.policy_context(
            state,
            FhirOperation::Update,
            &previous.resource_type,
            Some(&previous.id),
            &params,
        )?;
        if let Some(context) = &mut context {
            params.constraint = crate::authorization::query_visibility(
                context,
                &state.compartment_registry,
                &state.base_url,
            )?;
            context.request.body = Some(resource.clone());
            context.resource = Some(ResourceContext::from_resource(previous.resource.clone()));
            Self::evaluate(state, context).await?;
            context.resource = Some(ResourceContext::from_resource(resource.clone()));
            Self::evaluate(state, context).await?;
        }
        super::meta::validate_metadata_update(state, metadata, resource).await?;
        let cfg = state.search_config.config();
        let types = [&*previous.resource_type];
        let params = octofhir_db_postgres::queries::search::prepare_system_search_params(
            &types,
            &params,
            &cfg.registry,
            state.terminology_provider.as_ref(),
            state.config.search.max_valueset_expansion,
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        let mut tx = state
            .storage
            .begin_transaction()
            .await
            .map_err(crate::handlers::map_storage_error)?;
        let result = async {
            let updated = tx
                .update(resource, Some(&previous.version_id))
                .await
                .map_err(crate::handlers::map_storage_error)?;
            if context.is_some() {
                let visible = tx
                    .search_system_raw(&types, &params, state.config.search.include_limits, false)
                    .await
                    .map_err(crate::handlers::map_storage_error)?;
                if !visible
                    .entries
                    .iter()
                    .any(|entry| entry.id == updated.id && entry.version_id == updated.version_id)
                {
                    return Err(ApiError::forbidden(
                        "Updated resource is outside the granted scope",
                    ));
                }
            }
            Ok(updated)
        }
        .await;
        match result {
            Ok(updated) => {
                tx.commit()
                    .await
                    .map_err(crate::handlers::map_storage_error)?;
                if let Some(cache) = &state.resource_cache {
                    cache.invalidate(&updated.resource_type, &updated.id).await;
                }
                Ok(updated)
            }
            Err(error) => {
                tx.rollback()
                    .await
                    .map_err(crate::handlers::map_storage_error)?;
                Err(error.into())
            }
        }
    }

    /// Enumerated resources, pages and totals share the caller's trusted search
    /// predicates, including patient membership and filtered SMART grants.
    pub async fn search(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, OperationError> {
        let mut params = params.clone();
        if let Some(context) =
            self.policy_context(state, FhirOperation::Search, resource_type, None, &params)?
        {
            Self::evaluate(state, &context).await?;
            if let Some(grant) = crate::authorization::query_visibility(
                &context,
                &state.compartment_registry,
                &state.base_url,
            )? {
                params.constraint = Some(match params.constraint.take() {
                    Some(existing) => SearchConstraint::And(vec![existing, grant]),
                    None => grant,
                });
            }
        }
        self.query(state, resource_type, &params).await
    }
}

fn stored_resource(raw: RawStoredResource) -> Result<StoredResource, OperationError> {
    Ok(StoredResource {
        id: raw.id,
        version_id: raw.version_id,
        resource_type: raw.resource_type,
        resource: serde_json::from_str(&raw.resource_json)
            .map_err(|error| OperationError::Internal(error.to_string()))?,
        last_updated: raw.last_updated,
        created_at: raw.created_at,
    })
}
