//! SQL on FHIR operations.
//!
//! This module provides FHIR operations for executing ViewDefinitions
//! as defined in the SQL on FHIR Implementation Guide.
//!
//! # Operations
//!
//! - `$run` - Execute a ViewDefinition and return tabular results
//! - `$sql` - Generate SQL from a ViewDefinition without executing
//! - `$viewdefinition-export` - Async bulk export of ViewDefinition results to NDJSON
//!
//! # Example
//!
//! ```http
//! POST /fhir/ViewDefinition/$run
//! Content-Type: application/fhir+json
//!
//! {
//!   "resourceType": "Parameters",
//!   "parameter": [
//!     { "name": "viewDefinition", "resource": { /* ViewDefinition */ } },
//!     { "name": "limit", "valueInteger": 100 }
//!   ]
//! }
//! ```

mod engine;
mod expansion;
mod export;
mod output;
mod run;
mod scope;
mod sql;
mod sql_guard;
mod sql_library;
mod sql_run;
mod term_source;
mod terminology;
mod viewdefinition_run;

pub use export::{ViewDefinitionExportOperation, execute_viewdefinition_export};
pub use run::ViewDefinitionRunOperation;
pub use sql::ViewDefinitionSqlOperation;
pub use sql_run::SqlRunOperation;
pub(crate) use term_source::{PROVENANCE_HEADER, PROVENANCE_KEY};
pub use viewdefinition_run::ViewDefinitionRunSpecOperation;

/// Register the SQL on FHIR operations this server implements, so they are
/// available whether or not the SQL on FHIR IG package could be installed:
/// `$run` and `$sql` on ViewDefinition, and `$viewdefinition-run` on system,
/// type and instance level as the IG defines it.
pub fn register_operation_definitions(registry: &mut crate::operations::OperationRegistry) {
    use crate::operations::{OperationDefinition, OperationKind};
    let on_view_definition = |code: &str, url: &str| OperationDefinition {
        code: code.to_string(),
        url: url.to_string(),
        kind: OperationKind::Operation,
        system: false,
        type_level: true,
        instance: true,
        resource: vec!["ViewDefinition".to_string()],
        parameters: vec![],
        affects_state: false,
    };
    registry.register(on_view_definition(
        "run",
        "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-run",
    ));
    registry.register(on_view_definition(
        "sql",
        "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-sql",
    ));
    registry.register(OperationDefinition {
        system: true,
        ..on_view_definition(
            "viewdefinition-run",
            "http://sql-on-fhir.org/OperationDefinition/$viewdefinition-run",
        )
    });
    registry.register(OperationDefinition {
        system: true,
        type_level: false,
        instance: false,
        resource: vec![],
        ..on_view_definition(
            "sql-run",
            "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/SqlRun",
        )
    });
}
