//! Client authentication for the token endpoint.
//!
//! This module handles OAuth 2.0 client authentication at the token endpoint.
//! Multiple authentication methods are supported per RFC 6749 and OpenID Connect.
//!
//! # Authentication Methods
//!
//! - `none` - Public clients (no authentication)
//! - `client_secret_basic` - HTTP Basic Auth with client_id:client_secret
//! - `client_secret_post` - client_id and client_secret in request body
//! - `private_key_jwt` - Client assertion JWT (future)
//!
//! # Authentication Priority
//!
//! When multiple authentication methods are present, they are tried in order:
//! 1. HTTP Basic Auth header
//! 2. client_secret_post (body parameters)
//! 3. private_key_jwt (client assertion)
//! 4. Public client (client_id only)

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::AuthResult;
use crate::error::AuthError;
use crate::federation::ClientJwksCache;
use crate::oauth::client_assertion::{
    ClientAssertionValidator, extract_algorithm, extract_client_id_unverified, extract_key_id,
};
use crate::oauth::token::TokenRequest;
use crate::storage::{ClientStorage, JtiStorage};
use crate::types::Client;

/// Result of successful client authentication.
///
/// Contains the authenticated client and the method used for authentication.
#[derive(Debug, Clone)]
pub struct AuthenticatedClient {
    /// The authenticated client.
    pub client: Client,

    /// The authentication method used.
    pub auth_method: TokenEndpointAuthMethod,
}

/// Token endpoint authentication methods.
///
/// Defined in OpenID Connect Core Section 9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenEndpointAuthMethod {
    /// No client authentication (public clients).
    None,

    /// Client secret via HTTP Basic Auth.
    ClientSecretBasic,

    /// Client secret in request body.
    ClientSecretPost,

    /// Client assertion JWT signed with private key.
    PrivateKeyJwt,
}

impl TokenEndpointAuthMethod {
    /// Returns the string representation of the auth method.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ClientSecretBasic => "client_secret_basic",
            Self::ClientSecretPost => "client_secret_post",
            Self::PrivateKeyJwt => "private_key_jwt",
        }
    }
}

impl fmt::Display for TokenEndpointAuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Authenticates a client from a token request.
///
/// This function attempts to authenticate the client using the credentials
/// provided in the request or HTTP headers. It tries authentication methods
/// in priority order.
///
/// # Arguments
///
/// * `request` - The token request containing client credentials
/// * `basic_auth` - Optional HTTP Basic Auth credentials (client_id, client_secret)
/// * `client_storage` - Storage for looking up client registrations
///
/// # Returns
///
/// Returns the authenticated client with the method used.
///
/// # Errors
///
/// Returns an error if:
/// - No client credentials are provided
/// - The client is not found
/// - The client secret is invalid
/// - A confidential client attempts to use public authentication
/// - A public client provides unnecessary credentials
///
/// # Example
///
/// ```ignore
/// let auth_result = authenticate_client(&request, basic_auth, &client_storage).await?;
/// println!("Authenticated {} via {:?}", auth_result.client.client_id, auth_result.auth_method);
/// ```
pub async fn authenticate_client(
    request: &TokenRequest,
    basic_auth: Option<(&str, &str)>,
    client_storage: &dyn ClientStorage,
) -> AuthResult<AuthenticatedClient> {
    // 1. Try HTTP Basic Auth first (highest priority for confidential clients)
    if let Some((client_id, client_secret)) = basic_auth {
        return authenticate_basic(client_id, client_secret, client_storage).await;
    }

    // 2. Try client_secret_post
    if let (Some(client_id), Some(client_secret)) = (&request.client_id, &request.client_secret) {
        return authenticate_secret_post(client_id, client_secret, client_storage).await;
    }

    // 3. Try private_key_jwt (client assertion)
    // Note: For private_key_jwt authentication, use the authenticate_private_key_jwt
    // function directly, which requires additional dependencies (assertion validator,
    // JWKS cache) that are not available in this basic authentication flow.
    if let (Some(assertion_type), Some(_assertion)) =
        (&request.client_assertion_type, &request.client_assertion)
    {
        if assertion_type == "urn:ietf:params:oauth:client-assertion-type:jwt-bearer" {
            // private_key_jwt requires additional context (validator, JWKS cache)
            // Callers should use authenticate_private_key_jwt() directly for this flow
            return Err(AuthError::invalid_client(
                "private_key_jwt authentication requires using authenticate_private_key_jwt()",
            ));
        }
        return Err(AuthError::invalid_request(format!(
            "Unsupported client_assertion_type: {}",
            assertion_type
        )));
    }

    // 4. Try public client authentication (client_id only)
    if let Some(client_id) = &request.client_id {
        return authenticate_public(client_id, client_storage).await;
    }

    Err(AuthError::invalid_client("No client credentials provided"))
}

