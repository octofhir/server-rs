//! OAuth 2.0 Client domain types.
//!
//! This module defines the `Client` struct and related types for OAuth 2.0
//! client registrations. The field names align with the StructureDefinition-Client.json
//! in the internal IG.

use jsonwebtoken::jwk::JwkSet;
use serde::{Deserialize, Serialize};

use crate::smart::scopes::SmartScope;

// =============================================================================
// Grant Type
// =============================================================================

/// OAuth 2.0 grant types.
///
/// Defines the authorization flows a client is allowed to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    /// Authorization Code flow (with PKCE for public clients).
    AuthorizationCode,
    /// Client Credentials flow (confidential clients only).
    ClientCredentials,
    /// Refresh Token flow.
    RefreshToken,
    /// Resource Owner Password Credentials flow.
    /// WARNING: This grant type is considered legacy and should only be used
    /// for trusted first-party applications or migration scenarios.
    Password,
}

impl GrantType {
    /// Returns the OAuth 2.0 grant_type parameter value.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AuthorizationCode => "authorization_code",
            Self::ClientCredentials => "client_credentials",
            Self::RefreshToken => "refresh_token",
            Self::Password => "password",
        }
    }
}

impl std::fmt::Display for GrantType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// Client
// =============================================================================

/// OAuth 2.0 Client resource.
///
/// Represents an OAuth client registration with credentials and configuration.
/// Fields align with StructureDefinition-Client.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    /// Unique client identifier used in OAuth flows.
    pub client_id: String,

    /// BCrypt-hashed client secret (for confidential clients).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,

    /// Human-readable display name.
    pub name: String,

    /// Detailed description of the client application.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// OAuth 2.0 grant types this client is allowed to use.
    pub grant_types: Vec<GrantType>,

    /// Allowed redirect URIs for authorization code flow.
    #[serde(default)]
    pub redirect_uris: Vec<String>,

    /// Allowed post-logout redirect URIs for RP-initiated logout.
    /// Per OIDC RP-Initiated Logout 1.0, the OP MUST validate that
    /// post_logout_redirect_uri matches one of these registered URIs.
    #[serde(default)]
    pub post_logout_redirect_uris: Vec<String>,

    /// OAuth scopes this client is allowed to request.
    /// Empty list means all scopes are allowed.
    #[serde(default)]
    pub scopes: Vec<String>,

    /// Whether this is a confidential client (has client secret).
    pub confidential: bool,

    /// Whether this client is currently active and can be used.
    pub active: bool,

    /// Access token lifetime in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token_lifetime: Option<i64>,

    /// Refresh token lifetime in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token_lifetime: Option<i64>,

    /// Whether PKCE is required for authorization code flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkce_required: Option<bool>,

    /// Origins allowed for CORS requests from browser-based clients.
    #[serde(default)]
    pub allowed_origins: Vec<String>,

    /// Inline JWKS for backend service clients using private_key_jwt authentication.
    /// The keys in this set are the client's public keys used to verify JWT assertions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks: Option<JwkSet>,

    /// JWKS URI for fetching the client's public keys dynamically.
    /// Used when the client rotates keys and provides them via a URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,
}

impl Client {
    /// Validates the client configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the client configuration is invalid.
    pub fn validate(&self) -> Result<(), ClientValidationError> {
        if self.client_id.is_empty() {
            return Err(ClientValidationError::EmptyClientId);
        }

        if self.name.is_empty() {
            return Err(ClientValidationError::EmptyName);
        }

        if self.grant_types.is_empty() {
            return Err(ClientValidationError::NoGrantTypes);
        }

        // Public clients cannot use client_credentials
        if !self.confidential && self.grant_types.contains(&GrantType::ClientCredentials) {
            return Err(ClientValidationError::PublicClientCredentials);
        }

        // Confidential clients must have a client secret
        if self.confidential && self.client_secret.is_none() {
            return Err(ClientValidationError::MissingSecret);
        }

        // Authorization code flow requires redirect URIs
        if self.grant_types.contains(&GrantType::AuthorizationCode) && self.redirect_uris.is_empty()
        {
            return Err(ClientValidationError::NoRedirectUris);
        }

        Ok(())
    }

    /// Checks if the given redirect URI is allowed for this client.
    #[must_use]
    pub fn is_redirect_uri_allowed(&self, uri: &str) -> bool {
        self.redirect_uris.iter().any(|allowed| allowed == uri)
    }

