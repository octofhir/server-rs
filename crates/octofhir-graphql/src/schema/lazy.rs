//! Lazy schema loading implementation.
//!
//! This module provides `LazySchema`, a thread-safe wrapper that defers schema
//! building until first access. This allows the server to start immediately
//! without waiting for schema construction.

use std::sync::Arc;

use async_graphql::dynamic::Schema;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use super::FhirSchemaBuilder;
use crate::error::GraphQLError;

/// State of the lazy schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaState {
    /// Schema has not been built yet.
    Uninitialized,
    /// Schema is currently being built.
    Building,
    /// Schema is ready for use.
    Ready,
    /// Schema build failed.
    Failed,
}

/// Thread-safe lazy schema holder.
///
/// `LazySchema` wraps a GraphQL schema that is built on first access.
/// It handles concurrent access during build and supports hot-reload
/// through the `invalidate()` method.
///
/// # Example
///
/// ```ignore
/// let lazy_schema = LazySchema::new(schema_builder);
///
/// // First access triggers build
/// let schema = lazy_schema.get_or_build().await?;
///
/// // Subsequent accesses use cached schema
/// let schema = lazy_schema.get_or_build().await?;
///
/// // Force rebuild on next access
/// lazy_schema.invalidate().await;
/// ```
pub struct LazySchema {
    /// The cached schema (None if not built yet or invalidated).
    schema: RwLock<Option<Arc<Schema>>>,

    /// Build lock to ensure only one build at a time.
    build_lock: Mutex<()>,

    /// Current state of the schema.
    state: RwLock<SchemaState>,

    /// The schema builder.
    builder: Arc<FhirSchemaBuilder>,

    /// Last build error message (for diagnostics).
    last_error: RwLock<Option<String>>,
}

impl LazySchema {
    /// Creates a new lazy schema with the given builder.
    #[must_use]
    pub fn new(builder: FhirSchemaBuilder) -> Self {
        Self {
            schema: RwLock::new(None),
            build_lock: Mutex::new(()),
            state: RwLock::new(SchemaState::Uninitialized),
            builder: Arc::new(builder),
            last_error: RwLock::new(None),
        }
    }

    /// Returns the current state of the schema.
    pub async fn state(&self) -> SchemaState {
        *self.state.read().await
    }