/// Authenticates a client using HTTP Basic Auth.
///
/// # Arguments
///
/// * `client_id` - The client ID from Basic Auth
/// * `client_secret` - The client secret from Basic Auth
/// * `client_storage` - Storage for looking up clients
///
/// # Errors
///
/// Returns an error if:
/// - The client is not found
/// - The client is not active
/// - The client is a public client (cannot use Basic Auth)
/// - The secret is incorrect
async fn authenticate_basic(
    client_id: &str,
    client_secret: &str,
    client_storage: &dyn ClientStorage,
) -> AuthResult<AuthenticatedClient> {
    let client = client_storage
        .find_by_client_id(client_id)
        .await?
        .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

    if !client.active {
        return Err(AuthError::invalid_client("Client is inactive"));
    }

    // Public clients cannot use Basic Auth
    if !client.confidential {
        return Err(AuthError::invalid_client(
            "Public clients cannot use client_secret_basic authentication",
        ));
    }

    // Verify secret
    if !client_storage
        .verify_secret(client_id, client_secret)
        .await?
    {
        return Err(AuthError::invalid_client("Invalid client secret"));
    }

    Ok(AuthenticatedClient {
        client,
        auth_method: TokenEndpointAuthMethod::ClientSecretBasic,
    })
}

/// Authenticates a client using client_secret_post (body parameters).
///
/// # Arguments
///
/// * `client_id` - The client ID from request body
/// * `client_secret` - The client secret from request body
/// * `client_storage` - Storage for looking up clients
///
/// # Errors
///
/// Returns an error if:
/// - The client is not found
/// - The client is not active
/// - The client is a public client
/// - The secret is incorrect
async fn authenticate_secret_post(
    client_id: &str,
    client_secret: &str,
    client_storage: &dyn ClientStorage,
) -> AuthResult<AuthenticatedClient> {
    let client = client_storage
        .find_by_client_id(client_id)
        .await?
        .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

    if !client.active {
        return Err(AuthError::invalid_client("Client is inactive"));
    }

    // Public clients should not provide secrets
    if !client.confidential {
        return Err(AuthError::invalid_client(
            "Public clients cannot use client_secret_post authentication",
        ));
    }

    // Verify secret
    if !client_storage
        .verify_secret(client_id, client_secret)
        .await?
    {
        return Err(AuthError::invalid_client("Invalid client secret"));
    }

    Ok(AuthenticatedClient {
        client,
        auth_method: TokenEndpointAuthMethod::ClientSecretPost,
    })
}

/// Authenticates a public client (no secret required).
///
/// # Arguments
///
/// * `client_id` - The client ID
/// * `client_storage` - Storage for looking up clients
///
/// # Errors
///
/// Returns an error if:
/// - The client is not found
/// - The client is not active
/// - The client is confidential (must provide credentials)
async fn authenticate_public(
    client_id: &str,
    client_storage: &dyn ClientStorage,
) -> AuthResult<AuthenticatedClient> {
    let client = client_storage
        .find_by_client_id(client_id)
        .await?
        .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

    if !client.active {
        return Err(AuthError::invalid_client("Client is inactive"));
    }

    // Confidential clients must provide credentials
    if client.confidential {
        return Err(AuthError::invalid_client(
            "Confidential clients must provide client credentials",
        ));
    }

    Ok(AuthenticatedClient {
        client,
        auth_method: TokenEndpointAuthMethod::None,
    })
}

