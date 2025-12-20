//! SQL completion providers
//!
//! This module handles autocompletion for SQL elements, including schema-based
//! completions and FHIR-aware JSONB path suggestions.

mod provider;

pub use provider::*;
