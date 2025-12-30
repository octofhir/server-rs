//! Terminology Operations Module
//!
//! This module provides FHIR terminology service operations:
//! - `$expand` - Expand a ValueSet to its enumerated codes
//! - `$validate-code` - Validate a code against a CodeSystem or ValueSet
//! - `$lookup` - Look up a code in a CodeSystem (TODO)

pub mod expand;
pub mod validate_code;

pub use expand::ExpandOperation;
pub use validate_code::ValidateCodeOperation;