/// Authenticates a client using private_key_jwt (JWT client assertion).
///
/// This is used by SMART Backend Services to authenticate using a JWT
/// signed with the client's private key.
///
/// # Arguments
///
/// * `assertion` - The JWT client assertion
/// * `client_storage` - Storage for looking up clients
/// * `assertion_validator` - Validator for JWT assertions
/// * `jwks_cache` - Cache for fetching client JWKS
///
/// # Errors
///
/// Returns an error if:
/// - The JWT cannot be parsed
/// - The client is not found
/// - The client doesn't have JWKS configured
/// - The JWT signature is invalid
/// - The JWT claims are invalid (iss, sub, aud, exp, jti)
/// - The JTI has already been used (replay attack)
pub async fn authenticate_private_key_jwt<S: JtiStorage>(
    assertion: &str,
    client_storage: &dyn ClientStorage,
    assertion_validator: &ClientAssertionValidator<S>,
    jwks_cache: &ClientJwksCache,
) -> AuthResult<AuthenticatedClient> {
    // 1. Extract client_id from unverified JWT
    let client_id = extract_client_id_unverified(assertion)?;

    // 2. Look up client
    let client = client_storage
        .find_by_client_id(&client_id)
        .await?
        .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

    if !client.active {
        return Err(AuthError::invalid_client("Client is inactive"));
    }

    // 3. Extract algorithm and key ID from JWT header
    let algorithm = extract_algorithm(assertion)?;
    let kid = extract_key_id(assertion)?;

    // 4. Get decoding key from client's JWKS
    let decoding_key = if let Some(ref jwks) = client.jwks {
        // Use inline JWKS
        jwks_cache.get_decoding_key_from_inline(jwks, kid.as_deref(), algorithm)?
    } else if let Some(ref jwks_uri) = client.jwks_uri {
        // Fetch from JWKS URI
        jwks_cache
            .get_decoding_key(jwks_uri, kid.as_deref(), algorithm)
            .await?
    } else {
        return Err(AuthError::invalid_client(
            "Client has no JWKS or JWKS URI configured for private_key_jwt authentication",
        ));
    };

    // 5. Validate the assertion
    assertion_validator
        .validate(assertion, &client_id, &decoding_key, algorithm)
        .await?;

    Ok(AuthenticatedClient {
        client,
        auth_method: TokenEndpointAuthMethod::PrivateKeyJwt,
    })
}

