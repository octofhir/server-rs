//! OpenID Connect UserInfo endpoint.
//!
//! Provides the `/userinfo` endpoint for retrieving claims about the authenticated user
//! per OpenID Connect Core 1.0 and SMART on FHIR specifications.
//!
//! # Overview
//!
//! The UserInfo endpoint returns claims about the authenticated user. The claims
//! returned depend on the scopes granted in the access token:
//!
//! - `openid` (required): Enables the UserInfo endpoint
//! - `fhirUser`: Includes the FHIR resource reference for the user
//! - `profile`: Includes name, given_name, family_name, etc.
//! - `email`: Includes email and email_verified claims
//!
//! # References
//!
//! - [OpenID Connect UserInfo](https://openid.net/specs/openid-connect-core-1_0.html#UserInfo)
//! - [SMART Identity Scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html#scopes-for-requesting-identity-data)

use std::collections::HashMap;

use axum::{Json, http::header, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AuthError;
use crate::middleware::BearerAuth;
use crate::smart::SmartScopes;

// =============================================================================
// UserInfo Response
// =============================================================================

/// UserInfo response per OpenID Connect Core 1.0.
///
/// Contains claims about the authenticated user based on the scopes
/// granted in the access token.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserInfoResponse {
    /// Subject identifier (required). This is the user's unique identifier.
    pub sub: String,

    /// FHIR resource reference for the user (SMART on FHIR extension).
    /// Only included when `fhirUser` scope was granted.
    #[serde(rename = "fhirUser", skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,

    /// Full name of the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Given name(s) or first name(s) of the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,

    /// Surname(s) or last name(s) of the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,

    /// Middle name(s) of the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middle_name: Option<String>,

    /// Casual name of the user that may or may not be the same as the given_name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,

    /// Shorthand name by which the user wishes to be referred to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,

    /// URL of the user's profile page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,

    /// URL of the user's profile picture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,

    /// URL of the user's web page or blog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,

    /// User's preferred email address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// True if the user's email address has been verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,

    /// User's gender.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,

    /// User's birthday in ISO 8601 format (YYYY-MM-DD).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birthdate: Option<String>,

    /// User's time zone (e.g., "America/Los_Angeles").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoneinfo: Option<String>,

    /// User's locale (e.g., "en-US").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,

    /// User's preferred telephone number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,

    /// True if the user's phone number has been verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number_verified: Option<bool>,

    /// Time the user's information was last updated (Unix timestamp).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

// =============================================================================
// HTTP Handlers
// =============================================================================

/// Handler for `GET /userinfo`.
///
/// Returns claims about the authenticated user based on the scopes
/// granted in the access token.
///
/// # Errors
///
/// - Returns 401 Unauthorized if the token is missing or invalid
/// - Returns 403 Forbidden if the `openid` scope was not granted
/// - Returns 403 Forbidden if the token is not a user-delegated token
///
/// # Example Response
///
/// ```json
/// {
///   "sub": "user-123",
///   "fhirUser": "Practitioner/456",
///   "name": "Dr. Jane Smith",
///   "given_name": "Jane",
///   "family_name": "Smith",
///   "email": "jane.smith@example.com"
/// }
/// ```
pub async fn userinfo_handler(
    BearerAuth(auth): BearerAuth,
) -> Result<impl IntoResponse, AuthError> {
    // 1. Parse scopes and verify openid scope is present
    let scopes = SmartScopes::parse(&auth.token_claims.scope)
        .map_err(|_| AuthError::invalid_scope("Invalid scope format"))?;

    if !scopes.openid {
        return Err(AuthError::invalid_scope(
            "The openid scope is required for the userinfo endpoint",
        ));
    }

    // 2. Verify this is a user-delegated token
    let user = auth.user.ok_or_else(|| {
        AuthError::forbidden("The userinfo endpoint requires a user-delegated token")
    })?;

    // 3. Build response with available claims
    let mut response = UserInfoResponse {
        sub: auth.token_claims.sub.clone(),
        preferred_username: Some(user.username.clone()),
        ..Default::default()
    };

    // 4. Include fhirUser if scope was granted
    if scopes.fhir_user {
        response.fhir_user = user.fhir_user.clone();
    }

    // 5. Include profile claims from user attributes
    response.name = get_string_attr(&user.attributes, "name");
    response.given_name = get_string_attr(&user.attributes, "given_name");
    response.family_name = get_string_attr(&user.attributes, "family_name");
    response.middle_name = get_string_attr(&user.attributes, "middle_name");
    response.nickname = get_string_attr(&user.attributes, "nickname");
    response.profile = get_string_attr(&user.attributes, "profile");
    response.picture = get_string_attr(&user.attributes, "picture");
    response.website = get_string_attr(&user.attributes, "website");
    response.gender = get_string_attr(&user.attributes, "gender");
    response.birthdate = get_string_attr(&user.attributes, "birthdate");
    response.zoneinfo = get_string_attr(&user.attributes, "zoneinfo");
    response.locale = get_string_attr(&user.attributes, "locale");

    // 6. Include email claims from user attributes
    response.email = get_string_attr(&user.attributes, "email");
    response.email_verified = get_bool_attr(&user.attributes, "email_verified");

    // 7. Include phone claims from user attributes
    response.phone_number = get_string_attr(&user.attributes, "phone_number");
    response.phone_number_verified = get_bool_attr(&user.attributes, "phone_number_verified");

    // 8. Include updated_at if available
    response.updated_at = get_i64_attr(&user.attributes, "updated_at");

    Ok(([(header::CONTENT_TYPE, "application/json")], Json(response)))
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Extracts a string value from user attributes.
fn get_string_attr(attrs: &HashMap<String, Value>, key: &str) -> Option<String> {
    attrs.get(key).and_then(|v| v.as_str().map(String::from))
}

/// Extracts a boolean value from user attributes.
fn get_bool_attr(attrs: &HashMap<String, Value>, key: &str) -> Option<bool> {
    attrs.get(key).and_then(|v| v.as_bool())
}

/// Extracts an i64 value from user attributes.
fn get_i64_attr(attrs: &HashMap<String, Value>, key: &str) -> Option<i64> {
    attrs.get(key).and_then(|v| v.as_i64())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    use crate::middleware::types::{AuthContext, UserContext};
    use crate::token::jwt::AccessTokenClaims;
    use crate::types::{Client, GrantType};

    fn create_test_claims(scope: &str) -> AccessTokenClaims {
        AccessTokenClaims {
            iss: "https://auth.example.com".to_string(),
            sub: "user-123".to_string(),
            aud: vec!["https://fhir.example.com".to_string()],
            exp: 9999999999,
            iat: 1000000000,
            jti: "test-jti".to_string(),
            scope: scope.to_string(),
            client_id: "test-client".to_string(),
            patient: None,
            encounter: None,
            fhir_user: Some("Practitioner/456".to_string()),
        }
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
        let mut attributes = HashMap::new();
        attributes.insert(
            "name".to_string(),
            Value::String("Dr. Jane Smith".to_string()),
        );
        attributes.insert("given_name".to_string(), Value::String("Jane".to_string()));
        attributes.insert(
            "family_name".to_string(),
            Value::String("Smith".to_string()),
        );
        attributes.insert(
            "email".to_string(),
            Value::String("jane.smith@example.com".to_string()),
        );
        attributes.insert("email_verified".to_string(), Value::Bool(true));

        UserContext {
            id: Uuid::new_v4(),
            username: "jsmith".to_string(),
            fhir_user: Some("Practitioner/789".to_string()),
            roles: vec!["practitioner".to_string()],
            attributes,
        }
    }

    fn create_test_auth_context(scope: &str, with_user: bool) -> AuthContext {
        AuthContext {
            token_claims: create_test_claims(scope),
            client: create_test_client(),
            user: if with_user {
                Some(create_test_user_context())
            } else {
                None
            },
            patient: None,
            encounter: None,
        }
    }

    #[test]
    fn test_userinfo_response_serialization() {
        let response = UserInfoResponse {
            sub: "user-123".to_string(),
            fhir_user: Some("Practitioner/456".to_string()),
            name: Some("Dr. Jane Smith".to_string()),
            email: Some("jane@example.com".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["sub"], "user-123");
        assert_eq!(json["fhirUser"], "Practitioner/456");
        assert_eq!(json["name"], "Dr. Jane Smith");
        assert_eq!(json["email"], "jane@example.com");

        // Optional None fields should not be present
        assert!(json.get("given_name").is_none());
        assert!(json.get("family_name").is_none());
    }

    #[test]
    fn test_userinfo_response_deserialization() {
        let json = r#"{
            "sub": "user-123",
            "fhirUser": "Practitioner/456",
            "name": "Dr. Jane Smith"
        }"#;

        let response: UserInfoResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.sub, "user-123");
        assert_eq!(response.fhir_user, Some("Practitioner/456".to_string()));
        assert_eq!(response.name, Some("Dr. Jane Smith".to_string()));
        assert!(response.email.is_none());
    }

    #[test]
    fn test_get_string_attr() {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), Value::String("Jane".to_string()));
        attrs.insert("count".to_string(), Value::Number(42.into()));

        assert_eq!(get_string_attr(&attrs, "name"), Some("Jane".to_string()));
        assert_eq!(get_string_attr(&attrs, "missing"), None);
        assert_eq!(get_string_attr(&attrs, "count"), None); // Not a string
    }

    #[test]
    fn test_get_bool_attr() {
        let mut attrs = HashMap::new();
        attrs.insert("verified".to_string(), Value::Bool(true));
        attrs.insert("name".to_string(), Value::String("Jane".to_string()));

        assert_eq!(get_bool_attr(&attrs, "verified"), Some(true));
        assert_eq!(get_bool_attr(&attrs, "missing"), None);
        assert_eq!(get_bool_attr(&attrs, "name"), None); // Not a bool
    }

    #[test]
    fn test_get_i64_attr() {
        let mut attrs = HashMap::new();
        attrs.insert("updated_at".to_string(), Value::Number(1234567890.into()));
        attrs.insert("name".to_string(), Value::String("Jane".to_string()));

        assert_eq!(get_i64_attr(&attrs, "updated_at"), Some(1234567890));
        assert_eq!(get_i64_attr(&attrs, "missing"), None);
        assert_eq!(get_i64_attr(&attrs, "name"), None); // Not an i64
    }

    #[test]
    fn test_scope_parsing_with_openid() {
        let scopes = SmartScopes::parse("openid fhirUser patient/Patient.r").unwrap();

        assert!(scopes.openid);
        assert!(scopes.fhir_user);
    }

    #[test]
    fn test_scope_parsing_without_openid() {
        let scopes = SmartScopes::parse("patient/Patient.r").unwrap();

        assert!(!scopes.openid);
        assert!(!scopes.fhir_user);
    }

    #[test]
    fn test_auth_context_user_extraction() {
        let auth = create_test_auth_context("openid fhirUser", true);

        assert!(auth.user.is_some());
        let user = auth.user.unwrap();
        assert_eq!(user.username, "jsmith");
        assert_eq!(user.fhir_user, Some("Practitioner/789".to_string()));
        assert_eq!(
            user.attributes.get("name"),
            Some(&Value::String("Dr. Jane Smith".to_string()))
        );
    }

    #[test]
    fn test_auth_context_without_user() {
        let auth = create_test_auth_context("openid", false);

        assert!(auth.user.is_none());
    }
}
