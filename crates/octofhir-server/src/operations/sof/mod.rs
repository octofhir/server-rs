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
mod export;
mod output;
mod rejection;
mod run;
mod scope;
mod sql;
mod sql_export;
mod sql_plan;
mod sql_run;
pub(crate) mod term_source;
mod viewdefinition_run;

pub use export::{ViewDefinitionExportOperation, execute_viewdefinition_export};
pub use run::ViewDefinitionRunOperation;
pub use sql::ViewDefinitionSqlOperation;
pub use sql_export::{SqlExportOperation, execute_sql_export};
pub(crate) use sql_export::{JOB_TYPE as SQL_EXPORT_JOB, locations as sql_export_locations, media_type as sql_export_media_type};
pub use sql_run::SqlRunOperation;
pub(crate) use output::ENVELOPE_KEY;
pub(crate) use term_source::{
    PROVENANCE_HEADER, PROVENANCE_KEY, REMOTE_COUNT, ServerTerminology, packaged, stored,
    terminology_server,
};
pub use viewdefinition_run::ViewDefinitionRunSpecOperation;

/// Register the SQL on FHIR operations this server implements, so they are
/// available whether or not the SQL on FHIR IG package could be installed.
///
/// `$sql-run` and `$sql-export` cite this server's own OperationDefinitions
/// (`igs/octofhir-sof`), whose `base` is the SQL on FHIR one: the server
/// supports every parameter but `source`, and SQL on FHIR asks a server
/// supporting a subset to say so that way. The console's `$run` / `$sql` on
/// ViewDefinition and `$viewdefinition-run` (a continuous-build operation
/// that was never published) stay callable but are not advertised: their
/// definitions carry no canonical.
/// This server's `$sql-run` definition (base: SQL on FHIR SQLRun).
pub(crate) const SQL_RUN_DEFINITION: &str = "http://octofhir.io/OperationDefinition/sql-run";

/// This server's `$sql-export` definition (base: SQL on FHIR SQLExport).
pub(crate) const SQL_EXPORT_DEFINITION: &str = "http://octofhir.io/OperationDefinition/sql-export";

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
    registry.replace(on_view_definition("run", ""));
    registry.replace(on_view_definition("sql", ""));
    registry.replace(OperationDefinition {
        system: true,
        ..on_view_definition("viewdefinition-run", "")
    });
    registry.replace(OperationDefinition {
        system: true,
        type_level: false,
        instance: false,
        resource: vec![],
        ..on_view_definition(
            "sql-run",
            SQL_RUN_DEFINITION,
        )
    });
    registry.replace(OperationDefinition {
        system: true,
        type_level: false,
        instance: false,
        resource: vec![],
        affects_state: true,
        ..on_view_definition(
            "sql-export",
            SQL_EXPORT_DEFINITION,
        )
    });
}