    /// Checks if the given post-logout redirect URI is allowed for this client.
    ///
    /// The comparison ignores query parameters, so if `http://example.com/logout`
    /// is registered, `http://example.com/logout?state=foo` will also be allowed.
    /// This is a practical relaxation of the OIDC spec to support common use cases
    /// like passing logout reason or state via query parameters.
    #[must_use]
    pub fn is_post_logout_redirect_uri_allowed(&self, uri: &str) -> bool {
        // Strip query parameters from the incoming URI for comparison
        let uri_without_query = uri.split('?').next().unwrap_or(uri);

        self.post_logout_redirect_uris.iter().any(|allowed| {
            // Also strip query params from registered URI in case they're registered with params
            let allowed_without_query = allowed.split('?').next().unwrap_or(allowed);
            allowed_without_query == uri_without_query
        })
    }

    /// Checks if the given scope is allowed for this client.
    ///
    /// An empty scopes list means all scopes are allowed.
    /// For SMART on FHIR scopes, uses semantic matching via `SmartScope::covers()`
    /// so that wildcard registrations (e.g. `patient/*.cruds`) cover specific
    /// resource requests (e.g. `patient/Patient.rs`).
    #[must_use]
    pub fn is_scope_allowed(&self, scope: &str) -> bool {
        if self.scopes.is_empty() {
            return true;
        }

        let requested_smart = SmartScope::parse(scope).ok();

        self.scopes.iter().any(|allowed| {
            // Exact string match (handles openid, fhirUser, launch/*, offline_access, etc.)
            if allowed == scope {
                return true;
            }
            // SMART semantic matching: wildcard + permission coverage
            if let Some(ref req) = requested_smart {
                if let Ok(allowed_smart) = SmartScope::parse(allowed) {
                    return allowed_smart.covers(req);
                }
            }
            false
        })
    }

    /// Checks if the given grant type is allowed for this client.
    #[must_use]
    pub fn is_grant_type_allowed(&self, grant_type: GrantType) -> bool {
        self.grant_types.contains(&grant_type)
    }

    /// Returns whether PKCE is required for this client.
    ///
    /// PKCE is always required for public clients. For confidential clients,
    /// it depends on the `pkce_required` setting (defaults to false).
    #[must_use]
    pub fn requires_pkce(&self) -> bool {
        if !self.confidential {
            // Public clients always require PKCE
            true
        } else {
            // Confidential clients: use setting, default to false
            self.pkce_required.unwrap_or(false)
        }
    }

    /// Returns the access token lifetime in seconds.
    ///
    /// Defaults to 3600 (1 hour) if not specified.
    #[must_use]
    pub fn access_token_lifetime_secs(&self) -> i64 {
        self.access_token_lifetime.unwrap_or(3600)
    }

    /// Returns the refresh token lifetime in seconds.
    ///
    /// Defaults to 2592000 (30 days) if not specified.
    #[must_use]
    pub fn refresh_token_lifetime_secs(&self) -> i64 {
        self.refresh_token_lifetime.unwrap_or(2_592_000)
    }

    /// Checks if the given origin is allowed for CORS.
    #[must_use]
    pub fn is_origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|allowed| allowed == origin)
    }
}

// =============================================================================
// Validation Error
// =============================================================================

/// Errors that can occur during client validation.
#[derive(Debug, thiserror::Error)]
pub enum ClientValidationError {
    /// Client ID cannot be empty.
    #[error("Client ID cannot be empty")]
    EmptyClientId,

    /// Client name cannot be empty.
    #[error("Client name cannot be empty")]
    EmptyName,

    /// At least one grant type is required.
    #[error("At least one grant type is required")]
    NoGrantTypes,

    /// Public clients cannot use client_credentials grant.
    #[error("Public clients cannot use client_credentials grant")]
    PublicClientCredentials,

    /// Authorization code flow requires redirect URIs.
    #[error("Authorization code flow requires redirect URIs")]
    NoRedirectUris,

    /// Confidential clients require a client secret.
    #[error("Confidential clients require a client secret")]
    MissingSecret,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_valid_public_client() -> Client {
        Client {
            client_id: "test-client".to_string(),
            client_secret: None,
            name: "Test Client".to_string(),
            description: None,
            grant_types: vec![GrantType::AuthorizationCode],
            redirect_uris: vec!["https://example.com/callback".to_string()],
            post_logout_redirect_uris: vec!["https://example.com/logout".to_string()],
            scopes: vec![],
            confidential: false,
            active: true,
            access_token_lifetime: None,
            refresh_token_lifetime: None,
            pkce_required: None,
            allowed_origins: vec![],
            jwks: None,
            jwks_uri: None,
        }
    }

