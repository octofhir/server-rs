//! `$expunge`: permanently removes one resource and all of its versions.
//!
//! A normal DELETE keeps history; this is for data that must not stay, such as a record
//! created for the wrong person. It works on one instance and needs an administrator,
//! and `dryRun` shows what would go. Nothing here follows references.

use async_trait::async_trait;
use octofhir_api::ApiError;
use octofhir_auth::smart::scopes::{ResourceType, ScopeContext, SmartScopes};
use serde_json::{Value, json};

use super::OperationContext;
use super::handler::{OperationError, OperationHandler, OperationLevel};
use crate::server::AppState;

/// Roles that administer this server.
const ADMIN_ROLES: [&str; 2] = ["admin", "superuser"];

pub struct ExpungeOperation;

fn require_administrator(
    state: &AppState,
    context: &OperationContext,
) -> Result<(), OperationError> {
    let Some(auth) = context.auth() else {
        return if state.config.auth.policy.anonymous_access {
            Ok(())
        } else {
            Err(ApiError::unauthorized("Authentication required").into())
        };
    };
    let administrator = auth.user.as_ref().is_some_and(|user| {
        user.roles
            .iter()
            .any(|role| ADMIN_ROLES.contains(&role.as_str()))
    });
    // A backend service qualifies with delete on every resource type.
    let system_wide = SmartScopes::parse(&auth.token_claims.scope).is_ok_and(|scopes| {
        scopes.resource_scopes.iter().any(|scope| {
            scope.context == ScopeContext::System
                && scope.resource_type == ResourceType::Wildcard
                && scope.filter.is_none()
                && scope.permissions.delete
        })
    });
    if administrator || system_wide {
        Ok(())
    } else {
        Err(ApiError::forbidden("$expunge requires an administrator").into())
    }
}

fn dry_run(params: &Value) -> Result<bool, OperationError> {
    let found = params["parameter"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|parameter| parameter["name"] == "dryRun");
    match found {
        None => Ok(false),
        Some(parameter) => match (&parameter["valueBoolean"], &parameter["valueString"]) {
            (Value::Bool(flag), _) => Ok(*flag),
            (_, Value::String(text)) if text == "true" => Ok(true),
            (_, Value::String(text)) if text == "false" => Ok(false),
            _ => Err(OperationError::InvalidParameters(
                "dryRun must be boolean".into(),
            )),
        },
    }
}

#[async_trait]
impl OperationHandler for ExpungeOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        level == OperationLevel::Instance
    }

    fn code(&self) -> &str {
        "expunge"
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        require_administrator(state, context)?;
        if !state.resource_type_set.load().contains(resource_type) {
            return Err(OperationError::NotFound(format!(
                "Unknown resource type {resource_type}"
            )));
        }
        // A GET cannot remove anything, whatever its parameters say.
        let dry_run = dry_run(params)? || context.is_get();
        let (live, versions) = octofhir_db_postgres::queries::crud::expunge(
            &state.db_pool,
            resource_type,
            id,
            dry_run,
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        if versions == 0 {
            return Err(OperationError::NotFound(format!(
                "{resource_type}/{id} not found"
            )));
        }
        if !dry_run {
            if let Some(cache) = &state.resource_cache {
                cache.invalidate(resource_type, id).await;
            }
            tracing::warn!(resource_type, id, versions, "Resource expunged");
        }
        Ok(json!({"resourceType": "Parameters", "parameter": [
            {"name": "dryRun", "valueBoolean": dry_run},
            {"name": "live", "valueBoolean": live},
            {"name": "versions", "valueInteger": versions},
        ]}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_is_read_from_parameters_and_query() {
        let parameters = |parameter: Value| json!({"parameter": [parameter]});
        assert!(!dry_run(&json!({})).unwrap());
        assert!(dry_run(&parameters(json!({"name":"dryRun","valueBoolean":true}))).unwrap());
        assert!(dry_run(&parameters(json!({"name":"dryRun","valueString":"true"}))).unwrap());
        assert!(!dry_run(&parameters(json!({"name":"dryRun","valueBoolean":false}))).unwrap());
        assert!(dry_run(&parameters(json!({"name":"dryRun","valueString":"maybe"}))).is_err());
    }
}
