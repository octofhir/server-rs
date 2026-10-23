//! Mode-specific $validate checks. All speculative changes are rolled back;
//! no postprocessing, cache invalidation, or committed resource events run.

use octofhir_api::ApiError;
use octofhir_storage::{SearchParams, StorageError};
use serde_json::Value;

use super::{OperationContext, OperationError, validate::Issue};
pub(super) use crate::write::WriteMode;
use crate::{
    mapping::IdPolicy,
    server::AppState,
    write::{PreparedWrite, WriteError, parse_if_match},
};

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

pub(super) async fn check_write(
    state: &AppState,
    invocation: &OperationContext,
    mode: WriteMode,
    resource_type: &str,
    id: Option<&str>,
    candidate: Option<&Value>,
    references: &crate::handlers::LocalReferenceSet,
) -> Result<Option<Issue>, OperationError> {
    let id = if mode == WriteMode::Create { None } else { id };
    let params = SearchParams::new();
    let policy = invocation.policy_context(state, mode.operation(), resource_type, id, &params)?;
    let if_match = if mode != WriteMode::Create {
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
    let write = PreparedWrite::prepare(state, mode, resource_type, id, policy).await?;
    // The same write algorithm as primary CRUD runs with rollback, discarding
    // history/index changes and EventedTransaction's pending events.
    match write
        .run(
            state,
            prepared.as_ref(),
            references,
            if_match.as_deref(),
            false,
        )
        .await
    {
        Ok(_) => Ok(None),
        Err(WriteError::Storage(error)) => storage_failure(error).map(Some),
        Err(WriteError::Api(ApiError::PreconditionFailed(_))) => Ok(Some(Issue::error(
            "conflict",
            "Resource does not exist but If-Match was provided",
        ))),
        Err(WriteError::Api(error)) => Err(error.into()),
        Err(WriteError::MissingReferences(_)) => Ok(Some(Issue::error(
            "not-found",
            "A required local reference is unavailable for this write",
        ))),
    }
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
