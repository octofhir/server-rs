//! Mode-specific $validate checks. All speculative changes are rolled back;
//! no postprocessing, cache invalidation, or committed resource events run.

use octofhir_api::ApiError;
use octofhir_auth::{policy::ResourceContext, smart::scopes::FhirOperation};
use octofhir_storage::{SearchParams, StorageError};
use serde_json::Value;

use super::{OperationContext, OperationError, validate::Issue};
use crate::{mapping::IdPolicy, server::AppState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WriteMode {
    Create,
    Update,
    Delete,
}

impl WriteMode {
    pub(super) fn operation(self) -> FhirOperation {
        match self {
            Self::Create => FhirOperation::Create,
            Self::Update => FhirOperation::Update,
            Self::Delete => FhirOperation::Delete,
        }
    }
}

fn storage_failure(error: StorageError) -> Result<Issue, OperationError> {
    let (code, message) = match error {
        StorageError::AlreadyExists { .. } => {
            ("duplicate", "The write conflicts with an existing resource")
        }
        StorageError::VersionConflict { .. } => {
            ("conflict", "The write version does not match the resource")
        }
        StorageError::NotFound { .. } | StorageError::Deleted { .. } => {
            ("not-found", "The resource is unavailable for this write")
        }
        StorageError::InvalidResource { .. } => {
            ("invalid", "The storage write would reject the resource")
        }
        error => return Err(crate::handlers::map_storage_error(error).into()),
    };
    Ok(Issue::error(code, message))
}

fn parse_if_match(value: &str) -> Result<String, OperationError> {
    let value = value.trim();
    let version = value
        .strip_prefix("W/\"")
        .or_else(|| value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| ApiError::bad_request("If-Match requires a single version ETag"))?;
    octofhir_core::validate_id(version)
        .map_err(|_| ApiError::bad_request("Invalid If-Match version ETag"))?;
    Ok(version.to_owned())
}

pub(super) async fn check_write(
    state: &AppState,
    invocation: &OperationContext,
    mode: WriteMode,
    resource_type: &str,
    id: Option<&str>,
    candidate: Option<&Value>,
) -> Result<Option<Issue>, OperationError> {
    let id = if mode == WriteMode::Create { None } else { id };
    let mut params = SearchParams::new().with_count(1);
    if let Some(id) = id {
        octofhir_core::validate_id(id).map_err(|e| ApiError::bad_request(e.to_string()))?;
        params = params.with_param("_id", id);
    }
    let mut policy =
        invocation.policy_context(state, mode.operation(), resource_type, id, &params)?;
    if let Some(policy) = &policy {
        params.constraint = crate::authorization::query_visibility(
            policy,
            &state.compartment_registry,
            &state.base_url,
        )?;
    }
    // System-search preparation moves constraints into per-type predicates.
    // Retain whether access was restricted before that transformation.
    let constrained = params.constraint.is_some();
    let if_match = if mode == WriteMode::Update {
        invocation.if_match().map(parse_if_match).transpose()?
    } else {
        None
    };
    let mut prepared = candidate.cloned();
    if let Some(resource) = &mut prepared {
        let identity = if mode == WriteMode::Update {
            IdPolicy::Update {
                path_id: id
                    .ok_or_else(|| ApiError::bad_request("Update requires an instance"))?
                    .into(),
            }
        } else {
            IdPolicy::Create
        };
        if let Err(error) =
            crate::mapping::validate_payload_structure(resource_type, resource, &identity)
        {
            return Ok(Some(Issue::error("invalid", &error)));
        }
        crate::handlers::preprocess_payload(resource_type, resource).await?;
    }
    // Expand trusted terminology predicates before checking out the transaction.
    let cfg = state.search_config.config();
    let types = [resource_type];
    let mut params = octofhir_db_postgres::queries::search::prepare_system_search_params(
        &types,
        &params,
        &cfg.registry,
        state.terminology_provider.as_ref(),
        state.config.search.max_valueset_expansion,
    )
    .await
    .map_err(crate::handlers::map_storage_error)?;
    if state.storage.backend_name() != "postgres" {
        return Err(
            ApiError::not_implemented("Write validation requires transaction row locking").into(),
        );
    }
    let mut tx = state
        .storage
        .begin_transaction()
        .await
        .map_err(crate::handlers::map_storage_error)?;
    let result = async {
        let previous = if let Some(id) = id {
            match tx.read_for_update(resource_type, id).await {
                Ok(resource) => resource,
                Err(StorageError::Deleted { .. }) if constrained => {
                    return Err(ApiError::not_found(
                        "Resource is unavailable for write validation",
                    )
                    .into());
                }
                Err(error) => return storage_failure(error).map(Some),
            }
        } else {
            None
        };
        if id.is_some() && constrained {
            let visible = tx
                .search_system_raw(&types, &params, state.config.search.include_limits, false)
                .await
                .map_err(crate::handlers::map_storage_error)?;
            if !previous.as_ref().is_some_and(|previous| {
                visible
                    .entries
                    .iter()
                    .any(|row| row.id == previous.id && row.version_id == previous.version_id)
            }) {
                return Err(
                    ApiError::not_found("Resource is unavailable for write validation").into(),
                );
            }
        }
        if let Some(policy) = &mut policy {
            policy.request.body = candidate.cloned();
            policy.resource = previous
                .as_ref()
                .map(|row| ResourceContext::from_resource(row.resource.clone()));
            // The primary HTTP write surface currently rejects filtered grants.
            // Preserve that constraint: a dry run must never promise a write
            // that the corresponding HTTP interaction would deny.
            crate::authorization::check_resource_scope(policy, &state.base_url)?;
            OperationContext::evaluate(state, policy).await?;
            if let Some(candidate) = candidate {
                policy.resource = Some(ResourceContext::from_resource(candidate.clone()));
                OperationContext::evaluate(state, policy).await?;
            }
        }
        if resource_type == "AccessPolicy" && id == Some(crate::bootstrap::ADMIN_ACCESS_POLICY_ID) {
            return Ok(Some(Issue::error(
                "business-rule",
                "The default admin access policy is protected",
            )));
        }
        if mode == WriteMode::Delete {
            if let Err(error) = tx
                .delete(
                    resource_type,
                    id.ok_or_else(|| ApiError::bad_request("Delete requires an instance"))?,
                )
                .await
            {
                return storage_failure(error).map(Some);
            }
            return Ok(None);
        }
        let candidate = prepared
            .as_ref()
            .ok_or_else(|| ApiError::bad_request("Resource required"))?;
        let write = match mode {
            WriteMode::Create => tx.create(candidate).await,
            WriteMode::Update if previous.is_none() && if_match.is_some() => {
                return Ok(Some(Issue::error(
                    "conflict",
                    "Resource does not exist but If-Match was provided",
                )));
            }
            WriteMode::Update if previous.is_none() => tx.create(candidate).await,
            WriteMode::Update => {
                tx.update(
                    candidate,
                    if_match
                        .as_deref()
                        .or_else(|| previous.as_ref().map(|row| row.version_id.as_str())),
                )
                .await
            }
            WriteMode::Delete => unreachable!(),
        };
        let stored = match write {
            Ok(stored) => stored,
            Err(error) => return storage_failure(error).map(Some),
        };
        if constrained {
            params
                .parameters
                .insert("_id".into(), vec![stored.id.clone()]);
            let visible = tx
                .search_system_raw(&types, &params, state.config.search.include_limits, false)
                .await
                .map_err(crate::handlers::map_storage_error)?;
            if !visible
                .entries
                .iter()
                .any(|row| row.id == stored.id && row.version_id == stored.version_id)
            {
                return Err(
                    ApiError::forbidden("Proposed resource is outside the granted scope").into(),
                );
            }
        }
        Ok(None)
    }
    .await;
    // This also discards EventedTransaction's pending events. The transaction
    // archives/index changes are rolled back even when a check failed.
    tx.rollback()
        .await
        .map_err(crate::handlers::map_storage_error)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_etag_requires_one_valid_version() {
        for header in ["W/\"42\"", "\"42\"", " W/\"42\" "] {
            assert_eq!(parse_if_match(header).unwrap(), "42");
        }
        for header in ["42", "*", "W/\"\"", "\"1\", \"2\"", "W/\"a/b\""] {
            assert!(parse_if_match(header).is_err(), "{header}");
        }
    }
}
