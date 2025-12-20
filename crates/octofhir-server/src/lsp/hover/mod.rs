//! Hover information provider
//!
//! This module handles hover tooltips for SQL elements, including tables, columns,
//! functions, and FHIR-specific JSONB paths.

mod provider;

pub use provider::*;
