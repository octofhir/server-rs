//! OpenID Connect Discovery Document types.
//!
//! This module defines the data structures for OIDC provider metadata
//! as specified in [OpenID Connect Discovery 1.0](https://openid.net/specs/openid-connect-discovery-1_0.html).

use serde::{Deserialize, Serialize};

/// OpenID Connect Discovery Document.
///
/// Contains the provider metadata returned from the `.well-known/openid-configuration`
/// endpoint. This structure includes all fields defined in the OIDC Discovery 1.0
/// specification.
///
/// # Example
///
/// ```ignore
/// use octofhir_auth::federation::oidc::OidcDiscoveryDocument;
///
/// let json = r#"{
///     "issuer": "https://auth.example.com",
///     "authorization_endpoint": "https://auth.example.com/authorize",
///     "token_endpoint": "https://auth.example.com/token",
///     "jwks_uri": "https://auth.example.com/.well-known/jwks.json",
///     "response_types_supported": ["code"],
///     "subject_types_supported": ["public"],
///     "id_token_signing_alg_values_supported": ["RS256"]
/// }"#;
///
/// let doc: OidcDiscoveryDocument = serde_json::from_str(json)?;
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcDiscoveryDocument {
    // ----- Required Fields -----
    /// URL that the OP asserts as its Issuer Identifier.
    pub issuer: String,

    /// URL of the OP's Authorization Endpoint.
    pub authorization_endpoint: String,

    /// URL of the OP's Token Endpoint.
    pub token_endpoint: String,

    /// URL of the OP's JSON Web Key Set document.
    pub jwks_uri: String,

    /// JSON array containing a list of the OAuth 2.0 response_type values
    /// that this OP supports.
    pub response_types_supported: Vec<String>,

    /// JSON array containing a list of the Subject Identifier types that
    /// this OP supports. Valid types include `pairwise` and `public`.
    pub subject_types_supported: Vec<String>,

    /// JSON array containing a list of the JWS signing algorithms (alg values)
    /// supported by the OP for the ID Token.
    pub id_token_signing_alg_values_supported: Vec<String>,

    // ----- Recommended Fields -----
    /// URL of the OP's UserInfo Endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userinfo_endpoint: Option<String>,

    /// JSON array containing a list of the OAuth 2.0 scope values that
    /// this server supports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,

    /// JSON array containing a list of the Claim Names of the Claims
    /// that the OpenID Provider MAY be able to supply values for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims_supported: Option<Vec<String>>,

    // ----- Optional Fields -----
    /// URL of the OP's Dynamic Client Registration Endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<String>,

    /// JSON array containing a list of Client Authentication methods
    /// supported by this Token Endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,

    /// JSON array containing a list of the JWS signing algorithms supported
    /// by the Token Endpoint for the signature on the JWT used to
    /// authenticate the Client at the Token Endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_signing_alg_values_supported: Option<Vec<String>>,

    /// JSON array containing a list of the OAuth 2.0 Grant Type values
    /// that this OP supports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_types_supported: Option<Vec<String>>,

    /// JSON array containing a list of the Authentication Context Class
    /// References that this OP supports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acr_values_supported: Option<Vec<String>>,

    /// JSON array containing a list of the OAuth 2.0 response_mode values
    /// that this OP supports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_modes_supported: Option<Vec<String>>,

    /// JSON array containing a list of PKCE code challenge methods
    /// supported by this authorization server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_challenge_methods_supported: Option<Vec<String>>,

    /// URL of the authorization server's OAuth 2.0 revocation endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,

    /// URL of the authorization server's OAuth 2.0 introspection endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introspection_endpoint: Option<String>,

    /// URL at the OP to which an RP can perform a redirect to request that
    /// the End-User be logged out at the OP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_session_endpoint: Option<String>,

    /// JSON array containing a list of the JWS alg values supported by
    /// the OP for Request Objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_object_signing_alg_values_supported: Option<Vec<String>>,

    /// Boolean value specifying whether the OP supports use of the request parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_parameter_supported: Option<bool>,

    /// Boolean value specifying whether the OP supports use of the request_uri parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_uri_parameter_supported: Option<bool>,

    /// Boolean value specifying whether the OP requires any request_uri values
    /// used to be pre-registered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_request_uri_registration: Option<bool>,

    /// Boolean value specifying whether the OP supports the claims parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims_parameter_supported: Option<bool>,

    /// URL of a page containing human-readable information that developers
    /// might want or need to know when using the OpenID Provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_documentation: Option<String>,

    /// Languages and scripts supported for the user interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_locales_supported: Option<Vec<String>>,

    /// URL of the OP's policy concerning end-users' data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_policy_uri: Option<String>,

    /// URL of the OP's terms of service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_tos_uri: Option<String>,

    /// URL of the OP's Pushed Authorization Request Endpoint (RFC 9126).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_authorization_request_endpoint: Option<String>,

    /// Boolean indicating the authorization server accepts authorization
    /// requests only via PAR.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_pushed_authorization_requests: Option<bool>,
}

