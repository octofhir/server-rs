//! FHIR Operations Framework
//!
//! This module provides the infrastructure for loading, registering, and
//! routing FHIR operations. It supports system-level, type-level, and
//! instance-level operations as defined in OperationDefinition resources.
//!
//! # Architecture
//!
//! The operations framework consists of several components:
//!
//! - **OperationDefinition**: Core types representing FHIR operation definitions
//! - **OperationRegistry**: Storage and lookup for registered operations
//! - **OperationHandler**: Trait for implementing operation logic
//! - **OperationParams**: Parameter extraction from HTTP requests
//! - **Loader**: Loading operations from the canonical manager
//! - **Router**: HTTP handlers for operation endpoints
//!
//! # Example
//!
//! ```ignore
//! use octofhir_server::operations::{OperationHandler, OperationError, OperationRegistry};
//!
//! struct ValidateHandler;
//!
//! #[async_trait::async_trait]
//! impl OperationHandler for ValidateHandler {
//!     fn code(&self) -> &str {
//!         "validate"
//!     }
//!
//!     async fn handle_type(
//!         &self,
//!         state: &AppState,
//!         resource_type: &str,
//!         params: &serde_json::Value,
//!     ) -> Result<serde_json::Value, OperationError> {
//!         // Implementation
//!         Ok(serde_json::json!({"resourceType": "OperationOutcome"}))
//!     }
//! }
//! ```

pub mod definition;
pub mod handler;
pub mod loader;
pub mod params;
pub mod registry;
pub mod router;

// Re-export main types for convenience
pub use definition::{OperationDefinition, OperationKind, OperationParameter, ParameterUse};
pub use handler::{DynOperationHandler, OperationError, OperationHandler};
pub use loader::{LoadError, load_operations};
pub use params::OperationParams;
pub use registry::OperationRegistry;
pub use router::{
    instance_operation_handler, is_operation, merged_root_get_handler, merged_root_post_handler,
    merged_type_get_handler, merged_type_post_handler, system_operation_handler,
    type_operation_handler,
};
