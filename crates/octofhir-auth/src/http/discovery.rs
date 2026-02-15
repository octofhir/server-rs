//! SMART configuration discovery HTTP handler.
//!
//! Provides the `/.well-known/smart-configuration` endpoint for SMART app launch.

use axum::Json;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use url::Url;

use crate::config::AuthConfig;
use crate::smart::discovery::SmartConfiguration;

/// State for the SMART configuration endpoint.
#[derive(Clone)]
pub struct SmartConfigState {
    /// Authentication configuration.
    pub config: AuthConfig,
    /// Base URL of the FHIR server.
    pub base_url: Url,
}

impl SmartConfigState {
    /// Creates a new SMART configuration state.
    pub fn new(config: AuthConfig, base_url: Url) -> Self {
        Self { config, base_url }
    }
}

/// Handler for `GET /.well-known/smart-configuration`.
///
/// Returns a JSON document describing the server's SMART on FHIR capabilities,
/// OAuth endpoints, and supported features.
///
/// # Response
///
/// Returns 200 OK with `application/json` content type containing the
/// SMART configuration document.
///
/// # Example
///
/// ```text
/// GET /.well-known/smart-configuration HTTP/1.1
/// Host: fhir.example.com
///
/// HTTP/1.1 200 OK
/// Content-Type: application/json
///
/// {
///   "authorization_endpoint": "https://fhir.example.com/auth/authorize",
///   "token_endpoint": "https://fhir.example.com/auth/token",
///   "capabilities": ["launch-ehr", "launch-standalone", "client-public", "permission-v2"],
///   ...
/// }
/// ```
pub async fn smart_configuration_handler(
    State(state): State<SmartConfigState>,
) -> impl IntoResponse {
    let config = SmartConfiguration::build(&state.config, &state.base_url);
    ([(header::CONTENT_TYPE, "application/json")], Json(config))
}

/// Handler for `GET /.well-known/openid-configuration`.
///
/// Returns an OpenID Connect Discovery document. Uses the configured
/// `base_url` for all endpoint URLs, not the bind address.
pub async fn openid_configuration_handler(
    State(state): State<SmartConfigState>,
) -> impl IntoResponse {
    let base = state.base_url.as_str().trim_end_matches('/');

    let doc = serde_json::json!({
        "issuer": base,
        "authorization_endpoint": format!("{}/auth/authorize", base),
        "token_endpoint": format!("{}/auth/token", base),
        "userinfo_endpoint": format!("{}/auth/userinfo", base),
        "jwks_uri": format!("{}/.well-known/jwks.json", base),
        "scopes_supported": state.config.smart.supported_scopes,
        "response_types_supported": ["code"],
        "grant_types_supported": state.config.oauth.grant_types,
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": [state.config.signing.algorithm.to_string()],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post", "private_key_jwt"],
        "introspection_endpoint": format!("{}/auth/introspect", base),
        "revocation_endpoint": format!("{}/auth/revoke", base),
    });

    ([(header::CONTENT_TYPE, "application/json")], Json(doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_config_state_new() {
        let config = AuthConfig::default();
        let base_url = Url::parse("https://fhir.example.com").unwrap();

        let state = SmartConfigState::new(config.clone(), base_url.clone());

        assert_eq!(state.config.issuer, config.issuer);
        assert_eq!(state.base_url.as_str(), "https://fhir.example.com/");
    }
}
