//! # octofhir-auth
//!
//! Authentication and authorization module for the OctoFHIR server.
//!
//! This crate provides:
//! - OAuth 2.0 authorization server implementation
//! - SMART on FHIR compliance
//! - External identity provider federation
//! - AccessPolicy engine with scriptable policies
//! - Token management and validation
//! - Audit logging for security events
//!
//! ## Overview
//!
//! The authentication system is designed around the OAuth 2.0 framework with
//! SMART on FHIR extensions for healthcare-specific authorization patterns.
//!
//! ## Modules
//!
//! - [`oauth`] - OAuth 2.0 authorization server implementation
//! - [`token`] - Token generation, validation, and management
//! - [`smart`] - SMART on FHIR scopes and launch contexts
//! - [`federation`] - External identity provider integration
//! - [`policy`] - AccessPolicy engine for fine-grained authorization
//! - [`middleware`] - HTTP middleware for authentication/authorization
//! - [`audit`] - Security event audit logging
//! - [`storage`] - Storage traits for auth-related data

pub mod audit;
pub mod error;
pub mod federation;
pub mod middleware;
pub mod oauth;
pub mod policy;
pub mod smart;
pub mod storage;
pub mod token;
pub mod types;

pub use error::{AuthError, ErrorCategory};

/// Type alias for authentication/authorization results.
pub type AuthResult<T> = Result<T, AuthError>;

/// Prelude module for convenient imports.
///
/// ```ignore
/// use octofhir_auth::prelude::*;
/// ```
pub mod prelude {
    pub use crate::AuthResult;
    pub use crate::error::{AuthError, ErrorCategory};
}
