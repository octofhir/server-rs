//! Authentication context types.
//!
//! This module provides types for representing authenticated request context
//! extracted from Bearer tokens.

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::token::jwt::AccessTokenClaims;
use crate::types::Client;

// =============================================================================
// User Context
// =============================================================================

/// User context extracted from an authenticated request.
///
/// Contains user information loaded from storage based on the token's
/// subject claim.
#[derive(Debug, Clone)]
pub struct UserContext {
    /// User's unique identifier.
    pub id: Uuid,

    /// Username for display/logging.
    pub username: String,

    /// Reference to user's FHIR resource (e.g., "Practitioner/123").
    pub fhir_user: Option<String>,

    /// User's assigned roles.
    pub roles: Vec<String>,

    /// Additional user attributes for policy evaluation.
    pub attributes: HashMap<String, serde_json::Value>,
}

impl UserContext {
    /// Returns `true` if the user has a specific role.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Returns `true` if the user has any of the specified roles.
    #[must_use]
    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|role| self.has_role(role))
    }

    /// Gets an attribute value by key.
    #[must_use]
    pub fn get_attribute(&self, key: &str) -> Option<&serde_json::Value> {
        self.attributes.get(key)
    }
}

// =============================================================================
// Auth Context
// =============================================================================

/// Authenticated request context.
///
/// This struct is extracted from requests by the `BearerAuth` extractor
/// and contains all authentication/authorization information needed to
/// process the request.
///
/// The `token_claims` field is wrapped in `Arc` to allow cheap cloning
/// when caching or passing across async boundaries.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Validated access token claims (wrapped in Arc for cheap cloning).
    pub token_claims: Arc<AccessTokenClaims>,

    /// OAuth client that the token was issued to.
    pub client: Client,

    /// User context (if this is a user-delegated token).
    ///
    /// `None` for client credentials tokens.
    pub user: Option<UserContext>,

    /// Patient context from the token (SMART on FHIR).
    pub patient: Option<String>,

    /// Encounter context from the token (SMART on FHIR).
    pub encounter: Option<String>,
}

