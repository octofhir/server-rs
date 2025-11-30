//! Admin API types.
//!
//! This module provides types for administrative REST endpoints
//! for managing IdentityProvider and User resources.
//!
//! The actual handlers are implemented in octofhir-server
//! since they need access to storage types from octofhir-auth-postgres.

pub mod types;

pub use types::{
    Bundle, BundleEntry, IdpSearchParams, LinkIdentityRequest, UnlinkIdentityRequest,
    UserSearchParams,
};
