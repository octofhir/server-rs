//! SQL on FHIR implementation for OctoFHIR.
//!
//! This crate provides support for the SQL on FHIR specification, which enables
//! transformation of FHIR resources into tabular data using ViewDefinition resources.
//!
//! # Overview
//!
//! SQL on FHIR allows you to define views over FHIR resources that can be materialized
//! as relational tables. This is useful for analytics, reporting, and integration with
//! SQL-based tools.
//!
//! # Components
//!
//! - [`ViewDefinition`] - Parsed representation of a FHIR ViewDefinition resource
//! - [`SqlGenerator`] - Generates SQL from ViewDefinitions
//! - [`ViewRunner`] - Executes views against a PostgreSQL database
//!
//! # Example
//!
//! ```ignore
//! use octofhir_sof::{ViewDefinition, SqlGenerator, ViewRunner};
//!
//! // Parse a ViewDefinition
//! let view_def = ViewDefinition::from_json(&json)?;
//!
//! // Generate SQL
//! let generator = SqlGenerator::new();
//! let sql = generator.generate(&view_def)?;
//!
//! // Execute the view
//! let runner = ViewRunner::new(pool, generator);
//! let result = runner.run(&view_def).await?;
//! ```
//!
//! # SQL on FHIR Specification
//!
//! See: <https://build.fhir.org/ig/FHIR/sql-on-fhir-v2/>

mod column;
pub mod output;
mod runner;
mod sql_generator;
mod view_definition;

pub use column::{ColumnInfo, ColumnType};
pub use runner::{ViewResult, ViewRunner};
pub use sql_generator::{GeneratedColumn, GeneratedSql, SqlGenerator};
pub use view_definition::{Column, Constant, SelectColumn, ViewDefinition, WhereClause};

use thiserror::Error;

/// Errors that can occur during SQL on FHIR operations.
#[derive(Debug, Error)]
pub enum Error {
    /// The ViewDefinition JSON is invalid or missing required fields.
    #[error("Invalid ViewDefinition: {0}")]
    InvalidViewDefinition(String),

    /// A FHIRPath expression is invalid or cannot be converted to SQL.
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// An error occurred while executing SQL.
    #[error("SQL execution error: {0}")]
    Sql(#[from] sqlx_core::error::Error),

    /// An error occurred during FHIRPath evaluation or compilation.
    #[error("FHIRPath error: {0}")]
    FhirPath(String),

    /// An error occurred while generating output.
    #[error("Output error: {0}")]
    Output(String),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type alias using the crate's Error type.
pub type Result<T> = std::result::Result<T, Error>;
