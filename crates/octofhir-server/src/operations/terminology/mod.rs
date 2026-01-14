//! Terminology Operations Module
//!
//! This module provides FHIR terminology service operations:
//! - `$expand` - Expand a ValueSet to its enumerated codes
//! - `$validate-code` - Validate a code against a CodeSystem or ValueSet
//! - `$lookup` - Look up a code in a CodeSystem
//! - `$subsumes` - Test subsumption relationship between codes
//! - `$translate` - Translate a code using ConceptMap
//! - `$closure` - Maintain a transitive closure table for concept hierarchies
//!
//! ## Caching
//!
//! The `cache` module provides an LRU cache for CodeSystem and ConceptMap
//! resources to avoid repeated canonical manager lookups.

pub mod cache;
pub mod closure;
pub mod expand;
pub mod lookup;
pub mod subsumes;
pub mod translate;
pub mod validate_code;

pub use cache::{TerminologyResourceCache, get_cache};
pub use closure::ClosureOperation;
pub use expand::ExpandOperation;
pub use lookup::LookupOperation;
pub use subsumes::SubsumesOperation;
pub use translate::TranslateOperation;
pub use validate_code::ValidateCodeOperation;
