//! Access control for GraphQL resolvers.
//!
//! This module provides access control integration for GraphQL operations,
//! mapping GraphQL fields to FHIR operations and evaluating access policies.

use std::collections::HashMap;

use async_graphql::{ErrorExtensions, Value};
use octofhir_auth::policy::context::{
    ClientIdentity, ClientType, EnvironmentContext, PolicyContext, RequestContext, ScopeSummary,
    UserIdentity,
};
use octofhir_auth::policy::engine::{AccessDecision, DenyReason};
use octofhir_auth::smart::scopes::{FhirOperation, SmartScopes};
use time::OffsetDateTime;
use tracing::{debug, trace, warn};

use crate::context::GraphQLContext;

// =============================================================================
// Operation Mapping
// =============================================================================

/// Maps a GraphQL query/mutation field name to a FHIR operation.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(map_graphql_to_fhir_operation("Patient"), Some(FhirOperation::Read));
/// assert_eq!(map_graphql_to_fhir_operation("PatientList"), Some(FhirOperation::Search));
/// assert_eq!(map_graphql_to_fhir_operation("PatientCreate"), Some(FhirOperation::Create));
/// ```
#[must_use]
pub fn map_graphql_to_fhir_operation(field_name: &str) -> Option<FhirOperation> {
    // Check for mutation suffixes first (more specific)
    if field_name.ends_with("Create") {
        return Some(FhirOperation::Create);
    }
    if field_name.ends_with("Update") {
        return Some(FhirOperation::Update);
    }
    if field_name.ends_with("Delete") {
        return Some(FhirOperation::Delete);
    }

    // Check for query patterns
    if field_name.ends_with("List") || field_name.ends_with("Connection") {
        return Some(FhirOperation::Search);
    }

    // Single resource read (e.g., "Patient", "Observation")
    // This is a simple heuristic - resource type names start with uppercase
    if field_name.chars().next().is_some_and(|c| c.is_uppercase()) && !field_name.contains('_') {
        return Some(FhirOperation::Read);
    }

    // Special fields
    match field_name {
        "_health" | "_version" | "__schema" | "__type" => None, // No authorization needed
        _ => {
            // Could be a reverse reference field (e.g., "ObservationList_subject")
            if field_name.contains("List_") {
                Some(FhirOperation::Search)
            } else {
                None
            }
        }
    }
}

/// Extracts the resource type from a GraphQL field name.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(extract_resource_type("Patient"), Some("Patient"));
/// assert_eq!(extract_resource_type("PatientList"), Some("Patient"));
/// assert_eq!(extract_resource_type("PatientCreate"), Some("Patient"));
/// assert_eq!(extract_resource_type("ObservationList_subject"), Some("Observation"));
/// ```
#[must_use]
pub fn extract_resource_type(field_name: &str) -> Option<String> {
    // Handle mutations
    for suffix in ["Create", "Update", "Delete"] {
        if let Some(resource_type) = field_name.strip_suffix(suffix) {
            return Some(resource_type.to_string());
        }
    }

    // Handle search patterns
    if let Some(resource_type) = field_name.strip_suffix("List") {
        return Some(resource_type.to_string());
    }
    if let Some(resource_type) = field_name.strip_suffix("Connection") {
        return Some(resource_type.to_string());
    }

    // Handle reverse reference fields (e.g., "ObservationList_subject")
    if let Some(idx) = field_name.find("List_") {
        return Some(field_name[..idx].to_string());
    }

    // Single resource type
    if field_name.chars().next().is_some_and(|c| c.is_uppercase()) {
        return Some(field_name.to_string());
    }

    None
}

// =============================================================================
// Policy Context Building
// =============================================================================