impl AuthContext {
    /// Checks if the token has a specific scope.
    ///
    /// This performs exact matching on space-separated scopes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if auth.has_scope("patient/Observation.read") {
    ///     // Allow access
    /// }
    /// ```
    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.token_claims
            .scope
            .split_whitespace()
            .any(|s| s == scope)
    }

    /// Checks if any scope starts with the given prefix.
    ///
    /// Useful for checking resource type access without specifying
    /// exact permissions.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if auth.scope_contains("patient/Observation") {
    ///     // Has some access to Observations
    /// }
    /// ```
    #[must_use]
    pub fn scope_contains(&self, prefix: &str) -> bool {
        self.token_claims
            .scope
            .split_whitespace()
            .any(|s| s.starts_with(prefix))
    }

    /// Returns all scopes as an iterator.
    pub fn scopes(&self) -> impl Iterator<Item = &str> {
        self.token_claims.scope.split_whitespace()
    }

    /// Returns `true` if a user is authenticated (not a client credentials token).
    #[must_use]
    pub fn is_user_authenticated(&self) -> bool {
        self.user.is_some()
    }

    /// Gets the FHIR user reference if available.
    ///
    /// First checks the user context, then falls back to the token claim.
    #[must_use]
    pub fn fhir_user(&self) -> Option<&str> {
        self.user
            .as_ref()
            .and_then(|u| u.fhir_user.as_deref())
            .or(self.token_claims.fhir_user.as_deref())
    }

    /// Gets the subject identifier from the token.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.token_claims.sub
    }

    /// Gets the client ID from the token.
    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.token_claims.client_id
    }

    /// Gets the JWT ID (unique token identifier).
    #[must_use]
    pub fn jti(&self) -> &str {
        &self.token_claims.jti
    }

    /// Gets the token issuer.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.token_claims.iss
    }

    /// Gets the token audiences.
    #[must_use]
    pub fn audiences(&self) -> &[String] {
        &self.token_claims.aud
    }

    /// Returns `true` if the token has patient context.
    #[must_use]
    pub fn has_patient_context(&self) -> bool {
        self.patient.is_some()
    }

    /// Returns `true` if the token has encounter context.
    #[must_use]
    pub fn has_encounter_context(&self) -> bool {
        self.encounter.is_some()
    }

    /// Checks if the given patient ID matches the token's patient context.
    ///
    /// Returns `true` if:
    /// - The token has no patient context (system-level access), or
    /// - The patient ID matches the token's patient context
    #[must_use]
    pub fn can_access_patient(&self, patient_id: &str) -> bool {
        match &self.patient {
            Some(ctx_patient) => ctx_patient == patient_id,
            None => true, // No patient context = system-level access
        }
    }

    /// Checks if the given encounter ID matches the token's encounter context.
    ///
    /// Returns `true` if:
    /// - The token has no encounter context, or
    /// - The encounter ID matches the token's encounter context
    #[must_use]
    pub fn can_access_encounter(&self, encounter_id: &str) -> bool {
        match &self.encounter {
            Some(ctx_encounter) => ctx_encounter == encounter_id,
            None => true,
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GrantType;

    fn create_test_claims() -> Arc<AccessTokenClaims> {
        Arc::new(AccessTokenClaims {
            iss: "https://auth.example.com".to_string(),
            sub: "user123".to_string(),
            aud: vec!["https://fhir.example.com".to_string()],
            exp: 9999999999,
            iat: 1000000000,
            jti: "test-jti-123".to_string(),
            scope: "openid patient/Patient.read patient/Observation.rs".to_string(),
            client_id: "test-client".to_string(),
            patient: Some("Patient/123".to_string()),
            encounter: None,
            fhir_user: Some("Practitioner/456".to_string()),
            sid: None,
        })
    }

    fn create_test_client() -> Client {
        Client {
            client_id: "test-client".to_string(),
            client_secret: None,
            name: "Test Client".to_string(),
            description: None,
            grant_types: vec![GrantType::AuthorizationCode],
            redirect_uris: vec!["https://app.example.com/callback".to_string()],
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

    fn create_test_user_context() -> UserContext {
        UserContext {
            id: Uuid::new_v4(),
            username: "testuser".to_string(),
            fhir_user: Some("Practitioner/789".to_string()),
            roles: vec!["practitioner".to_string(), "admin".to_string()],
            attributes: HashMap::new(),
        }
    }

    #[test]
    fn test_auth_context_has_scope() {
        let auth = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: None,
            patient: Some("Patient/123".to_string()),
            encounter: None,
        };

        assert!(auth.has_scope("openid"));
        assert!(auth.has_scope("patient/Patient.read"));
        assert!(auth.has_scope("patient/Observation.rs"));
        assert!(!auth.has_scope("patient/Patient.write"));
        assert!(!auth.has_scope("system/Patient.read"));
    }

    #[test]
    fn test_auth_context_scope_contains() {
        let auth = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: None,
            patient: Some("Patient/123".to_string()),
            encounter: None,
        };

        assert!(auth.scope_contains("patient/Patient"));
        assert!(auth.scope_contains("patient/Observation"));
        assert!(!auth.scope_contains("system/"));
        assert!(!auth.scope_contains("user/"));
    }

    #[test]
    fn test_auth_context_is_user_authenticated() {
        let mut auth = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: None,
            patient: None,
            encounter: None,
        };

        assert!(!auth.is_user_authenticated());

        auth.user = Some(create_test_user_context());
        assert!(auth.is_user_authenticated());
    }

    #[test]
    fn test_auth_context_fhir_user() {
        // Without user context, uses token claim
        let auth = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: None,
            patient: None,
            encounter: None,
        };
        assert_eq!(auth.fhir_user(), Some("Practitioner/456"));

        // With user context, prefers user context
        let auth_with_user = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: Some(create_test_user_context()),
            patient: None,
            encounter: None,
        };
        assert_eq!(auth_with_user.fhir_user(), Some("Practitioner/789"));
    }

    #[test]
    fn test_auth_context_can_access_patient() {
        let auth = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: None,
            patient: Some("Patient/123".to_string()),
            encounter: None,
        };

        assert!(auth.can_access_patient("Patient/123"));
        assert!(!auth.can_access_patient("Patient/456"));

        // Without patient context, can access any patient
        let auth_no_patient = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: None,
            patient: None,
            encounter: None,
        };
        assert!(auth_no_patient.can_access_patient("Patient/123"));
        assert!(auth_no_patient.can_access_patient("Patient/456"));
    }

    #[test]
    fn test_auth_context_accessors() {
        let auth = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: None,
            patient: Some("Patient/123".to_string()),
            encounter: None,
        };

        assert_eq!(auth.subject(), "user123");
        assert_eq!(auth.client_id(), "test-client");
        assert_eq!(auth.jti(), "test-jti-123");
        assert_eq!(auth.issuer(), "https://auth.example.com");
        assert_eq!(auth.audiences(), &["https://fhir.example.com".to_string()]);
        assert!(auth.has_patient_context());
        assert!(!auth.has_encounter_context());
    }

    #[test]
    fn test_user_context_has_role() {
        let user = create_test_user_context();

        assert!(user.has_role("practitioner"));
        assert!(user.has_role("admin"));
        assert!(!user.has_role("patient"));

        assert!(user.has_any_role(&["admin", "superuser"]));
        assert!(!user.has_any_role(&["patient", "guest"]));
    }

    #[test]
    fn test_auth_context_scopes_iterator() {
        let auth = AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: None,
            patient: None,
            encounter: None,
        };

        let scopes: Vec<&str> = auth.scopes().collect();
        assert_eq!(
            scopes,
            vec!["openid", "patient/Patient.read", "patient/Observation.rs"]
        );
    }
}
