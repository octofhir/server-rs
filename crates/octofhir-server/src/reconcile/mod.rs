//! Reconciliation logic for App Platform resources.
//!
//! Handles automatic synchronization of derived resources when Apps are created/updated:
//! - CustomOperations (from App.operations[])
//! - AppSubscriptions (from App.subscriptions[])
//! - Provisioned resources (from App.resources)

mod operations;
mod resources;
mod subscriptions;

pub use operations::{reconcile_operations, ReconcileResult};
pub use resources::reconcile_resources;
pub use subscriptions::reconcile_subscriptions;
