//! GraphQL subscriptions for FHIR resource changes.
//!
//! This module implements GraphQL subscriptions that allow clients to receive
//! real-time notifications when FHIR resources are created, updated, or deleted.
//!
//! ## Architecture
//!
//! Subscriptions use a broadcast channel pattern:
//! 1. Resource changes emit events to a broadcast channel
//! 2. GraphQL subscription fields subscribe to filtered views of this channel
//! 3. Events are delivered to connected clients via WebSocket or SSE
//!
//! ## Events
//!
//! - `resourceCreated(resourceType: String)` - Emits when a resource is created
//! - `resourceUpdated(resourceType: String)` - Emits when a resource is updated
//! - `resourceDeleted(resourceType: String)` - Emits when a resource is deleted
//! - `resourceChanged(resourceType: String)` - Emits for any change (create/update/delete)

mod events;
pub mod fields;

pub use events::{ResourceChangeEvent, ResourceEventType, ResourceEventBroadcaster};
pub use fields::{build_subscription_type, create_resource_change_event_type};
