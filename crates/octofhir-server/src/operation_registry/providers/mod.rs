//! Operation Providers
//!
//! Each module provides its operations via the `OperationProvider` trait.

mod auth;
mod fhir;
mod system;
mod ui;

pub use auth::AuthOperationProvider;
pub use fhir::FhirOperationProvider;
pub use system::SystemOperationProvider;
pub use ui::UiOperationProvider;