/// Parses HTTP Basic Auth header value.
///
/// # Arguments
///
/// * `header_value` - The Authorization header value (e.g., "Basic dGVzdDoxMjM=")
///
/// # Returns
///
/// Returns `Some((client_id, client_secret))` if valid, `None` otherwise.
///
/// # Example
///
/// ```ignore
/// let auth_header = "Basic Y2xpZW50X2lkOmNsaWVudF9zZWNyZXQ=";
/// if let Some((id, secret)) = parse_basic_auth(auth_header) {
///     println!("client_id: {}, secret: {}", id, secret);
/// }
/// ```
#[must_use]
pub fn parse_basic_auth(header_value: &str) -> Option<(String, String)> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let header_value = header_value.trim();

    // Must start with "Basic "
    if !header_value.starts_with("Basic ") {
        return None;
    }

    let encoded = &header_value[6..];
    let decoded = STANDARD.decode(encoded).ok()?;
    let credentials = String::from_utf8(decoded).ok()?;

    // Split on first colon (password may contain colons)
    let (client_id, client_secret) = credentials.split_once(':')?;

    Some((client_id.to_string(), client_secret.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GrantType;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Mock client storage for testing.
    struct MockClientStorage {
        clients: RwLock<HashMap<String, (Client, String)>>, // client_id -> (client, secret)
    }

    impl MockClientStorage {
        fn new() -> Self {
            Self {
                clients: RwLock::new(HashMap::new()),
            }
        }

        fn add_client(&self, client: Client, secret: Option<&str>) {
            self.clients.write().unwrap().insert(
                client.client_id.clone(),
                (client, secret.unwrap_or("").to_string()),
            );
        }
    }

    #[async_trait::async_trait]
    impl ClientStorage for MockClientStorage {
        async fn find_by_client_id(&self, client_id: &str) -> AuthResult<Option<Client>> {
            Ok(self
                .clients
                .read()
                .unwrap()
                .get(client_id)
                .map(|(c, _)| c.clone()))
        }

        async fn create(&self, client: &Client) -> AuthResult<Client> {
            self.add_client(client.clone(), None);
            Ok(client.clone())
        }

        async fn update(&self, _client_id: &str, client: &Client) -> AuthResult<Client> {
            self.add_client(client.clone(), None);
            Ok(client.clone())
        }

        async fn delete(&self, client_id: &str) -> AuthResult<()> {
            self.clients.write().unwrap().remove(client_id);
            Ok(())
        }

        async fn list(&self, _limit: i64, _offset: i64) -> AuthResult<Vec<Client>> {
            Ok(self
                .clients
                .read()
                .unwrap()
                .values()
                .map(|(c, _)| c.clone())
                .collect())
        }

        async fn verify_secret(&self, client_id: &str, secret: &str) -> AuthResult<bool> {
            Ok(self
                .clients
                .read()
                .unwrap()
                .get(client_id)
                .map(|(_, s)| s == secret)
                .unwrap_or(false))
        }

        async fn regenerate_secret(&self, client_id: &str) -> AuthResult<(Client, String)> {
            let mut clients = self.clients.write().unwrap();
            if let Some((client, _)) = clients.get(client_id) {
                let new_secret = "new-test-secret".to_string();
                let mut updated = client.clone();
                updated.client_secret = Some(new_secret.clone());
                clients.insert(client_id.to_string(), (updated.clone(), new_secret.clone()));
                Ok((updated, new_secret))
            } else {
                Err(crate::error::AuthError::invalid_client(format!(
                    "Client not found: {}",
                    client_id
                )))
            }
        }
    }

    fn create_public_client() -> Client {
        Client {
            client_id: "public-client".to_string(),
            client_secret: None,
            name: "Public Client".to_string(),
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

    fn create_confidential_client() -> Client {
        Client {
            client_id: "confidential-client".to_string(),
            client_secret: Some("hashed-secret".to_string()),
            name: "Confidential Client".to_string(),
            description: None,
            grant_types: vec![GrantType::AuthorizationCode, GrantType::ClientCredentials],
            redirect_uris: vec!["https://app.example.com/callback".to_string()],
            scopes: vec![],
            confidential: true,
            active: true,
            access_token_lifetime: None,
            refresh_token_lifetime: None,
            pkce_required: None,
            allowed_origins: vec![],
            jwks: None,
            jwks_uri: None,
        }
    }

    #[tokio::test]
    async fn test_authenticate_public_client() {
        let storage = MockClientStorage::new();
        storage.add_client(create_public_client(), None);

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            client_id: Some("public-client".to_string()),
            code: Some("code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some("verifier".to_string()),
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let result = authenticate_client(&request, None, &storage).await;
        assert!(result.is_ok());

        let auth = result.unwrap();
        assert_eq!(auth.client.client_id, "public-client");
        assert_eq!(auth.auth_method, TokenEndpointAuthMethod::None);
    }

    #[tokio::test]
    async fn test_authenticate_basic_auth() {
        let storage = MockClientStorage::new();
        storage.add_client(create_confidential_client(), Some("secret123"));

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            client_id: None,
            code: Some("code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some("verifier".to_string()),
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let result = authenticate_client(
            &request,
            Some(("confidential-client", "secret123")),
            &storage,
        )
        .await;
        assert!(result.is_ok());

        let auth = result.unwrap();
        assert_eq!(auth.client.client_id, "confidential-client");
        assert_eq!(auth.auth_method, TokenEndpointAuthMethod::ClientSecretBasic);
    }

    #[tokio::test]
    async fn test_authenticate_secret_post() {
        let storage = MockClientStorage::new();
        storage.add_client(create_confidential_client(), Some("secret123"));

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            client_id: Some("confidential-client".to_string()),
            code: Some("code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some("verifier".to_string()),
            client_secret: Some("secret123".to_string()),
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let result = authenticate_client(&request, None, &storage).await;
        assert!(result.is_ok());

        let auth = result.unwrap();
        assert_eq!(auth.client.client_id, "confidential-client");
        assert_eq!(auth.auth_method, TokenEndpointAuthMethod::ClientSecretPost);
    }

    #[tokio::test]
    async fn test_authenticate_unknown_client() {
        let storage = MockClientStorage::new();

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            client_id: Some("unknown-client".to_string()),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let result = authenticate_client(&request, None, &storage).await;
        assert!(matches!(result, Err(AuthError::InvalidClient { .. })));
    }

    #[tokio::test]
    async fn test_authenticate_wrong_secret() {
        let storage = MockClientStorage::new();
        storage.add_client(create_confidential_client(), Some("correct-secret"));

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            client_id: Some("confidential-client".to_string()),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_secret: Some("wrong-secret".to_string()),
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let result = authenticate_client(&request, None, &storage).await;
        assert!(matches!(result, Err(AuthError::InvalidClient { .. })));
    }

    #[tokio::test]
    async fn test_confidential_client_requires_credentials() {
        let storage = MockClientStorage::new();
        storage.add_client(create_confidential_client(), Some("secret"));

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            client_id: Some("confidential-client".to_string()),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_secret: None, // No secret provided
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let result = authenticate_client(&request, None, &storage).await;
        assert!(matches!(result, Err(AuthError::InvalidClient { .. })));
    }

    #[tokio::test]
    async fn test_public_client_cannot_use_basic_auth() {
        let storage = MockClientStorage::new();
        storage.add_client(create_public_client(), None);

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            client_id: None,
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let result =
            authenticate_client(&request, Some(("public-client", "any-secret")), &storage).await;
        assert!(matches!(result, Err(AuthError::InvalidClient { .. })));
    }

    #[tokio::test]
    async fn test_no_credentials_provided() {
        let storage = MockClientStorage::new();

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            client_id: None,
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let result = authenticate_client(&request, None, &storage).await;
        assert!(matches!(result, Err(AuthError::InvalidClient { .. })));
    }

    #[test]
    fn test_parse_basic_auth_valid() {
        // "client_id:client_secret" base64 encoded
        let header = "Basic Y2xpZW50X2lkOmNsaWVudF9zZWNyZXQ=";
        let result = parse_basic_auth(header);

        assert!(result.is_some());
        let (id, secret) = result.unwrap();
        assert_eq!(id, "client_id");
        assert_eq!(secret, "client_secret");
    }

    #[test]
    fn test_parse_basic_auth_with_colon_in_password() {
        // "client:pass:word" base64 encoded
        let header = "Basic Y2xpZW50OnBhc3M6d29yZA==";
        let result = parse_basic_auth(header);

        assert!(result.is_some());
        let (id, secret) = result.unwrap();
        assert_eq!(id, "client");
        assert_eq!(secret, "pass:word");
    }

    #[test]
    fn test_parse_basic_auth_invalid_scheme() {
        let header = "Bearer some-token";
        assert!(parse_basic_auth(header).is_none());
    }

    #[test]
    fn test_parse_basic_auth_invalid_base64() {
        let header = "Basic not-valid-base64!!!";
        assert!(parse_basic_auth(header).is_none());
    }

    #[test]
    fn test_parse_basic_auth_no_colon() {
        // "clientonly" base64 encoded (no colon separator)
        let header = "Basic Y2xpZW50b25seQ==";
        assert!(parse_basic_auth(header).is_none());
    }

    #[test]
    fn test_auth_method_as_str() {
        assert_eq!(TokenEndpointAuthMethod::None.as_str(), "none");
        assert_eq!(
            TokenEndpointAuthMethod::ClientSecretBasic.as_str(),
            "client_secret_basic"
        );
        assert_eq!(
            TokenEndpointAuthMethod::ClientSecretPost.as_str(),
            "client_secret_post"
        );
        assert_eq!(
            TokenEndpointAuthMethod::PrivateKeyJwt.as_str(),
            "private_key_jwt"
        );
    }
}