    /// Gets the schema, building it if necessary.
    ///
    /// If the schema is not yet built, this method triggers a build.
    /// Concurrent callers will receive an error if a build is in progress.
    ///
    /// For introspection queries or cases where waiting is acceptable,
    /// use `get_or_build_wait()` instead.
    ///
    /// # Errors
    ///
    /// Returns `GraphQLError::SchemaInitializing` if another build is in progress.
    /// Returns `GraphQLError::SchemaBuildFailed` if the build fails.
    pub async fn get_or_build(&self) -> Result<Arc<Schema>, GraphQLError> {
        // Fast path: schema already built
        {
            let schema = self.schema.read().await;
            if let Some(ref s) = *schema {
                return Ok(Arc::clone(s));
            }
        }

        // Check if a build is in progress
        let state = *self.state.read().await;
        if state == SchemaState::Building {
            // Return a "please retry" error rather than blocking
            return Err(GraphQLError::SchemaInitializing);
        }

        // Try to acquire build lock
        let _guard = match self.build_lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                // Another thread is building
                return Err(GraphQLError::SchemaInitializing);
            }
        };

        // Double-check after acquiring lock
        {
            let schema = self.schema.read().await;
            if let Some(ref s) = *schema {
                return Ok(Arc::clone(s));
            }
        }

        // Mark as building
        *self.state.write().await = SchemaState::Building;
        info!("Building GraphQL schema...");

        // Build the schema
        match self.builder.build().await {
            Ok(schema) => {
                let schema = Arc::new(schema);
                *self.schema.write().await = Some(Arc::clone(&schema));
                *self.state.write().await = SchemaState::Ready;
                *self.last_error.write().await = None;
                info!("GraphQL schema built successfully");
                Ok(schema)
            }
            Err(e) => {
                let error_msg = e.to_string();
                warn!(error = %error_msg, "Failed to build GraphQL schema");
                *self.state.write().await = SchemaState::Failed;
                *self.last_error.write().await = Some(error_msg.clone());
                Err(GraphQLError::SchemaBuildFailed(error_msg))
            }
        }
    }

    /// Gets the schema, building it if necessary, and waits for the build to complete.
    ///
    /// Unlike `get_or_build()`, this method waits for an in-progress build to complete
    /// instead of returning an error. This is useful for introspection queries where
    /// waiting is acceptable.
    ///
    /// # Errors
    ///
    /// Returns `GraphQLError::SchemaBuildFailed` if the build fails.
    pub async fn get_or_build_wait(&self) -> Result<Arc<Schema>, GraphQLError> {
        // Fast path: schema already built
        {
            let schema = self.schema.read().await;
            if let Some(ref s) = *schema {
                return Ok(Arc::clone(s));
            }
        }

        // Acquire build lock and wait for it
        let _guard = self.build_lock.lock().await;

        // Double-check after acquiring lock
        {
            let schema = self.schema.read().await;
            if let Some(ref s) = *schema {
                return Ok(Arc::clone(s));
            }
        }

        // Check if build failed previously
        let state = *self.state.read().await;
        if state == SchemaState::Failed {
            if let Some(err) = self.last_error.read().await.as_ref() {
                return Err(GraphQLError::SchemaBuildFailed(err.clone()));
            }
        }

        // Mark as building
        *self.state.write().await = SchemaState::Building;
        info!("Building GraphQL schema (wait mode)...");

        // Build the schema
        match self.builder.build().await {
            Ok(schema) => {
                let schema = Arc::new(schema);
                *self.schema.write().await = Some(Arc::clone(&schema));
                *self.state.write().await = SchemaState::Ready;
                *self.last_error.write().await = None;
                info!("GraphQL schema built successfully (wait mode)");
                Ok(schema)
            }
            Err(e) => {
                let error_msg = e.to_string();
                warn!(error = %error_msg, "Failed to build GraphQL schema (wait mode)");
                *self.state.write().await = SchemaState::Failed;
                *self.last_error.write().await = Some(error_msg.clone());
                Err(GraphQLError::SchemaBuildFailed(error_msg))
            }
        }
    }

    /// Gets the schema if it's already built, without triggering a build.
    ///
    /// Returns `None` if the schema has not been built yet.
    pub async fn get(&self) -> Option<Arc<Schema>> {
        self.schema.read().await.clone()
    }

    /// Invalidates the cached schema, causing the next `get_or_build()`
    /// to rebuild it.
    ///
    /// This is used for hot-reload support when the model provider changes.
    pub async fn invalidate(&self) {
        // Acquire build lock to ensure no concurrent build
        let _guard = self.build_lock.lock().await;

        *self.schema.write().await = None;
        *self.state.write().await = SchemaState::Uninitialized;
        *self.last_error.write().await = None;

        info!("GraphQL schema invalidated - will rebuild on next request");
    }

    /// Returns the last build error, if any.
    pub async fn last_error(&self) -> Option<String> {
        self.last_error.read().await.clone()
    }

    /// Returns whether the schema is ready for use.
    pub async fn is_ready(&self) -> bool {
        *self.state.read().await == SchemaState::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full tests require FhirSchemaBuilder with mock dependencies.
    // These tests verify the state machine logic.

    #[test]
    fn test_schema_state_enum() {
        assert_ne!(SchemaState::Uninitialized, SchemaState::Building);
        assert_ne!(SchemaState::Building, SchemaState::Ready);
        assert_ne!(SchemaState::Ready, SchemaState::Failed);
    }
}