/// Builds a PolicyContext for access evaluation from GraphQL context.
///
/// # Arguments
///
/// * `gql_ctx` - The GraphQL context with auth and request info
/// * `operation` - The FHIR operation being performed
/// * `resource_type` - The target resource type
/// * `resource_id` - Optional resource ID for instance-level operations
/// * `resource` - Optional resource data (for update operations)
#[must_use]
pub fn build_policy_context(
    gql_ctx: &GraphQLContext,
    operation: FhirOperation,
    resource_type: &str,
    resource_id: Option<&str>,
    resource: Option<serde_json::Value>,
) -> PolicyContext {
    // Build user identity from auth context
    let user = gql_ctx
        .auth_context
        .as_ref()
        .and_then(|auth| auth.user.as_ref())
        .map(UserIdentity::from_user_context);

    // Build client identity
    let (client, scopes) = if let Some(auth) = &gql_ctx.auth_context {
        let scope_str = &auth.token_claims.scope;
        let parsed_scopes = SmartScopes::parse(scope_str).unwrap_or_default();

        let client = ClientIdentity::from_client(&auth.client, &parsed_scopes);
        let scopes = ScopeSummary::from_smart_scopes(scope_str, &parsed_scopes);

        (client, scopes)
    } else {
        // Anonymous/unauthenticated request
        (
            ClientIdentity {
                id: "anonymous".to_string(),
                name: "Anonymous".to_string(),
                trusted: false,
                client_type: ClientType::Public,
            },
            ScopeSummary::default(),
        )
    };

    // Build request context
    let path = build_fhir_path(resource_type, resource_id);
    let method = method_for_operation(operation);

    let request = RequestContext {
        operation,
        operation_id: Some("graphql.query".to_string()),
        resource_type: resource_type.to_string(),
        resource_id: resource_id.map(String::from),
        compartment_type: None,
        compartment_id: None,
        body: resource.clone(),
        query_params: HashMap::new(),
        path,
        method,
    };

    // Build resource context if we have a resource
    let resource_ctx = resource.map(octofhir_auth::policy::context::ResourceContext::from_resource);

    // Build environment context
    let patient_context = gql_ctx
        .auth_context
        .as_ref()
        .and_then(|auth| auth.patient.clone());
    let encounter_context = gql_ctx
        .auth_context
        .as_ref()
        .and_then(|auth| auth.encounter.clone());

    let environment = EnvironmentContext {
        request_time: OffsetDateTime::now_utc(),
        source_ip: gql_ctx.source_ip,
        request_id: gql_ctx.request_id.clone(),
        patient_context,
        encounter_context,
    };

    PolicyContext {
        user,
        client,
        scopes,
        request,
        resource: resource_ctx,
        environment,
    }
}

/// Returns the HTTP method equivalent for a FHIR operation.
fn method_for_operation(operation: FhirOperation) -> String {
    match operation {
        FhirOperation::Read
        | FhirOperation::VRead
        | FhirOperation::Search
        | FhirOperation::SearchType
        | FhirOperation::SearchSystem
        | FhirOperation::HistoryInstance
        | FhirOperation::HistoryType
        | FhirOperation::HistorySystem
        | FhirOperation::Capabilities => "GET".to_string(),
        FhirOperation::Create => "POST".to_string(),
        FhirOperation::Update => "PUT".to_string(),
        FhirOperation::Patch => "PATCH".to_string(),
        FhirOperation::Delete => "DELETE".to_string(),
        FhirOperation::Batch | FhirOperation::Transaction | FhirOperation::Operation => {
            "POST".to_string()
        }
    }
}

/// Builds a FHIR path from resource type and optional ID.
fn build_fhir_path(resource_type: &str, resource_id: Option<&str>) -> String {
    match resource_id {
        Some(id) => format!("/{}/{}", resource_type, id),
        None => format!("/{}", resource_type),
    }
}

// =============================================================================
// Access Evaluation
// =============================================================================

/// Evaluates access for a GraphQL operation.
///
/// Returns `Ok(())` if access is allowed, or an error if denied.
///
/// # Arguments
///
/// * `gql_ctx` - The GraphQL context
/// * `operation` - The FHIR operation being performed
/// * `resource_type` - The target resource type
/// * `resource_id` - Optional resource ID
pub async fn evaluate_access(
    gql_ctx: &GraphQLContext,
    operation: FhirOperation,
    resource_type: &str,
    resource_id: Option<&str>,
) -> Result<(), async_graphql::Error> {
    evaluate_access_with_resource(gql_ctx, operation, resource_type, resource_id, None).await
}