    fn make_valid_confidential_client() -> Client {
        Client {
            client_id: "test-confidential".to_string(),
            client_secret: Some("$2b$12$hash".to_string()),
            name: "Confidential Client".to_string(),
            description: Some("A test confidential client".to_string()),
            grant_types: vec![GrantType::ClientCredentials, GrantType::RefreshToken],
            redirect_uris: vec![],
            post_logout_redirect_uris: vec![],
            scopes: vec!["read".to_string(), "write".to_string()],
            confidential: true,
            active: true,
            access_token_lifetime: Some(1800),
            refresh_token_lifetime: Some(86400),
            pkce_required: Some(false),
            allowed_origins: vec!["https://admin.example.com".to_string()],
            jwks: None,
            jwks_uri: None,
        }
    }

    #[test]
    fn test_valid_public_client() {
        let client = make_valid_public_client();
        assert!(client.validate().is_ok());
    }

    #[test]
    fn test_valid_confidential_client() {
        let client = make_valid_confidential_client();
        assert!(client.validate().is_ok());
    }

    #[test]
    fn test_empty_client_id() {
        let mut client = make_valid_public_client();
        client.client_id = String::new();
        assert!(matches!(
            client.validate(),
            Err(ClientValidationError::EmptyClientId)
        ));
    }

    #[test]
    fn test_empty_name() {
        let mut client = make_valid_public_client();
        client.name = String::new();
        assert!(matches!(
            client.validate(),
            Err(ClientValidationError::EmptyName)
        ));
    }

    #[test]
    fn test_no_grant_types() {
        let mut client = make_valid_public_client();
        client.grant_types = vec![];
        assert!(matches!(
            client.validate(),
            Err(ClientValidationError::NoGrantTypes)
        ));
    }

    #[test]
    fn test_public_client_with_client_credentials() {
        let mut client = make_valid_public_client();
        client.grant_types.push(GrantType::ClientCredentials);
        assert!(matches!(
            client.validate(),
            Err(ClientValidationError::PublicClientCredentials)
        ));
    }

    #[test]
    fn test_confidential_without_secret() {
        let mut client = make_valid_confidential_client();
        client.client_secret = None;
        assert!(matches!(
            client.validate(),
            Err(ClientValidationError::MissingSecret)
        ));
    }

    #[test]
    fn test_auth_code_without_redirect_uris() {
        let mut client = make_valid_public_client();
        client.redirect_uris = vec![];
        assert!(matches!(
            client.validate(),
            Err(ClientValidationError::NoRedirectUris)
        ));
    }

    #[test]
    fn test_redirect_uri_allowed() {
        let client = make_valid_public_client();
        assert!(client.is_redirect_uri_allowed("https://example.com/callback"));
        assert!(!client.is_redirect_uri_allowed("https://evil.com/callback"));
    }

    #[test]
    fn test_scope_allowed_empty_list() {
        let client = make_valid_public_client();
        // Empty scopes list means all scopes allowed
        assert!(client.is_scope_allowed("anything"));
        assert!(client.is_scope_allowed("read"));
    }

    #[test]
    fn test_scope_allowed_restricted() {
        let client = make_valid_confidential_client();
        assert!(client.is_scope_allowed("read"));
        assert!(client.is_scope_allowed("write"));
        assert!(!client.is_scope_allowed("admin"));
    }

    #[test]
    fn test_grant_type_allowed() {
        let client = make_valid_confidential_client();
        assert!(client.is_grant_type_allowed(GrantType::ClientCredentials));
        assert!(client.is_grant_type_allowed(GrantType::RefreshToken));
        assert!(!client.is_grant_type_allowed(GrantType::AuthorizationCode));
    }

    #[test]
    fn test_requires_pkce_public_client() {
        let client = make_valid_public_client();
        // Public clients always require PKCE
        assert!(client.requires_pkce());
    }

    #[test]
    fn test_requires_pkce_confidential_client() {
        let mut client = make_valid_confidential_client();
        // Default for confidential is false
        client.pkce_required = None;
        assert!(!client.requires_pkce());

        client.pkce_required = Some(true);
        assert!(client.requires_pkce());

        client.pkce_required = Some(false);
        assert!(!client.requires_pkce());
    }

