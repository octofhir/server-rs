//! JWKS endpoint HTTP handler.
//!
//! Provides the `/.well-known/jwks.json` endpoint for JWT verification.
//!
//! # Overview
//!
//! The JWKS (JSON Web Key Set) endpoint allows clients to retrieve the server's
//! public keys for verifying JWTs issued by this server. This is essential for
//! token validation in distributed systems.
//!
//! # References
//!
//! - [RFC 7517 - JSON Web Key](https://tools.ietf.org/html/rfc7517)
//! - [SMART JWKS](https://build.fhir.org/ig/HL7/smart-app-launch/conformance.html#keys)

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;

use crate::token::jwt::JwtService;

/// State for the JWKS endpoint.
#[derive(Clone)]
pub struct JwksState {
    /// The JWT service containing the signing keys.
    pub jwt_service: Arc<JwtService>,
}

impl JwksState {
    /// Creates a new JWKS state.
    pub fn new(jwt_service: Arc<JwtService>) -> Self {
        Self { jwt_service }
    }
}

/// Handler for `GET /.well-known/jwks.json`.
///
/// Returns the server's public keys for JWT verification.
/// Includes cache headers for efficient client caching.
///
/// # Response
///
/// Returns 200 OK with `application/json` content type containing the JWKS document.
/// The response includes a `Cache-Control` header allowing caching for 1 hour.
///
/// # Example Response
///
/// ```json
/// {
///   "keys": [
///     {
///       "kty": "RSA",
///       "kid": "key-1",
///       "use": "sig",
///       "alg": "RS384",
///       "n": "base64url-encoded-modulus",
///       "e": "AQAB"
///     }
///   ]
/// }
/// ```
pub async fn jwks_handler(State(state): State<JwksState>) -> impl IntoResponse {
    let jwks = state.jwt_service.jwks();
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        Json(jwks),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::jwt::{SigningAlgorithm, SigningKeyPair};

    fn create_test_jwt_service() -> Arc<JwtService> {
        let signing_key = SigningKeyPair::generate_rsa(SigningAlgorithm::RS384).unwrap();
        Arc::new(JwtService::new(signing_key, "https://test.example.com"))
    }

    #[test]
    fn test_jwks_state_new() {
        let jwt_service = create_test_jwt_service();
        let state = JwksState::new(jwt_service.clone());

        // Verify the state holds the service
        assert!(Arc::ptr_eq(&state.jwt_service, &jwt_service));
    }

    #[test]
    fn test_jwks_state_clone() {
        let jwt_service = create_test_jwt_service();
        let state = JwksState::new(jwt_service.clone());
        let cloned = state.clone();

        // Cloned state should share the same Arc
        assert!(Arc::ptr_eq(&state.jwt_service, &cloned.jwt_service));
    }

    #[test]
    fn test_jwks_handler_returns_keys() {
        let jwt_service = create_test_jwt_service();
        let jwks = jwt_service.jwks();

        // Should have at least one key
        assert!(!jwks.keys.is_empty());

        // Key should have required fields
        let key = &jwks.keys[0];
        assert_eq!(key.kty, "RSA");
        assert_eq!(key.use_, "sig");
        assert_eq!(key.alg, "RS384");
        assert!(!key.kid.is_empty());
        assert!(key.n.is_some());
        assert!(key.e.is_some());
    }
}
