//! Storage traits for authentication and authorization data.
//!
//! This module defines storage interfaces for:
//!
//! - OAuth client registrations
//! - Authorization codes and sessions
//! - Authorize flow sessions (login/consent UI)
//! - User consent records
//! - Access and refresh tokens
//! - JWT ID tracking (replay prevention)
//! - Revoked access token tracking
//! - User sessions
//! - User management
//! - Role management
//! - AccessPolicy resources
//! - SMART launch contexts
//! - Basic authentication (Client and App)
//!
//! # Implementations
//!
//! Storage implementations are provided in separate crates:
//!
//! - `octofhir-auth-postgres` - PostgreSQL storage backend

pub mod authorize_session;
pub mod basic_auth;
pub mod client;
pub mod consent;
pub mod jti;
pub mod launch_context;
pub mod policy;
pub mod refresh_token;
pub mod revoked_token;
pub mod role;
pub mod session;
pub mod sso_session;
pub mod user;

pub use authorize_session::AuthorizeSessionStorage;
pub use basic_auth::BasicAuthStorage;
pub use client::ClientStorage;
pub use consent::{ConsentStorage, UserConsent};
pub use jti::JtiStorage;
pub use launch_context::LaunchContextStorage;
pub use policy::{PolicySearchParams, PolicyStorage};
pub use refresh_token::RefreshTokenStorage;
pub use revoked_token::RevokedTokenStorage;
pub use role::{Permission, Role, RoleStorage, default_permissions};
pub use session::SessionStorage;
pub use sso_session::SsoSessionStorage;
pub use user::{User, UserStorage};
