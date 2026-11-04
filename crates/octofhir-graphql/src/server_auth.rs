//! Adapter from GraphQL authorization to the server policy engine.

use crate::authorization::{AuthorizationError, Authorizer, FhirOperation as Operation};
use crate::context::GraphQLContext;
use octofhir_auth::middleware::AuthContext;
use octofhir_auth::policy::PolicyEvaluator;
use octofhir_auth::policy::context::{
    ClientIdentity, ClientType, EnvironmentContext, PolicyContext, RequestContext, ScopeSummary,
    UserIdentity,
};
use octofhir_auth::policy::engine::{AccessDecision, DenyReason};
use octofhir_auth::smart::scopes::{FhirOperation, SmartScopes};
use std::collections::HashMap;
use std::sync::Arc;
use time::OffsetDateTime;

/// Server policies and authenticated identity for one GraphQL request.
pub struct PolicyAuthorizer {
    pub evaluator: Arc<PolicyEvaluator>,
    pub auth: Option<AuthContext>,
}

#[async_trait::async_trait]
impl Authorizer for PolicyAuthorizer {
    async fn authorize(
        &self,
        context: &GraphQLContext,
        operation: Operation,
        resource_type: &str,
        resource_id: Option<&str>,
        resource: Option<serde_json::Value>,
    ) -> Result<(), AuthorizationError> {
        let policy_context = build_policy_context(
            context,
            self.auth.as_ref(),
            operation,
            resource_type,
            resource_id,
            resource,
        );
        match self.evaluator.evaluate(&policy_context).await {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny(reason) => Err(reason.into()),
            AccessDecision::Abstain => Err(DenyReason::no_matching_policy().into()),
        }
    }
}

impl From<DenyReason> for AuthorizationError {
    fn from(reason: DenyReason) -> Self {
        Self {
            message: reason.message,
            code: reason.code,
            policy_id: reason.policy_id,
            details: reason.details,
        }
    }
}

fn to_policy_operation(operation: Operation) -> FhirOperation {
    match operation {
        Operation::Read => FhirOperation::Read,
        Operation::VRead => FhirOperation::VRead,
        Operation::Update => FhirOperation::Update,
        Operation::Patch => FhirOperation::Patch,
        Operation::Delete => FhirOperation::Delete,
        Operation::Create => FhirOperation::Create,
        Operation::Search => FhirOperation::Search,
        Operation::SearchType => FhirOperation::SearchType,
        Operation::SearchSystem => FhirOperation::SearchSystem,
        Operation::Capabilities => FhirOperation::Capabilities,
        Operation::Batch => FhirOperation::Batch,
        Operation::Transaction => FhirOperation::Transaction,
        Operation::HistoryInstance => FhirOperation::HistoryInstance,
        Operation::HistoryType => FhirOperation::HistoryType,
        Operation::HistorySystem => FhirOperation::HistorySystem,
        Operation::Operation => FhirOperation::Operation,
    }
}

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
    auth_context: Option<&AuthContext>,
    operation: Operation,
    resource_type: &str,
    resource_id: Option<&str>,
    resource: Option<serde_json::Value>,
) -> PolicyContext {
    let operation = to_policy_operation(operation);
    // Build user identity from auth context
    let user = auth_context
        .and_then(|auth| auth.user.as_ref())
        .map(UserIdentity::from_user_context);

    // Build client identity
    let (client, scopes) = if let Some(auth) = auth_context {
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
    let patient_context = auth_context.and_then(|auth| auth.patient.clone());
    let encounter_context = auth_context.and_then(|auth| auth.encounter.clone());

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_build_fhir_path() {
        assert_eq!(build_fhir_path("Patient", None), "/Patient");
        assert_eq!(build_fhir_path("Patient", Some("123")), "/Patient/123");
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
    fn test_deny_reason_to_graphql_error() {
        let reason = DenyReason::scope_insufficient("patient/Patient.r");
        let error = crate::resolvers::access::deny_reason_to_graphql_error(reason.into());

        assert!(error.message.contains("scope"));
    }
}
