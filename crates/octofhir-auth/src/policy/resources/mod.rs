//! AccessPolicy resource types.
//!
//! This module provides the FHIR-like AccessPolicy resource for configuring
//! access control policies with pattern matching and scriptable engines.

pub mod access_policy;

pub use access_policy::{
    AccessPolicy, ConversionError, EngineElement, InternalPolicy, MatcherElement, PolicyEngine,
    PolicyEngineType, ResourceMeta, ValidationError,
};
