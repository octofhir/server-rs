//! FHIR resource types for federation.
//!
//! This module provides Rust types for custom FHIR resources used in
//! identity federation.

pub mod identity_provider;
pub mod user;

pub use identity_provider::{
    IdentityProviderResource, IdentityProviderType, UserMappingElement, ValidationError,
};
pub use user::{
    Reference, UserIdentityElement, UserResource, ValidationError as UserValidationError,
};
