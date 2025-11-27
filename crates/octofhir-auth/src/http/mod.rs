//! HTTP handlers for OAuth 2.0 endpoints.
//!
//! This module provides Axum handlers for OAuth endpoints.
//!
//! # Available Handlers
//!
//! - [`revoke`] - Token revocation endpoint (RFC 7009)

pub mod revoke;

pub use revoke::revoke_handler;
