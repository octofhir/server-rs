//! Unified Resource Event System for inter-module communication.
//!
//! This module provides the core event infrastructure for OctoFHIR,
//! enabling loose coupling between modules through event-driven communication.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                       Event Broadcaster                              │
//! │              (tokio::sync::broadcast channel)                        │
//! └─────────────────────────────────────────────────────────────────────┘
//!          │                    │                    │
//!          ▼                    ▼                    ▼
//!    ┌──────────┐        ┌──────────┐        ┌──────────┐
//!    │  Hook 1  │        │  Hook 2  │        │  Hook 3  │
//!    │ (async)  │        │ (async)  │        │ (async)  │
//!    └──────────┘        └──────────┘        └──────────┘
//! ```
//!
//! # Features
//!
//! - **Event Types**: `ResourceEvent` for FHIR resources, `AuthEvent` for auth
//! - **Broadcaster**: Central event bus with multi-subscriber support
//! - **Hooks**: Async handlers with timeout and panic protection
//! - **Registry**: Hook registration and lifecycle management
//! - **Isolation**: Each hook runs in its own task, failures don't propagate
//!
//! # Example
//!
//! ```ignore
//! use octofhir_core::events::{EventBroadcaster, HookRegistry, ResourceEvent};
//!
//! // Create broadcaster and registry
//! let broadcaster = EventBroadcaster::new_shared();
//! let registry = HookRegistry::new();
//!
//! // Register a hook
//! registry.register_resource(my_hook).await;
//!
//! // Start dispatcher
//! let dispatcher = HookDispatcher::new(registry);
//! tokio::spawn(dispatcher.run(broadcaster.subscribe()));
//!
//! // Send events
//! broadcaster.send_created("Patient", "123", json!({}));
//! ```
//!
//! # Module Structure
//!
//! - [`types`]: Event type definitions (`ResourceEvent`, `AuthEvent`, `SystemEvent`)
//! - [`broadcaster`]: Event broadcasting infrastructure
//! - [`hooks`]: Hook traits and error types
//! - [`registry`]: Hook registry and dispatcher

pub mod broadcaster;
pub mod hooks;
pub mod registry;
pub mod types;

// Re-export main types for convenience
pub use broadcaster::EventBroadcaster;
pub use hooks::{AuthHook, AuthHookAdapter, HookError, ResourceHook, ResourceHookAdapter, SystemHook};
pub use registry::{HookDispatcher, HookRegistry, HookSystemBuilder};
pub use types::{AuthEvent, AuthEventType, ResourceEvent, ResourceEventType, SystemEvent};
