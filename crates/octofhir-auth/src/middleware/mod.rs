//! HTTP middleware for authentication and authorization.
//!
//! This module provides Axum middleware for:
//!
//! - Bearer token extraction and validation
//! - Request authentication
//! - Authorization context injection
//! - FHIR-compliant error responses
//!
//! # Example
//!
//! ```ignore
//! use axum::{Router, routing::get};
//! use octofhir_auth::middleware::{AuthState, BearerAuth};
//!
//! async fn protected_handler(BearerAuth(auth): BearerAuth) -> String {
//!     format!("Hello, {}!", auth.subject())
//! }
//!
//! // Create auth state
//! let auth_state = AuthState::new(
//!     jwt_service,
//!     client_storage,
//!     revoked_token_storage,
//!     user_storage,
//! );
//!
//! let app = Router::new()
//!     .route("/protected", get(protected_handler))
//!     .with_state(auth_state);
//! ```

pub mod auth;
pub mod error;
pub mod types;

pub use auth::{AuthState, BearerAuth, OptionalBearerAuth};
pub use error::operation_outcome_json;
pub use types::{AuthContext, UserContext};