impl OidcDiscoveryDocument {
    /// Returns `true` if this provider supports the specified grant type.
    #[must_use]
    pub fn supports_grant_type(&self, grant_type: &str) -> bool {
        self.grant_types_supported
            .as_ref()
            .is_some_and(|grants| grants.iter().any(|g| g == grant_type))
    }

    /// Returns `true` if this provider supports the specified response type.
    #[must_use]
    pub fn supports_response_type(&self, response_type: &str) -> bool {
        self.response_types_supported
            .iter()
            .any(|rt| rt == response_type)
    }

    /// Returns `true` if this provider supports the specified scope.
    #[must_use]
    pub fn supports_scope(&self, scope: &str) -> bool {
        self.scopes_supported
            .as_ref()
            .is_some_and(|scopes| scopes.iter().any(|s| s == scope))
    }

    /// Returns `true` if this provider supports PKCE with the specified method.
    #[must_use]
    pub fn supports_pkce_method(&self, method: &str) -> bool {
        self.code_challenge_methods_supported
            .as_ref()
            .is_some_and(|methods| methods.iter().any(|m| m == method))
    }

    /// Returns `true` if this provider supports the specified token endpoint
    /// authentication method.
    #[must_use]
    pub fn supports_token_auth_method(&self, method: &str) -> bool {
        self.token_endpoint_auth_methods_supported
            .as_ref()
            .is_some_and(|methods| methods.iter().any(|m| m == method))
    }

    /// Returns `true` if this provider supports the `authorization_code` grant type.
    ///
    /// Note: If `grant_types_supported` is not present, the default per OIDC spec
    /// is that only `authorization_code` and `implicit` are supported.
    #[must_use]
    pub fn supports_authorization_code(&self) -> bool {
        match &self.grant_types_supported {
            Some(grants) => grants.iter().any(|g| g == "authorization_code"),
            None => true, // Default per OIDC spec
        }
    }

    /// Returns `true` if this provider supports the `client_credentials` grant type.
    #[must_use]
    pub fn supports_client_credentials(&self) -> bool {
        self.supports_grant_type("client_credentials")
    }

    /// Returns `true` if this provider supports the `refresh_token` grant type.
    #[must_use]
    pub fn supports_refresh_token(&self) -> bool {
        self.supports_grant_type("refresh_token")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_discovery_doc() -> OidcDiscoveryDocument {
        OidcDiscoveryDocument {
            issuer: "https://auth.example.com".to_string(),
            authorization_endpoint: "https://auth.example.com/authorize".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            jwks_uri: "https://auth.example.com/.well-known/jwks.json".to_string(),
            response_types_supported: vec!["code".to_string()],
            subject_types_supported: vec!["public".to_string()],
            id_token_signing_alg_values_supported: vec!["RS256".to_string()],
            userinfo_endpoint: None,
            scopes_supported: None,
            claims_supported: None,
            registration_endpoint: None,
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            grant_types_supported: None,
            acr_values_supported: None,
            response_modes_supported: None,
            code_challenge_methods_supported: None,
            revocation_endpoint: None,
            introspection_endpoint: None,
            end_session_endpoint: None,
            request_object_signing_alg_values_supported: None,
            request_parameter_supported: None,
            request_uri_parameter_supported: None,
            require_request_uri_registration: None,
            claims_parameter_supported: None,
            service_documentation: None,
            ui_locales_supported: None,
            op_policy_uri: None,
            op_tos_uri: None,
            pushed_authorization_request_endpoint: None,
            require_pushed_authorization_requests: None,
        }
    }

    #[test]
    fn test_parse_minimal_document() {
        let json = r#"{
            "issuer": "https://auth.example.com",
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token",
            "jwks_uri": "https://auth.example.com/.well-known/jwks.json",
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"]
        }"#;

        let doc: OidcDiscoveryDocument = serde_json::from_str(json).unwrap();

        assert_eq!(doc.issuer, "https://auth.example.com");
        assert_eq!(
            doc.authorization_endpoint,
            "https://auth.example.com/authorize"
        );
        assert_eq!(doc.token_endpoint, "https://auth.example.com/token");
        assert_eq!(
            doc.jwks_uri,
            "https://auth.example.com/.well-known/jwks.json"
        );
        assert!(doc.response_types_supported.contains(&"code".to_string()));
        assert!(doc.subject_types_supported.contains(&"public".to_string()));
        assert!(
            doc.id_token_signing_alg_values_supported
                .contains(&"RS256".to_string())
        );

        // Optional fields should be None
        assert!(doc.userinfo_endpoint.is_none());
        assert!(doc.scopes_supported.is_none());
        assert!(doc.grant_types_supported.is_none());
    }

