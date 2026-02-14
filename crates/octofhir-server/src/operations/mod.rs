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

pub mod auth_session;
pub mod bulk;
pub mod cql;
pub mod definition;
pub mod evaluate_measure;
pub mod everything;
pub mod fhirpath;
pub mod handler;
pub mod loader;
pub mod meta;
pub mod notifications;
pub mod params;
pub mod registry;
pub mod router;
pub mod sof;
pub mod sql;
pub mod terminology;
pub mod validate;

// Re-export main types for convenience
pub use bulk::{
    BulkExportJob, BulkExportLevel, BulkExportManifest, BulkExportStatus, ExportOperation,
    cleanup_expired_exports, execute_bulk_export,
};
pub use cql::CqlOperation;
pub use definition::{OperationDefinition, OperationKind, OperationParameter, ParameterUse};
pub use evaluate_measure::EvaluateMeasureOperation;
pub use everything::EverythingOperation;
pub use fhirpath::FhirPathOperation;
pub use handler::{DynOperationHandler, OperationError, OperationHandler};
pub use loader::{LoadError, load_operations};
pub use meta::{MetaAddOperation, MetaDeleteOperation, MetaOperation};
pub use params::OperationParams;
pub use registry::OperationRegistry;
pub use router::{
    instance_operation_handler, instance_operation_or_history_handler, is_operation,
    merged_root_get_handler, merged_root_post_handler, merged_type_get_handler,
    merged_type_post_handler, system_operation_handler, type_operation_handler,
};
pub use sof::{
    ViewDefinitionRunOperation, ViewDefinitionSqlOperation, execute_viewdefinition_export,
};
pub use terminology::{
    ClosureOperation, ExpandOperation, LookupOperation, SubsumesOperation, TranslateOperation,
    ValidateCodeOperation,
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
/// - `$export` - Bulk data export (system, patient, group, ViewDefinition level)
/// - `$run` - Execute ViewDefinition synchronously (SQL on FHIR)
/// - `$sql` - Generate SQL from ViewDefinition (SQL on FHIR)
///
/// # Arguments
///
/// * `fhirpath_engine` - The FHIRPath engine for validation constraints
/// * `model_provider` - The schema provider for type information
/// * `bulk_export_config` - Configuration for bulk export operations
///
/// # Returns
///
/// A HashMap mapping operation codes to their handlers.
pub fn register_core_operations(
    fhirpath_engine: Arc<FhirPathEngine>,
    model_provider: SharedModelProvider,
) -> HashMap<String, DynOperationHandler> {
    register_core_operations_with_config(
        fhirpath_engine,
        model_provider,
        crate::config::BulkExportConfig::default(),
    )
}

/// Registers core operations with explicit bulk export configuration.
pub fn register_core_operations_with_config(
    fhirpath_engine: Arc<FhirPathEngine>,
    model_provider: SharedModelProvider,
    bulk_export_config: crate::config::BulkExportConfig,
) -> HashMap<String, DynOperationHandler> {
    register_core_operations_full(
        fhirpath_engine,
        model_provider,
        bulk_export_config,
        crate::config::SqlOnFhirConfig::default(),
        false, // CQL disabled by default
    )
}

/// Registers core operations with all configuration options.
pub fn register_core_operations_full(
    fhirpath_engine: Arc<FhirPathEngine>,
    model_provider: SharedModelProvider,
    bulk_export_config: crate::config::BulkExportConfig,
    sql_on_fhir_config: crate::config::SqlOnFhirConfig,
    cql_enabled: bool,
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

    // Terminology operations
    handlers.insert("expand".to_string(), Arc::new(ExpandOperation::new()));
    handlers.insert(
        "validate-code".to_string(),
        Arc::new(ValidateCodeOperation::new()),
    );
    handlers.insert("lookup".to_string(), Arc::new(LookupOperation::new()));
    handlers.insert("subsumes".to_string(), Arc::new(SubsumesOperation::new()));
    handlers.insert("translate".to_string(), Arc::new(TranslateOperation::new()));
    handlers.insert("closure".to_string(), Arc::new(ClosureOperation::new()));

    // $export operation (Bulk Data Access + ViewDefinition/SQL on FHIR)
    // The unified ExportOperation handles:
    // - /$export (system level)
    // - /Patient/$export (patient level)
    // - /Group/{id}/$export (group level)
    // - /ViewDefinition/$export (SQL on FHIR type level)
    // - /ViewDefinition/{id}/$export (SQL on FHIR instance level)
    if bulk_export_config.enabled || sql_on_fhir_config.enabled {
        handlers.insert(
            "export".to_string(),
            Arc::new(ExportOperation::with_sql_on_fhir(
                bulk_export_config,
                sql_on_fhir_config.enabled,
            )),
        );
    }

    // SQL on FHIR operations ($run, $sql on ViewDefinition)
    // Always register - the handlers check if feature is enabled
    handlers.insert(
        "run".to_string(),
        Arc::new(ViewDefinitionRunOperation::new(sql_on_fhir_config.enabled)),
    );
    handlers.insert(
        "sql".to_string(),
        Arc::new(ViewDefinitionSqlOperation::new(sql_on_fhir_config.enabled)),
    );

    // Notification operations
    handlers.insert(
        "resend".to_string(),
        Arc::new(notifications::ResendOperation),
    );
    handlers.insert(
        "resend-all".to_string(),
        Arc::new(notifications::ResendAllOperation),
    );
    handlers.insert("stats".to_string(), Arc::new(notifications::StatsOperation));
    handlers.insert(
        "cancel".to_string(),
        Arc::new(notifications::CancelOperation),
    );

    // Subscription $status operation
    handlers.insert(
        "status".to_string(),
        Arc::new(crate::subscriptions::operations::StatusOperation),
    );

    // $fhirpath operation
    handlers.insert("fhirpath".to_string(), Arc::new(FhirPathOperation::new()));

    // CQL operations ($cql, $evaluate-measure) - only if enabled
    if cql_enabled {
        tracing::info!("Registering CQL operations (cql_enabled=true)");
        handlers.insert("cql".to_string(), Arc::new(CqlOperation::new()));
        handlers.insert(
            "evaluate-measure".to_string(),
            Arc::new(EvaluateMeasureOperation::new()),
        );
        tracing::info!("CQL operations registered: cql, evaluate-measure");
    } else {
        tracing::info!("CQL operations NOT registered (cql_enabled=false)");
    }

    handlers
}
