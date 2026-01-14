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

mod export;
mod run;
mod sql;

pub use export::{ViewDefinitionExportOperation, execute_viewdefinition_export};
pub use run::ViewDefinitionRunOperation;
pub use sql::ViewDefinitionSqlOperation;
