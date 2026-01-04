//! Operation Providers
//!
//! Each module provides its operations via the `OperationProvider` trait.

mod auth;
mod fhir;
mod gateway;
mod notifications;
mod system;
mod ui;

pub use auth::AuthOperationProvider;
pub use fhir::FhirOperationProvider;
pub use gateway::GatewayOperationProvider;
pub use notifications::NotificationsOperationProvider;
pub use system::SystemOperationProvider;
pub use ui::UiOperationProvider;