/// Evaluates access for a GraphQL operation with resource data.
///
/// This variant is used for update/delete operations where access control
/// may need to examine the existing resource.
pub async fn evaluate_access_with_resource(
    gql_ctx: &GraphQLContext,
    operation: FhirOperation,
    resource_type: &str,
    resource_id: Option<&str>,
    resource: Option<serde_json::Value>,
) -> Result<(), async_graphql::Error> {
    trace!(
        operation = ?operation,
        resource_type = %resource_type,
        resource_id = ?resource_id,
        "Evaluating access"
    );

    // Build policy context
    let policy_ctx = build_policy_context(gql_ctx, operation, resource_type, resource_id, resource);

    // Evaluate access
    let decision = gql_ctx.policy_evaluator.evaluate(&policy_ctx).await;

    match decision {
        AccessDecision::Allow => {
            debug!(
                operation = ?operation,
                resource_type = %resource_type,
                "Access allowed"
            );
            Ok(())
        }
        AccessDecision::Deny(reason) => {
            warn!(
                operation = ?operation,
                resource_type = %resource_type,
                reason = %reason.message,
                code = %reason.code,
                "Access denied"
            );
            Err(deny_reason_to_graphql_error(reason))
        }
        AccessDecision::Abstain => {
            // Abstain should not happen if policies are configured correctly
            // Treat as denied for safety
            warn!(
                operation = ?operation,
                resource_type = %resource_type,
                "Policy abstained, treating as denied"
            );
            Err(deny_reason_to_graphql_error(
                DenyReason::no_matching_policy(),
            ))
        }
    }
}

// =============================================================================
// Error Conversion
// =============================================================================

/// Converts a DenyReason to a GraphQL error with OperationOutcome in extensions.
pub fn deny_reason_to_graphql_error(reason: DenyReason) -> async_graphql::Error {
    let message = reason.message.clone();

    async_graphql::Error::new(&message).extend_with(|_, e| {
        e.set("code", reason.code.clone());

        if let Some(policy_id) = &reason.policy_id {
            e.set("policyId", policy_id.clone());
        }

        // Add OperationOutcome in extensions
        e.set(
            "operationOutcome",
            Value::Object({
                let mut map = async_graphql::indexmap::IndexMap::new();
                map.insert(
                    async_graphql::Name::new("resourceType"),
                    Value::String("OperationOutcome".to_string()),
                );
                map.insert(
                    async_graphql::Name::new("issue"),
                    Value::List(vec![Value::Object({
                        let mut issue = async_graphql::indexmap::IndexMap::new();
                        issue.insert(
                            async_graphql::Name::new("severity"),
                            Value::String("error".to_string()),
                        );
                        issue.insert(
                            async_graphql::Name::new("code"),
                            Value::String(code_to_issue_type(&reason.code)),
                        );
                        issue.insert(
                            async_graphql::Name::new("diagnostics"),
                            Value::String(message.clone()),
                        );
                        if let Some(details) = &reason.details {
                            issue.insert(
                                async_graphql::Name::new("details"),
                                json_to_graphql_value(details),
                            );
                        }
                        issue
                    })]),
                );
                map
            }),
        );
    })
}

/// Maps a deny reason code to a FHIR issue type.
fn code_to_issue_type(code: &str) -> String {
    match code {
        "insufficient-scope" => "forbidden".to_string(),
        "no-matching-policy" => "forbidden".to_string(),
        "policy-denied" => "forbidden".to_string(),
        "policy-error" | "script-error" => "exception".to_string(),
        _ => "forbidden".to_string(),
    }
}

