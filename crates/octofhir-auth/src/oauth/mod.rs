//! OAuth 2.0 authorization server implementation.
//!
//! This module provides the core OAuth 2.0 functionality including:
//!
//! - Authorization endpoint handling
//! - Token endpoint handling
//! - Client registration and management
//! - Grant type implementations (authorization code, client credentials, refresh token)
//! - PKCE support for public clients
//!
//! # Authorization Code Flow
//!
//! The authorization code flow is implemented across several submodules:
//!
//! - [`authorize`] - Request/response types for the authorization endpoint
//! - [`session`] - Authorization session management
//! - [`service`] - Authorization service with validation logic
//! - [`pkce`] - PKCE challenge/verifier implementation
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::oauth::{
//!     AuthorizationService, AuthorizationConfig, AuthorizationRequest,
//!     AuthorizationResponse, PkceVerifier, PkceChallenge,
//! };
//!
//! // Client generates PKCE verifier and challenge
//! let verifier = PkceVerifier::generate();
//! let challenge = PkceChallenge::from_verifier(&verifier);
//!
//! // Server processes authorization request
//! let service = AuthorizationService::new(client_storage, session_storage, config);
//! let session = service.authorize(&request).await?;
//!
//! // Build redirect response
//! let response = AuthorizationResponse::new(session.code, request.state);
//! let redirect_url = response.to_redirect_url(&request.redirect_uri)?;
//! ```

pub mod authorize;
pub mod client_assertion;
pub mod client_auth;
pub mod pkce;
pub mod service;
pub mod session;
pub mod token;

// Authorization endpoint types
pub use authorize::{
    AuthorizationError, AuthorizationErrorCode, AuthorizationRequest, AuthorizationResponse,
};

// Client assertion types (for private_key_jwt authentication)
pub use client_assertion::{
    ClientAssertionClaims, ClientAssertionConfig, ClientAssertionValidator, StringOrArray,
};

// Client authentication
pub use client_auth::{
    AuthenticatedClient, TokenEndpointAuthMethod, authenticate_client,
    authenticate_private_key_jwt, parse_basic_auth,
};

// PKCE types
pub use pkce::{PkceChallenge, PkceChallengeMethod, PkceError, PkceVerifier};

// Service types
pub use service::{AuthorizationConfig, AuthorizationService};

// Session types
pub use session::{AuthorizationSession, FhirContextItem, LaunchContext};

// Token endpoint types
pub use token::{TokenError, TokenErrorCode, TokenRequest, TokenResponse};
