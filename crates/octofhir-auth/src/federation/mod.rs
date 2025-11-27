//! External identity provider federation.
//!
//! This module provides integration with external identity providers:
//!
//! - OpenID Connect discovery and validation
//! - External IdP configuration
//! - Token exchange for federated identities
//! - User identity mapping
//! - JWK set fetching and caching
//!
//! # Client JWKS
//!
//! The [`ClientJwksCache`] provides caching for client JWKS used in
//! `private_key_jwt` authentication for backend services.

pub mod client_jwks;

pub use client_jwks::{ClientJwksCache, JwksCacheConfig};