    #[test]
    fn test_token_lifetimes() {
        let mut client = make_valid_public_client();

        // Default values
        assert_eq!(client.access_token_lifetime_secs(), 3600);
        assert_eq!(client.refresh_token_lifetime_secs(), 2_592_000);

        // Custom values
        client.access_token_lifetime = Some(1800);
        client.refresh_token_lifetime = Some(86400);
        assert_eq!(client.access_token_lifetime_secs(), 1800);
        assert_eq!(client.refresh_token_lifetime_secs(), 86400);
    }

    #[test]
    fn test_origin_allowed() {
        let client = make_valid_confidential_client();
        assert!(client.is_origin_allowed("https://admin.example.com"));
        assert!(!client.is_origin_allowed("https://evil.com"));
    }

    #[test]
    fn test_grant_type_as_str() {
        assert_eq!(GrantType::AuthorizationCode.as_str(), "authorization_code");
        assert_eq!(GrantType::ClientCredentials.as_str(), "client_credentials");
        assert_eq!(GrantType::RefreshToken.as_str(), "refresh_token");
    }

    #[test]
    fn test_serde_roundtrip() {
        let client = make_valid_confidential_client();
        let json = serde_json::to_string(&client).unwrap();
        let parsed: Client = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.client_id, client.client_id);
        assert_eq!(parsed.name, client.name);
        assert_eq!(parsed.confidential, client.confidential);
        assert_eq!(parsed.grant_types, client.grant_types);
    }

    #[test]
    fn test_post_logout_redirect_uri_ignores_query_params() {
        let client = make_valid_public_client();
        // Registered URI is "https://example.com/logout"

        // Exact match works
        assert!(client.is_post_logout_redirect_uri_allowed("https://example.com/logout"));

        // URI with query params should also match
        assert!(
            client.is_post_logout_redirect_uri_allowed("https://example.com/logout?reason=expired")
        );
        assert!(client.is_post_logout_redirect_uri_allowed(
            "https://example.com/logout?state=abc&reason=session_timeout"
        ));

        // Different path should not match
        assert!(!client.is_post_logout_redirect_uri_allowed("https://example.com/other"));
        assert!(
            !client.is_post_logout_redirect_uri_allowed("https://example.com/other?reason=expired")
        );

        // Different host should not match
        assert!(!client.is_post_logout_redirect_uri_allowed("https://evil.com/logout"));
    }

    #[test]
    fn test_scope_allowed_smart_wildcard() {
        let mut client = make_valid_public_client();
        client.scopes = vec!["patient/*.cruds".to_string()];

        // Specific resource scopes covered by wildcard
        assert!(client.is_scope_allowed("patient/Patient.rs"));
        assert!(client.is_scope_allowed("patient/Observation.r"));
        assert!(client.is_scope_allowed("patient/Condition.cruds"));

        // Different context not covered
        assert!(!client.is_scope_allowed("user/Patient.rs"));
        assert!(!client.is_scope_allowed("system/Patient.rs"));
    }

    #[test]
    fn test_scope_allowed_smart_permission_subset() {
        let mut client = make_valid_public_client();
        client.scopes = vec!["patient/*.r".to_string()];

        // Read-only covered
        assert!(client.is_scope_allowed("patient/Patient.r"));

        // Write permissions NOT covered
        assert!(!client.is_scope_allowed("patient/Patient.cruds"));
        assert!(!client.is_scope_allowed("patient/Patient.cu"));
    }

    #[test]
    fn test_scope_allowed_inferno_scenario() {
        let mut client = make_valid_public_client();
        client.scopes = vec![
            "openid".to_string(),
            "fhirUser".to_string(),
            "launch".to_string(),
            "launch/patient".to_string(),
            "offline_access".to_string(),
            "online_access".to_string(),
            "patient/*.r".to_string(),
            "patient/*.cruds".to_string(),
            "user/*.r".to_string(),
            "user/*.cruds".to_string(),
            "system/*.cruds".to_string(),
        ];

        // Non-SMART scopes: exact match
        assert!(client.is_scope_allowed("openid"));
        assert!(client.is_scope_allowed("fhirUser"));
        assert!(client.is_scope_allowed("launch/patient"));
        assert!(client.is_scope_allowed("offline_access"));

        // SMART scopes: semantic matching
        assert!(client.is_scope_allowed("patient/Patient.rs"));
        assert!(client.is_scope_allowed("patient/Observation.rs"));
        assert!(client.is_scope_allowed("user/Patient.r"));
        assert!(client.is_scope_allowed("system/Encounter.cruds"));

        // Not allowed
        assert!(!client.is_scope_allowed("unknown_scope"));
    }
}
