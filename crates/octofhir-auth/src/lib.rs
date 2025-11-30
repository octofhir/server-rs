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
//! - [`config`] - Authentication and authorization configuration
//! - [`oauth`] - OAuth 2.0 authorization server implementation
//! - [`token`] - Token generation, validation, and management
//! - [`smart`] - SMART on FHIR scopes and launch contexts
//! - [`federation`] - External identity provider integration
//! - [`policy`] - AccessPolicy engine for fine-grained authorization
//! - [`middleware`] - HTTP middleware for authentication/authorization
//! - [`audit`] - Security event audit logging
//! - [`storage`] - Storage traits for auth-related data
//! - [`http`] - Axum HTTP handlers for OAuth endpoints

pub mod audit;
pub mod config;
pub mod error;
pub mod federation;
pub mod http;
pub mod middleware;
pub mod oauth;
pub mod policy;
pub mod smart;
pub mod storage;
pub mod token;
pub mod types;

pub use config::{AuthConfig, ConfigError};
pub use error::{AuthError, ErrorCategory};
pub use http::{
    CreateLaunchRequest, CreateLaunchResponse, JwksState, LaunchState, SmartConfigState,
    UserInfoResponse, create_launch_handler, introspect_handler, jwks_handler, revoke_handler,
    smart_configuration_handler, userinfo_handler,
};
pub use middleware::{AuthContext, AuthState, BearerAuth, OptionalBearerAuth, UserContext};
pub use smart::{
    CapabilitySecurityBuilder, ConformanceError, DEFAULT_LAUNCH_CONTEXT_TTL, FhirContextItem,
    SmartConfiguration, SmartScopes, StandaloneContextRequirements, StoredLaunchContext,
    add_smart_security, generate_launch_id,
};
pub use storage::{
    ClientStorage, JtiStorage, LaunchContextStorage, RefreshTokenStorage, RevokedTokenStorage,
    SessionStorage, User, UserStorage,
};
pub use types::{Client, ClientValidationError, GrantType, RefreshToken};

/// Type alias for authentication/authorization results.
pub type AuthResult<T> = Result<T, AuthError>;

/// Prelude module for convenient imports.
///
/// ```ignore
/// use octofhir_auth::prelude::*;
/// ```
pub mod prelude {
    pub use crate::AuthResult;
    pub use crate::config::{AuthConfig, ConfigError};
    pub use crate::error::{AuthError, ErrorCategory};
    pub use crate::http::{
        CreateLaunchRequest, CreateLaunchResponse, JwksState, LaunchState, SmartConfigState,
        UserInfoResponse, create_launch_handler, introspect_handler, jwks_handler, revoke_handler,
        smart_configuration_handler, userinfo_handler,
    };
    pub use crate::middleware::{
        AuthContext, AuthState, BearerAuth, OptionalBearerAuth, UserContext,
    };
    pub use crate::smart::{
        CapabilitySecurityBuilder, ConformanceError, DEFAULT_LAUNCH_CONTEXT_TTL, FhirContextItem,
        SmartConfiguration, SmartScopes, StandaloneContextRequirements, StoredLaunchContext,
        add_smart_security, generate_launch_id,
    };
    pub use crate::storage::{
        ClientStorage, JtiStorage, LaunchContextStorage, RefreshTokenStorage, RevokedTokenStorage,
        SessionStorage, User, UserStorage,
    };
    pub use crate::types::{Client, ClientValidationError, GrantType, RefreshToken};
}
