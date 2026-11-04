//! Required, transport-independent authorization for GraphQL operations.

use crate::context::GraphQLContext;

/// FHIR interaction requested by a GraphQL resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FhirOperation {
    Read,
    VRead,
    Update,
    Patch,
    Delete,
    Create,
    Search,
    SearchType,
    SearchSystem,
    Capabilities,
    Batch,
    Transaction,
    HistoryInstance,
    HistoryType,
    HistorySystem,
    Operation,
}

/// A denied interaction, including FHIR-compatible error details.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct AuthorizationError {
    pub message: String,
    pub code: String,
    pub policy_id: Option<String>,
    pub details: Option<serde_json::Value>,
}

/// Checks every resource interaction. Implementations must explicitly grant access.
///
/// There is no implicit allow implementation; the context builder requires an authorizer.
#[async_trait::async_trait]
pub trait Authorizer: Send + Sync {
    async fn authorize(
        &self,
        context: &GraphQLContext,
        operation: FhirOperation,
        resource_type: &str,
        resource_id: Option<&str>,
        resource: Option<serde_json::Value>,
    ) -> Result<(), AuthorizationError>;
}