/// Converts a serde_json::Value to async_graphql::Value.
fn json_to_graphql_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(
                    async_graphql::Number::from_f64(f).unwrap_or(async_graphql::Number::from(0)),
                )
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::List(arr.iter().map(json_to_graphql_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let map: async_graphql::indexmap::IndexMap<async_graphql::Name, Value> = obj
                .iter()
                .map(|(k, v)| (async_graphql::Name::new(k), json_to_graphql_value(v)))
                .collect();
            Value::Object(map)
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_graphql_to_fhir_operation_read() {
        assert_eq!(
            map_graphql_to_fhir_operation("Patient"),
            Some(FhirOperation::Read)
        );
        assert_eq!(
            map_graphql_to_fhir_operation("Observation"),
            Some(FhirOperation::Read)
        );
    }

    #[test]
    fn test_map_graphql_to_fhir_operation_search() {
        assert_eq!(
            map_graphql_to_fhir_operation("PatientList"),
            Some(FhirOperation::Search)
        );
        assert_eq!(
            map_graphql_to_fhir_operation("PatientConnection"),
            Some(FhirOperation::Search)
        );
        assert_eq!(
            map_graphql_to_fhir_operation("ObservationList_subject"),
            Some(FhirOperation::Search)
        );
    }

    #[test]
    fn test_map_graphql_to_fhir_operation_create() {
        assert_eq!(
            map_graphql_to_fhir_operation("PatientCreate"),
            Some(FhirOperation::Create)
        );
    }

    #[test]
    fn test_map_graphql_to_fhir_operation_update() {
        assert_eq!(
            map_graphql_to_fhir_operation("PatientUpdate"),
            Some(FhirOperation::Update)
        );
    }

    #[test]
    fn test_map_graphql_to_fhir_operation_delete() {
        assert_eq!(
            map_graphql_to_fhir_operation("PatientDelete"),
            Some(FhirOperation::Delete)
        );
    }

    #[test]
    fn test_map_graphql_to_fhir_operation_special() {
        assert_eq!(map_graphql_to_fhir_operation("_health"), None);
        assert_eq!(map_graphql_to_fhir_operation("_version"), None);
        assert_eq!(map_graphql_to_fhir_operation("__schema"), None);
    }

    #[test]
    fn test_extract_resource_type() {
        assert_eq!(
            extract_resource_type("Patient"),
            Some("Patient".to_string())
        );
        assert_eq!(
            extract_resource_type("PatientList"),
            Some("Patient".to_string())
        );
        assert_eq!(
            extract_resource_type("PatientConnection"),
            Some("Patient".to_string())
        );
        assert_eq!(
            extract_resource_type("PatientCreate"),
            Some("Patient".to_string())
        );
        assert_eq!(
            extract_resource_type("PatientUpdate"),
            Some("Patient".to_string())
        );
        assert_eq!(
            extract_resource_type("PatientDelete"),
            Some("Patient".to_string())
        );
        assert_eq!(
            extract_resource_type("ObservationList_subject"),
            Some("Observation".to_string())
        );
    }

    #[test]
    fn test_method_for_operation() {
        assert_eq!(method_for_operation(FhirOperation::Read), "GET");
        assert_eq!(method_for_operation(FhirOperation::Search), "GET");
        assert_eq!(method_for_operation(FhirOperation::Create), "POST");
        assert_eq!(method_for_operation(FhirOperation::Update), "PUT");
        assert_eq!(method_for_operation(FhirOperation::Delete), "DELETE");
    }

    #[test]
    fn test_build_fhir_path() {
        assert_eq!(build_fhir_path("Patient", None), "/Patient");
        assert_eq!(build_fhir_path("Patient", Some("123")), "/Patient/123");
    }

    #[test]
    fn test_code_to_issue_type() {
        assert_eq!(code_to_issue_type("insufficient-scope"), "forbidden");
        assert_eq!(code_to_issue_type("no-matching-policy"), "forbidden");
        assert_eq!(code_to_issue_type("policy-error"), "exception");
    }

    #[test]
    fn test_deny_reason_to_graphql_error() {
        let reason = DenyReason::scope_insufficient("patient/Patient.r");
        let error = deny_reason_to_graphql_error(reason);

        assert!(error.message.contains("scope"));
    }
}
