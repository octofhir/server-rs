//! Hook implementations for the unified event system.
//!
//! This module provides concrete hook implementations that respond to
//! resource events for various server functionality:
//!
//! - [`PolicyReloadHook`] - Triggers policy cache reload on AccessPolicy changes
//! - [`GatewayReloadHook`] - Triggers gateway router reload on App/CustomOperation changes
//! - [`SearchParamHook`] - Updates search parameter registry on SearchParameter changes
//! - [`GraphQLSubscriptionHook`] - Forwards events to GraphQL subscription broadcaster
//! - [`AsyncAuditHook`] - Logs resource changes asynchronously as FHIR AuditEvents
//!
//! # Architecture
//!
//! Hooks are registered with the [`HookRegistry`] during server startup.
//! When a resource is created/updated/deleted, the [`EventedStorage`] wrapper
//! emits events that are dispatched to registered hooks.
//!
//! Each hook runs in isolation with:
//! - Timeout protection (30s default)
//! - Panic recovery via `catch_unwind`
//! - Error logging without propagation
//!
//! # Example
//!
//! ```ignore
//! use octofhir_core::events::{HookRegistry, EventBroadcaster};
//! use octofhir_server::hooks::PolicyReloadHook;
//!
//! let broadcaster = EventBroadcaster::new_shared();
//! let mut registry = HookRegistry::new();
//!
//! // Register hooks
//! let policy_hook = PolicyReloadHook::new(policy_notifier);
//! registry.register(Arc::new(policy_hook));
//!
//! // Start dispatcher
//! registry.start_dispatcher(broadcaster.subscribe());
//! ```

mod audit;
mod gateway;
mod graphql;
mod policy;
mod search;

pub use audit::AsyncAuditHook;
pub use gateway::GatewayReloadHook;
pub use graphql::GraphQLSubscriptionHook;
pub use policy::PolicyReloadHook;
pub use search::SearchParamHook;

// Re-export core types for convenience
pub use octofhir_core::events::{
    HookError, HookRegistry, ResourceEventType, ResourceHook, SystemEvent, SystemHook,
};
