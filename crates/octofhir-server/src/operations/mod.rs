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
pub mod everything;
pub mod handler;
pub mod loader;
pub mod meta;
pub mod params;
pub mod registry;
pub mod router;
pub mod validate;

// Re-export main types for convenience
pub use definition::{OperationDefinition, OperationKind, OperationParameter, ParameterUse};
pub use everything::EverythingOperation;
pub use handler::{DynOperationHandler, OperationError, OperationHandler};
pub use loader::{LoadError, load_operations};
pub use meta::{MetaAddOperation, MetaDeleteOperation, MetaOperation};
pub use params::OperationParams;
pub use registry::OperationRegistry;
pub use router::{
    instance_operation_handler, is_operation, merged_root_get_handler, merged_root_post_handler,
    merged_type_get_handler, merged_type_post_handler, system_operation_handler,
    type_operation_handler,
};
pub use validate::{Issue, Severity, ValidateOperation};

use std::collections::HashMap;
use std::sync::Arc;

use crate::server::SharedModelProvider;
use octofhir_fhirpath::FhirPathEngine;

/// Registers the core FHIR operations.
///
/// This function creates and registers handlers for:
/// - `$validate` - Resource validation
/// - `$meta` - Get resource metadata
/// - `$meta-add` - Add metadata elements
/// - `$meta-delete` - Remove metadata elements
/// - `$everything` - Retrieve complete record for Patient, Encounter, or Group
///
/// # Arguments
///
/// * `fhirpath_engine` - The FHIRPath engine for validation constraints
/// * `model_provider` - The schema provider for type information
///
/// # Returns
///
/// A HashMap mapping operation codes to their handlers.
pub fn register_core_operations(
    fhirpath_engine: Arc<FhirPathEngine>,
    model_provider: SharedModelProvider,
) -> HashMap<String, DynOperationHandler> {
    let mut handlers: HashMap<String, DynOperationHandler> = HashMap::new();

    // $validate operation
    handlers.insert(
        "validate".to_string(),
        Arc::new(ValidateOperation::new(
            fhirpath_engine.clone(),
            model_provider.clone(),
        )),
    );

    // $meta operations
    handlers.insert("meta".to_string(), Arc::new(MetaOperation));
    handlers.insert("meta-add".to_string(), Arc::new(MetaAddOperation));
    handlers.insert("meta-delete".to_string(), Arc::new(MetaDeleteOperation));

    // $everything operation
    handlers.insert(
        "everything".to_string(),
        Arc::new(EverythingOperation::new()),
    );

    handlers
}
