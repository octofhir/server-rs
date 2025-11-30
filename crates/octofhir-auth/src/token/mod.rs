//! Token generation, validation, and management.
//!
//! This module provides:
//!
//! - Access token generation and validation
//! - Refresh token handling
//! - Token introspection (RFC 7662)
//! - Token revocation (RFC 7009)
//! - JWT encoding and decoding

pub mod introspection;
pub mod jwt;
pub mod revocation;
pub mod service;

pub use introspection::{
    IntrospectionError, IntrospectionErrorCode, IntrospectionRequest, IntrospectionResponse,
};
pub use jwt::{
    AccessTokenClaims, AccessTokenClaimsBuilder, IdTokenClaims, Jwk, Jwks, JwtError, JwtService,
    SigningAlgorithm, SigningKeyPair,
};
pub use revocation::{RevocationError, RevocationErrorCode, RevocationRequest, TokenTypeHint};
pub use service::{TokenConfig, TokenService};
