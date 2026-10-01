//! Authorization of interactions inside FHIR convenience wrappers.

use octofhir_api::ApiError;
use octofhir_auth::middleware::AuthContext;
use octofhir_auth::policy::compartment::CompartmentChecker;
use octofhir_auth::policy::{AccessDecision, PolicyContext, PolicyContextBuilder};
use octofhir_auth::smart::scopes::{FhirOperation, ScopeContext};
use serde_json::Value;

use crate::server::AppState;

pub(crate) async fn hydrate_scope_context(
    state: &AppState,
    request: &mut axum::http::Request<axum::body::Body>,
    context: &mut PolicyContext,
) -> Result<(), ApiError> {
    let operation = context.request.operation;
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

pub(crate) async fn authorize_entry(
    state: &AppState,
    auth: &AuthContext,
    entry: &Value,
) -> Result<(), ApiError> {
    if state.config.auth.policy.anonymous_access {
        return Ok(());
    }
    let request = &entry["request"];
    let method = request["method"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.method"))?;
    let url = request["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing request.url"))?;
    // Bundle request URLs are relative to this service, not arbitrary hosts.
    if url.contains("://") || url.starts_with("//") || url.contains('#') {
        return Err(ApiError::bad_request(
            "Bundle request.url must be relative to the FHIR base",
        ));
    }
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
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
        AccessDecision::Allow => check_resource_scope(&context, &state.base_url),
        AccessDecision::Deny(reason) => Err(ApiError::forbidden(reason.message)),
        AccessDecision::Abstain => Err(ApiError::forbidden("No policy allows this Bundle entry")),
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
                let prefix = format!("{}/fhir/", base_url.trim_end_matches('/'));
                if let Some(relative) = reference.strip_prefix(&prefix) {
                    *reference = relative.to_string();
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
}
