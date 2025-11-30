//! Launch context storage trait for SMART on FHIR EHR launches.
//!
//! This module defines the storage interface for temporarily storing
//! launch contexts during the EHR launch flow. Launch contexts are
//! created by the EHR and consumed during authorization.
//!
//! # Lifecycle
//!
//! 1. EHR creates launch context with patient/encounter/etc. and stores it
//! 2. EHR sends `launch=<launch_id>` parameter to app
//! 3. App includes `launch` in authorization request
//! 4. Server retrieves launch context by `launch_id`
//! 5. Launch context is consumed (deleted) during token exchange
//! 6. Expired contexts are cleaned up automatically
//!
//! # Security Considerations
//!
//! - Launch IDs must be cryptographically secure (256 bits of entropy)
//! - Contexts have short TTL (default 10 minutes)
//! - Consume operation must be atomic (get + delete)
//! - Contexts must be single-use
//!
//! # Implementation Notes
//!
//! The `consume` method should atomically retrieve and delete the context.
//! This prevents the same launch context from being used multiple times.

use async_trait::async_trait;

use crate::AuthResult;
use crate::smart::launch::StoredLaunchContext;

/// Storage trait for SMART launch contexts.
///
/// This trait defines the interface for storing and retrieving launch
/// contexts during the SMART on FHIR EHR launch flow.
///
/// # Implementations
///
/// Implementations are provided in separate crates:
/// - `octofhir-auth-postgres` - PostgreSQL storage backend
///
/// # Example Implementation
///
/// ```ignore
/// use octofhir_auth::storage::LaunchContextStorage;
/// use octofhir_auth::smart::launch::StoredLaunchContext;
/// use octofhir_auth::AuthResult;
/// use std::collections::HashMap;
/// use std::sync::RwLock;
/// use std::time::{Duration, Instant};
///
/// struct InMemoryLaunchStorage {
///     contexts: RwLock<HashMap<String, (StoredLaunchContext, Instant)>>,
/// }
///
/// #[async_trait::async_trait]
/// impl LaunchContextStorage for InMemoryLaunchStorage {
///     async fn store(
///         &self,
///         context: &StoredLaunchContext,
///         ttl_seconds: u64,
///     ) -> AuthResult<()> {
///         let mut contexts = self.contexts.write().unwrap();
///         let expires_at = Instant::now() + Duration::from_secs(ttl_seconds);
///         contexts.insert(context.launch_id.clone(), (context.clone(), expires_at));
///         Ok(())
///     }
///     // ... other methods
/// }
/// ```
#[async_trait]
pub trait LaunchContextStorage: Send + Sync {
    /// Stores a launch context with TTL.
    ///
    /// The context is keyed by its `launch_id` field and will expire
    /// after `ttl_seconds`.
    ///
    /// # Arguments
    ///
    /// * `context` - The launch context to store
    /// * `ttl_seconds` - Time-to-live in seconds (typically 600 = 10 minutes)
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use octofhir_auth::smart::launch::{StoredLaunchContext, DEFAULT_LAUNCH_CONTEXT_TTL};
    ///
    /// let ctx = StoredLaunchContext::with_patient("launch123", "patient456");
    /// storage.store(&ctx, DEFAULT_LAUNCH_CONTEXT_TTL).await?;
    /// ```
    async fn store(&self, context: &StoredLaunchContext, ttl_seconds: u64) -> AuthResult<()>;

    /// Retrieves a launch context by launch ID without consuming it.
    ///
    /// Note: For the token exchange flow, use `consume` instead which
    /// atomically retrieves and deletes the context.
    ///
    /// # Arguments
    ///
    /// * `launch_id` - The opaque launch identifier from the EHR
    ///
    /// # Returns
    ///
    /// Returns `Some(context)` if found and not expired, `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn get(&self, launch_id: &str) -> AuthResult<Option<StoredLaunchContext>>;

    /// Atomically retrieves and deletes a launch context.
    ///
    /// This is the preferred method for the token exchange flow as it
    /// ensures single-use semantics. If two concurrent requests try to
    /// consume the same context, only one will succeed.
    ///
    /// # Arguments
    ///
    /// * `launch_id` - The opaque launch identifier from the EHR
    ///
    /// # Returns
    ///
    /// Returns `Some(context)` if found and consumed, `None` if not found
    /// or already consumed.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Atomicity
    ///
    /// Implementations must ensure this operation is atomic. A common approach
    /// is to use `DELETE ... RETURNING`:
    ///
    /// ```sql
    /// DELETE FROM smart_launch_contexts
    /// WHERE launch_id = $1 AND expires_at > NOW()
    /// RETURNING context_data
    /// ```
    async fn consume(&self, launch_id: &str) -> AuthResult<Option<StoredLaunchContext>>;

    /// Deletes a launch context by launch ID.
    ///
    /// Use this method when you need to explicitly invalidate a launch
    /// context before it's consumed (e.g., authorization denied).
    ///
    /// # Arguments
    ///
    /// * `launch_id` - The opaque launch identifier from the EHR
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn delete(&self, launch_id: &str) -> AuthResult<()>;

    /// Deletes expired launch context entries.
    ///
    /// Should be called periodically to clean up old entries and prevent
    /// storage growth. Entries can be deleted once their TTL has elapsed.
    ///
    /// # Returns
    ///
    /// Returns the number of entries deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the cleanup operation fails.
    async fn cleanup_expired(&self) -> AuthResult<u64>;

    /// Checks if a launch context exists and is not expired.
    ///
    /// This is a lightweight check that doesn't retrieve the full context.
    ///
    /// # Arguments
    ///
    /// * `launch_id` - The opaque launch identifier from the EHR
    ///
    /// # Returns
    ///
    /// Returns `true` if the context exists and has not expired.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn exists(&self, launch_id: &str) -> AuthResult<bool> {
        // Default implementation using get()
        Ok(self.get(launch_id).await?.is_some())
    }
}