    #[test]
    fn test_parse_full_document() {
        let json = r#"{
            "issuer": "https://auth.example.com",
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token",
            "jwks_uri": "https://auth.example.com/.well-known/jwks.json",
            "userinfo_endpoint": "https://auth.example.com/userinfo",
            "response_types_supported": ["code", "token", "id_token"],
            "subject_types_supported": ["public", "pairwise"],
            "id_token_signing_alg_values_supported": ["RS256", "ES256"],
            "scopes_supported": ["openid", "profile", "email"],
            "claims_supported": ["sub", "name", "email"],
            "token_endpoint_auth_methods_supported": ["client_secret_basic", "private_key_jwt"],
            "grant_types_supported": ["authorization_code", "refresh_token", "client_credentials"],
            "code_challenge_methods_supported": ["S256", "plain"],
            "revocation_endpoint": "https://auth.example.com/revoke",
            "introspection_endpoint": "https://auth.example.com/introspect",
            "end_session_endpoint": "https://auth.example.com/logout"
        }"#;

        let doc: OidcDiscoveryDocument = serde_json::from_str(json).unwrap();

        assert_eq!(
            doc.userinfo_endpoint,
            Some("https://auth.example.com/userinfo".to_string())
        );
        assert!(
            doc.scopes_supported
                .as_ref()
                .unwrap()
                .contains(&"openid".to_string())
        );
        assert!(
            doc.grant_types_supported
                .as_ref()
                .unwrap()
                .contains(&"refresh_token".to_string())
        );
        assert!(
            doc.code_challenge_methods_supported
                .as_ref()
                .unwrap()
                .contains(&"S256".to_string())
        );
    }

    #[test]
    fn test_serialize_document() {
        let doc = minimal_discovery_doc();
        let json = serde_json::to_string(&doc).unwrap();

        // Verify we can round-trip
        let parsed: OidcDiscoveryDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.issuer, doc.issuer);

        // Verify None fields are not serialized
        assert!(!json.contains("userinfo_endpoint"));
        assert!(!json.contains("scopes_supported"));
    }

    #[test]
    fn test_supports_grant_type() {
        let mut doc = minimal_discovery_doc();

        // When grant_types_supported is None
        assert!(!doc.supports_grant_type("client_credentials"));

        // When grant_types_supported is set
        doc.grant_types_supported = Some(vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
            "client_credentials".to_string(),
        ]);

        assert!(doc.supports_grant_type("authorization_code"));
        assert!(doc.supports_grant_type("refresh_token"));
        assert!(doc.supports_grant_type("client_credentials"));
        assert!(!doc.supports_grant_type("password"));
    }

    #[test]
    fn test_supports_response_type() {
        let doc = minimal_discovery_doc();

        assert!(doc.supports_response_type("code"));
        assert!(!doc.supports_response_type("token"));
    }

    #[test]
    fn test_supports_scope() {
        let mut doc = minimal_discovery_doc();

        // When scopes_supported is None
        assert!(!doc.supports_scope("openid"));

        // When scopes_supported is set
        doc.scopes_supported = Some(vec!["openid".to_string(), "profile".to_string()]);

        assert!(doc.supports_scope("openid"));
        assert!(doc.supports_scope("profile"));
        assert!(!doc.supports_scope("email"));
    }

    #[test]
    fn test_supports_pkce_method() {
        let mut doc = minimal_discovery_doc();

        // When code_challenge_methods_supported is None
        assert!(!doc.supports_pkce_method("S256"));

        // When code_challenge_methods_supported is set
        doc.code_challenge_methods_supported = Some(vec!["S256".to_string()]);

        assert!(doc.supports_pkce_method("S256"));
        assert!(!doc.supports_pkce_method("plain"));
    }

    #[test]
    fn test_supports_authorization_code_default() {
        let doc = minimal_discovery_doc();

        // Per OIDC spec, if grant_types_supported is absent,
        // authorization_code is supported by default
        assert!(doc.supports_authorization_code());
    }

    #[test]
    fn test_supports_authorization_code_explicit() {
        let mut doc = minimal_discovery_doc();
        doc.grant_types_supported = Some(vec!["client_credentials".to_string()]);

        // Only client_credentials is supported, not authorization_code
        assert!(!doc.supports_authorization_code());
    }

    #[test]
    fn test_supports_token_auth_method() {
        let mut doc = minimal_discovery_doc();

        assert!(!doc.supports_token_auth_method("private_key_jwt"));

        doc.token_endpoint_auth_methods_supported = Some(vec![
            "client_secret_basic".to_string(),
            "private_key_jwt".to_string(),
        ]);

        assert!(doc.supports_token_auth_method("client_secret_basic"));
        assert!(doc.supports_token_auth_method("private_key_jwt"));
        assert!(!doc.supports_token_auth_method("client_secret_post"));
    }
}
