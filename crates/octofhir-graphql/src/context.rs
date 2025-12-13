//! GraphQL execution context.
//!
//! This module provides the context struct that holds all dependencies needed
//! by GraphQL resolvers. The context is constructed per-request and contains
//! both shared state (storage, config) and request-specific state (auth, loaders).
//!
//! # Example
//!
//! ```ignore
//! use octofhir_graphql::GraphQLContextBuilder;
//!
//! let context = GraphQLContextBuilder::new()
//!     .with_storage(storage.clone())
//!     .with_search_config(search_config.clone())
//!     .with_model_provider(model_provider.clone())
//!     .with_policy_evaluator(policy_evaluator.clone())
//!     .with_auth_context(Some(auth_context))
//!     .with_request_id("req-123")
//!     .build()?;
//! ```

use std::net::IpAddr;
use std::sync::Arc;

use octofhir_auth::middleware::AuthContext;
use octofhir_auth::policy::PolicyEvaluator;
use octofhir_search::SearchConfig;
use octofhir_storage::DynStorage;

use crate::loaders::DataLoaders;

/// GraphQL execution context.
///
/// This struct holds all dependencies needed by GraphQL resolvers to execute
/// queries. It is constructed per-request and passed through the async-graphql
/// context system.
///
/// The context is designed to be `Clone` and `Send + Sync` safe, using `Arc`
/// for shared state.
#[derive(Clone)]
pub struct GraphQLContext {
    /// FHIR resource storage.
    pub storage: DynStorage,

    /// Search configuration with parameter registry.
    pub search_config: SearchConfig,

    /// Policy evaluator for access control.
    pub policy_evaluator: Arc<PolicyEvaluator>,

    /// Authentication context for the current request (None for unauthenticated).
    pub auth_context: Option<AuthContext>,

    /// Request ID for tracing and correlation.
    pub request_id: String,

    /// Source IP address of the request.
    pub source_ip: Option<IpAddr>,

    /// Target resource type for instance-level queries (e.g., "Patient").
    pub target_resource_type: Option<String>,

    /// Target resource ID for instance-level queries (e.g., "123").
    pub target_resource_id: Option<String>,

    /// DataLoaders for efficient batched data loading.
    ///
    /// These loaders batch and cache resource loads within a single request,
    /// preventing N+1 query problems when resolving references.
    pub loaders: DataLoaders,
}

impl GraphQLContext {
    /// Returns whether the request is authenticated.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.auth_context.is_some()
    }

    /// Returns the user ID if authenticated.
    #[must_use]
    pub fn user_id(&self) -> Option<String> {
        self.auth_context
            .as_ref()
            .and_then(|ctx| ctx.user.as_ref())
            .map(|u| u.id.to_string())
    }

    /// Returns whether this is an instance-level query.
    #[must_use]
    pub fn is_instance_level(&self) -> bool {
        self.target_resource_type.is_some() && self.target_resource_id.is_some()
    }

    /// Returns the target resource reference (e.g., "Patient/123").
    #[must_use]
    pub fn target_reference(&self) -> Option<String> {
        match (&self.target_resource_type, &self.target_resource_id) {
            (Some(rt), Some(id)) => Some(format!("{rt}/{id}")),
            _ => None,
        }
    }

    /// Loads a resource by type and ID using the DataLoader.
    ///
    /// Returns `None` if the resource does not exist.
    pub async fn load_resource(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Option<serde_json::Value> {
        use crate::loaders::ResourceKey;
        let key = ResourceKey::new(resource_type, id);
        self.loaders.resource_loader.load_one(key).await.ok().flatten()
    }

    /// Resolves a FHIR reference string to a resource.
    ///
    /// Supports relative (`Patient/123`), absolute (`http://...`), and
    /// contained (`#id`) reference formats.
    ///
    /// Returns `None` if the reference is invalid or the resource doesn't exist.
    pub async fn resolve_reference(
        &self,
        reference: &str,
    ) -> Option<crate::loaders::ResolvedReference> {
        use crate::loaders::ReferenceKey;
        let key = ReferenceKey::new(reference);
        self.loaders.reference_loader.load_one(key).await.ok().flatten()
    }

    /// Creates a new builder for GraphQLContext.
    #[must_use]
    pub fn builder() -> GraphQLContextBuilder {
        GraphQLContextBuilder::default()
    }
}

