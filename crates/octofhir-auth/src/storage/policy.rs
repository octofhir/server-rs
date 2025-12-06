//! Policy storage trait.
//!
//! Defines the interface for AccessPolicy persistence and caching operations.
//! Implementations are provided by storage backends (e.g., PostgreSQL).

use async_trait::async_trait;

use crate::AuthResult;
use crate::policy::resources::{AccessPolicy, PolicyEngineType};
use crate::smart::scopes::FhirOperation;

// =============================================================================
// Policy Search Parameters
// =============================================================================

/// Parameters for searching policies.
#[derive(Debug, Default, Clone)]
pub struct PolicySearchParams {
    /// Filter by policy name (partial match).
    pub name: Option<String>,

    /// Filter by active status.
    pub active: Option<bool>,

    /// Filter by engine type.
    pub engine_type: Option<PolicyEngineType>,

    /// Filter by target resource type.
    pub resource_type: Option<String>,

    /// Maximum number of results to return.
    pub count: Option<usize>,

    /// Number of results to skip.
    pub offset: Option<usize>,
}

impl PolicySearchParams {
    /// Create new empty search parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the name filter.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the active filter.
    #[must_use]
    pub fn with_active(mut self, active: bool) -> Self {
        self.active = Some(active);
        self
    }

    /// Set the engine type filter.
    #[must_use]
    pub fn with_engine_type(mut self, engine_type: PolicyEngineType) -> Self {
        self.engine_type = Some(engine_type);
        self
    }

    /// Set the resource type filter.
    #[must_use]
    pub fn with_resource_type(mut self, resource_type: impl Into<String>) -> Self {
        self.resource_type = Some(resource_type.into());
        self
    }

    /// Set pagination parameters.
    #[must_use]
    pub fn with_pagination(mut self, count: usize, offset: usize) -> Self {
        self.count = Some(count);
        self.offset = Some(offset);
        self
    }
}

// =============================================================================
// Policy Storage Trait
// =============================================================================

/// Storage operations for AccessPolicy resources.
///
/// This trait defines the interface for persisting and retrieving access
/// control policies. Implementations handle the actual database operations.
///
/// # Example
///
/// ```ignore
/// use octofhir_auth::storage::PolicyStorage;
///
/// async fn example(storage: &impl PolicyStorage) {
///     // Get all active policies
///     let policies = storage.list_active().await?;
///     for policy in policies {
///         println!("Policy: {} (priority: {})", policy.name, policy.priority);
///     }
/// }
/// ```
#[async_trait]
pub trait PolicyStorage: Send + Sync {
    /// Get a policy by its ID.
    ///
    /// Returns `None` if the policy doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn get(&self, id: &str) -> AuthResult<Option<AccessPolicy>>;

    /// List all active policies ordered by priority.
    ///
    /// Only returns policies where `active == true`.
    /// Results are sorted by priority (ascending - lower priority evaluated first).
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn list_active(&self) -> AuthResult<Vec<AccessPolicy>>;

    /// List all policies (for admin use).
    ///
    /// Returns both active and inactive policies.
    /// Results are sorted by priority (ascending).
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn list_all(&self) -> AuthResult<Vec<AccessPolicy>>;

    /// Create a new policy.
    ///
    /// The policy is validated before creation.
    /// An ID is generated if not provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The policy validation fails
    /// - A policy with the same ID already exists
    /// - The storage operation fails
    async fn create(&self, policy: &AccessPolicy) -> AuthResult<AccessPolicy>;

    /// Update an existing policy.
    ///
    /// The policy is validated before update.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The policy doesn't exist
    /// - The policy validation fails
    /// - The storage operation fails
    async fn update(&self, id: &str, policy: &AccessPolicy) -> AuthResult<AccessPolicy>;

    /// Delete a policy.
    ///
    /// Implementations should perform a soft delete to preserve audit trail.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The policy doesn't exist
    /// - The storage operation fails
    async fn delete(&self, id: &str) -> AuthResult<()>;

    /// Find policies applicable to a resource type and operation.
    ///
    /// Returns policies that:
    /// - Are active
    /// - Match the given resource type (or have no resource type filter)
    /// - Match the given operation (or have no operation filter)
    ///
    /// Results are sorted by priority (ascending).
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_applicable(
        &self,
        resource_type: &str,
        operation: FhirOperation,
    ) -> AuthResult<Vec<AccessPolicy>>;

    /// Get policies by their IDs.
    ///
    /// Used for retrieving policies linked to clients or users.
    /// Returns policies in priority order.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn get_by_ids(&self, ids: &[String]) -> AuthResult<Vec<AccessPolicy>>;

    /// Search policies with filtering.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn search(&self, params: &PolicySearchParams) -> AuthResult<Vec<AccessPolicy>>;

    /// Find policies linked to a specific client.
    ///
    /// Returns active policies that have a link element referencing the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_for_client(&self, client_id: &str) -> AuthResult<Vec<AccessPolicy>>;

    /// Find policies linked to a specific user.
    ///
    /// Returns active policies that have a link element referencing the user.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_for_user(&self, user_id: &str) -> AuthResult<Vec<AccessPolicy>>;

    /// Find policies linked to a specific role.
    ///
    /// Returns active policies that have a link element referencing the role.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_for_role(&self, role: &str) -> AuthResult<Vec<AccessPolicy>>;
}
