//! Storage traits for authentication and authorization data.
//!
//! This module defines storage interfaces for:
//!
//! - OAuth client registrations
//! - Authorization codes and sessions
//! - Access and refresh tokens
//! - JWT ID tracking (replay prevention)
//! - Revoked access token tracking
//! - User sessions
//! - User management
//! - AccessPolicy resources
//! - SMART launch contexts
//!
//! # Implementations
//!
//! Storage implementations are provided in separate crates:
//!
//! - `octofhir-auth-postgres` - PostgreSQL storage backend

pub mod client;
pub mod jti;
pub mod launch_context;
pub mod refresh_token;
pub mod revoked_token;
pub mod session;
pub mod user;

pub use client::ClientStorage;
pub use jti::JtiStorage;
pub use launch_context::LaunchContextStorage;
pub use refresh_token::RefreshTokenStorage;
pub use revoked_token::RevokedTokenStorage;
pub use session::SessionStorage;
pub use user::{User, UserStorage};