/// Builder for constructing GraphQLContext.
///
/// This builder validates that all required fields are provided before
/// creating the context.
#[derive(Default)]
pub struct GraphQLContextBuilder {
    storage: Option<DynStorage>,
    search_config: Option<SearchConfig>,
    policy_evaluator: Option<Arc<PolicyEvaluator>>,
    auth_context: Option<AuthContext>,
    request_id: Option<String>,
    source_ip: Option<IpAddr>,
    target_resource_type: Option<String>,
    target_resource_id: Option<String>,
}

impl GraphQLContextBuilder {
    /// Creates a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the storage backend.
    #[must_use]
    pub fn with_storage(mut self, storage: DynStorage) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Sets the search configuration.
    #[must_use]
    pub fn with_search_config(mut self, config: SearchConfig) -> Self {
        self.search_config = Some(config);
        self
    }

    /// Sets the policy evaluator.
    #[must_use]
    pub fn with_policy_evaluator(mut self, evaluator: Arc<PolicyEvaluator>) -> Self {
        self.policy_evaluator = Some(evaluator);
        self
    }

    /// Sets the authentication context.
    #[must_use]
    pub fn with_auth_context(mut self, auth: Option<AuthContext>) -> Self {
        self.auth_context = auth;
        self
    }

    /// Sets the request ID.
    #[must_use]
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Sets the source IP address.
    #[must_use]
    pub fn with_source_ip(mut self, ip: Option<IpAddr>) -> Self {
        self.source_ip = ip;
        self
    }

    /// Sets the target resource type for instance-level queries.
    #[must_use]
    pub fn with_target_resource_type(mut self, resource_type: impl Into<String>) -> Self {
        self.target_resource_type = Some(resource_type.into());
        self
    }

    /// Sets the target resource ID for instance-level queries.
    #[must_use]
    pub fn with_target_resource_id(mut self, id: impl Into<String>) -> Self {
        self.target_resource_id = Some(id.into());
        self
    }

    /// Sets both target resource type and ID at once.
    #[must_use]
    pub fn with_target_resource(
        mut self,
        resource_type: impl Into<String>,
        id: impl Into<String>,
    ) -> Self {
        self.target_resource_type = Some(resource_type.into());
        self.target_resource_id = Some(id.into());
        self
    }

    /// Builds the GraphQLContext.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> Result<GraphQLContext, ContextBuilderError> {
        let storage = self
            .storage
            .ok_or(ContextBuilderError::MissingField("storage"))?;

        let search_config = self
            .search_config
            .ok_or(ContextBuilderError::MissingField("search_config"))?;

        let policy_evaluator = self
            .policy_evaluator
            .ok_or(ContextBuilderError::MissingField("policy_evaluator"))?;

        let request_id = self
            .request_id
            .ok_or(ContextBuilderError::MissingField("request_id"))?;

        // Create DataLoaders for this request
        // Each request gets its own set of loaders to ensure proper batching scope
        let loaders = DataLoaders::new(storage.clone());

        Ok(GraphQLContext {
            storage,
            search_config,
            policy_evaluator,
            auth_context: self.auth_context,
            request_id,
            source_ip: self.source_ip,
            target_resource_type: self.target_resource_type,
            target_resource_id: self.target_resource_id,
            loaders,
        })
    }
}

/// Errors that can occur when building a GraphQLContext.
#[derive(Debug, thiserror::Error)]
pub enum ContextBuilderError {
    /// A required field was not provided.
    #[error("Missing required field: {0}")]
    MissingField(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full tests require actual storage/search/policy implementations.
    // These tests verify the builder logic.

    #[test]
    fn test_builder_missing_storage() {
        let result = GraphQLContextBuilder::new()
            .with_request_id("req-123")
            .build();

        assert!(matches!(
            result,
            Err(ContextBuilderError::MissingField("storage"))
        ));
    }

    #[test]
    fn test_builder_missing_search_config() {
        // We can't easily create a mock storage here, so just test the pattern
        let builder = GraphQLContextBuilder::new().with_request_id("req-123");
        // Without storage set, it will fail on storage first
        assert!(builder.build().is_err());
    }
}
