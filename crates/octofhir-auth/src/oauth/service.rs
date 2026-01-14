//! OAuth authorization service.
//!
//! This module provides the authorization service that handles OAuth 2.0
//! authorization requests. It validates requests, creates authorization
//! sessions, and enforces security requirements.
//!
//! # Security Requirements
//!
//! The service enforces several security requirements:
//!
//! - PKCE is required for all clients (S256 method only)
//! - State parameter must have at least 122 bits of entropy
//! - Authorization codes are 256-bit random values
//! - Sessions expire after a configurable time (default 10 minutes)
//!
//! # Usage
//!
//! ```ignore
//! use octofhir_auth::oauth::{AuthorizationService, AuthorizationConfig, AuthorizationRequest};
//!
//! let service = AuthorizationService::new(
//!     client_storage,
//!     session_storage,
//!     AuthorizationConfig::default(),
//! );
//!
//! let session = service.authorize(&request).await?;
//! let redirect_url = AuthorizationResponse::new(session.code, request.state)
//!     .to_redirect_url(&request.redirect_uri)?;
//! ```

use std::sync::Arc;

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::AuthResult;
use crate::error::AuthError;
use crate::oauth::authorize::AuthorizationRequest;
use crate::oauth::pkce::{PkceChallenge, PkceChallengeMethod};
use crate::oauth::session::{AuthorizationSession, LaunchContext};
use crate::smart::launch::StoredLaunchContext;
use crate::smart::scopes::SmartScopes;
use crate::storage::ClientStorage;
use crate::storage::LaunchContextStorage;
use crate::storage::session::SessionStorage;
use crate::types::GrantType;

/// Authorization service for handling OAuth 2.0 authorization requests.
///
/// This service validates authorization requests, creates authorization
/// sessions, and coordinates with storage backends to persist session state.
pub struct AuthorizationService {
    /// Client storage for looking up registered clients.
    client_storage: Arc<dyn ClientStorage>,

    /// Session storage for persisting authorization sessions.
    session_storage: Arc<dyn SessionStorage>,

    /// Launch context storage for EHR launch flow.
    /// Optional - only needed for EHR launch support.
    launch_context_storage: Option<Arc<dyn LaunchContextStorage>>,

    /// Service configuration.
    config: AuthorizationConfig,
}

/// Configuration for the authorization service.
#[derive(Debug, Clone)]
pub struct AuthorizationConfig {
    /// Authorization code lifetime.
    /// Default: 10 minutes (as recommended by OAuth 2.0 spec).
    pub code_lifetime: Duration,

    /// Minimum state entropy in bits.
    /// Default: 122 bits (as recommended by SMART on FHIR).
    pub min_state_entropy_bits: usize,

    /// Whether to require the `aud` parameter.
    /// Default: true (required for SMART on FHIR).
    pub require_aud: bool,
}

impl Default for AuthorizationConfig {
    fn default() -> Self {
        Self {
            code_lifetime: Duration::minutes(10),
            min_state_entropy_bits: 122,
            require_aud: true,
        }
    }
}

impl AuthorizationConfig {
    /// Creates a new configuration with custom code lifetime.
    #[must_use]
    pub fn with_code_lifetime(mut self, lifetime: Duration) -> Self {
        self.code_lifetime = lifetime;
        self
    }

    /// Creates a new configuration with custom minimum state entropy.
    #[must_use]
    pub fn with_min_state_entropy(mut self, bits: usize) -> Self {
        self.min_state_entropy_bits = bits;
        self
    }

    /// Creates a new configuration that doesn't require `aud`.
    #[must_use]
    pub fn without_aud_requirement(mut self) -> Self {
        self.require_aud = false;
        self
    }
}

impl AuthorizationService {
    /// Creates a new authorization service.
    ///
    /// # Arguments
    ///
    /// * `client_storage` - Storage for looking up registered clients
    /// * `session_storage` - Storage for persisting authorization sessions
    /// * `config` - Service configuration
    #[must_use]
    pub fn new(
        client_storage: Arc<dyn ClientStorage>,
        session_storage: Arc<dyn SessionStorage>,
        config: AuthorizationConfig,
    ) -> Self {
        Self {
            client_storage,
            session_storage,
            launch_context_storage: None,
            config,
        }
    }

    /// Configures the service with launch context storage for EHR launch support.
    ///
    /// # Arguments
    ///
    /// * `launch_storage` - Storage for SMART launch contexts
    ///
    /// # Example
    ///
    /// ```ignore
    /// let service = AuthorizationService::new(client_storage, session_storage, config)
    ///     .with_launch_storage(launch_context_storage);
    /// ```
    #[must_use]
    pub fn with_launch_storage(mut self, launch_storage: Arc<dyn LaunchContextStorage>) -> Self {
        self.launch_context_storage = Some(launch_storage);
        self
    }

    /// Processes an authorization request.
    ///
    /// This method validates the request parameters, looks up the client,
    /// and creates an authorization session if validation succeeds.
    ///
    /// # Arguments
    ///
    /// * `request` - The authorization request to process
    ///
    /// # Returns
    ///
    /// Returns the created authorization session on success. The caller
    /// should use the session's `code` to build the redirect response.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `response_type` is not "code" (`UnsupportedResponseType`)
    /// - Client is not found (`InvalidClient`)
    /// - Client is inactive (`InvalidClient`)
    /// - Redirect URI is not allowed (`InvalidGrant`)
    /// - Grant type is not allowed (`InvalidGrant`)
    /// - PKCE method is not S256 (`InvalidRequest`)
    /// - PKCE challenge is invalid (`InvalidRequest`)
    /// - State has insufficient entropy (`InvalidRequest`)
    ///
    /// # Security
    ///
    /// - Never log the authorization code or state parameter
    /// - Redirect URI must exactly match a registered URI
    /// - PKCE is always required (no fallback to plain)
    pub async fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> AuthResult<AuthorizationSession> {
        // 1. Validate response_type
        if request.response_type != "code" {
            return Err(AuthError::unsupported_response_type(&request.response_type));
        }

        // 2. Validate client exists and is active
        let client = self
            .client_storage
            .find_by_client_id(&request.client_id)
            .await?
            .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

        if !client.active {
            return Err(AuthError::invalid_client("Client is inactive"));
        }

        // 3. Validate redirect_uri
        if !client.is_redirect_uri_allowed(&request.redirect_uri) {
            return Err(AuthError::invalid_grant("Invalid redirect_uri"));
        }

        // 4. Validate grant type is allowed
        if !client.is_grant_type_allowed(GrantType::AuthorizationCode) {
            return Err(AuthError::invalid_grant(
                "Client is not authorized for authorization_code grant",
            ));
        }

        // 5. Validate PKCE based on client type (RFC 8252, RFC 9207)
        if !client.confidential {
            // Public client: PKCE is REQUIRED (RFC 8252)
            if request.code_challenge.is_none() || request.code_challenge_method.is_none() {
                return Err(AuthError::invalid_request(
                    "PKCE (code_challenge and code_challenge_method) is required for public clients",
                ));
            }
        } else {
            // Confidential client: PKCE is RECOMMENDED but optional (RFC 9207)
            if request.code_challenge.is_none() && request.code_challenge_method.is_none() {
                tracing::warn!(
                    client_id = %request.client_id,
                    "Confidential client is not using PKCE (recommended per RFC 9207)"
                );
            }
        }

        // 6. Validate PKCE if provided
        if let Some(ref method) = request.code_challenge_method {
            let _method = PkceChallengeMethod::parse(method).map_err(|e| {
                AuthError::invalid_request(format!("Invalid PKCE challenge method: {}", e))
            })?;
        }

        if let Some(ref challenge) = request.code_challenge {
            let _challenge = PkceChallenge::new(challenge.clone()).map_err(|e| {
                AuthError::invalid_request(format!("Invalid PKCE challenge: {}", e))
            })?;
        }

        // Ensure both PKCE parameters are provided together if one is provided
        if request.code_challenge.is_some() != request.code_challenge_method.is_some() {
            return Err(AuthError::invalid_request(
                "Both code_challenge and code_challenge_method must be provided together",
            ));
        }

        // 7. Validate state entropy
        self.validate_state_entropy(&request.state)?;

        // 8. Validate aud parameter if required
        if self.config.require_aud && request.aud.is_empty() {
            return Err(AuthError::invalid_request(
                "Missing required parameter: aud",
            ));
        }

        // 9. Validate scopes are allowed for this client
        if !request.scope.is_empty() {
            for scope in request.scope.split_whitespace() {
                if !client.is_scope_allowed(scope) {
                    return Err(AuthError::invalid_scope(format!(
                        "Scope '{}' is not allowed for this client",
                        scope
                    )));
                }
            }
        }

        // 10. Parse scopes for launch validation
        let scopes = SmartScopes::parse(&request.scope)
            .map_err(|e| AuthError::invalid_scope(format!("Invalid scope format: {}", e)))?;

        // 11. Process EHR launch if present
        let launch_context = self
            .process_launch_parameter(request.launch.as_deref(), &scopes)
            .await?;

        // 12. Create authorization session
        let now = OffsetDateTime::now_utc();
        let session = AuthorizationSession {
            id: Uuid::new_v4(),
            code: AuthorizationSession::generate_code(),
            client_id: request.client_id.clone(),
            redirect_uri: request.redirect_uri.clone(),
            scope: request.scope.clone(),
            state: request.state.clone(),
            code_challenge: request.code_challenge.clone(), // Already Option<String>
            code_challenge_method: request.code_challenge_method.clone(), // Already Option<String>
            user_id: None,
            launch_context,
            nonce: request.nonce.clone(),
            aud: request.aud.clone(),
            created_at: now,
            expires_at: now + self.config.code_lifetime,
            consumed_at: None,
        };

        // 13. Store session
        self.session_storage.create(&session).await?;

        Ok(session)
    }

    /// Validates that the state parameter has sufficient entropy.
    ///
    /// The state parameter must have at least `min_state_entropy_bits` bits
    /// of entropy to provide adequate CSRF protection.
    ///
    /// # Entropy Estimation
    ///
    /// This uses a conservative estimate based on character set:
    /// - Base64 URL-safe characters: ~6 bits per character
    /// - Required length: `min_state_entropy_bits / 6` characters
    ///
    /// For 122 bits, this requires approximately 21 characters.
    fn validate_state_entropy(&self, state: &str) -> AuthResult<()> {
        // Conservative entropy estimate: ~6 bits per base64 character
        let estimated_bits = state.len() * 6;

        if estimated_bits < self.config.min_state_entropy_bits {
            return Err(AuthError::invalid_request(format!(
                "State parameter has insufficient entropy (minimum {} bits required, estimated {} bits)",
                self.config.min_state_entropy_bits, estimated_bits
            )));
        }

        Ok(())
    }

    /// Processes the launch parameter from an EHR launch.
    ///
    /// This method handles the `launch` parameter in authorization requests,
    /// retrieving the corresponding launch context from storage and converting
    /// it to a session launch context.
    ///
    /// # Arguments
    ///
    /// * `launch_param` - The opaque launch parameter from the authorization request
    /// * `scopes` - The parsed SMART scopes from the request
    ///
    /// # Returns
    ///
    /// Returns `Some(LaunchContext)` if a valid launch context was found,
    /// or `None` for standalone launches (no launch parameter).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `launch` scope requested but no launch parameter provided
    /// - Launch parameter present but no `launch` scope requested
    /// - Launch context storage not configured
    /// - Launch context not found or expired
    /// - Launch scopes don't match context (e.g., `launch/patient` without patient)
    async fn process_launch_parameter(
        &self,
        launch_param: Option<&str>,
        scopes: &SmartScopes,
    ) -> AuthResult<Option<LaunchContext>> {
        // Case 1: launch scope requested but no launch parameter
        if scopes.launch && launch_param.is_none() {
            return Err(AuthError::invalid_scope(
                "launch scope requested but no launch parameter provided",
            ));
        }

        // Case 2: launch parameter present but no launch scope
        if launch_param.is_some() && !scopes.launch {
            return Err(AuthError::invalid_scope(
                "launch parameter present but launch scope not requested",
            ));
        }

        // Case 3: No launch parameter and no launch scope - standalone launch
        let Some(launch_id) = launch_param else {
            return Ok(None);
        };

        // Case 4: EHR launch - retrieve launch context from storage
        let Some(launch_storage) = &self.launch_context_storage else {
            return Err(AuthError::internal("Launch context storage not configured"));
        };

        let stored_context = launch_storage
            .get(launch_id)
            .await?
            .ok_or_else(|| AuthError::invalid_grant("Invalid or expired launch parameter"))?;

        // Validate launch scopes against context
        self.validate_launch_scopes(scopes, &stored_context)?;

        // Convert StoredLaunchContext to session LaunchContext
        let launch_context = LaunchContext {
            patient: stored_context.patient.clone(),
            encounter: stored_context.encounter.clone(),
            fhir_context: stored_context
                .fhir_context
                .iter()
                .map(|item| crate::oauth::session::FhirContextItem {
                    reference: item.reference.clone(),
                    role: item.role.clone(),
                })
                .collect(),
            need_patient_banner: stored_context.need_patient_banner,
            smart_style_url: stored_context.smart_style_url.clone(),
            intent: stored_context.intent.clone(),
        };

        Ok(Some(launch_context))
    }

    /// Validates that launch scopes match the launch context.
    ///
    /// # Arguments
    ///
    /// * `scopes` - The parsed SMART scopes from the request
    /// * `context` - The stored launch context
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `launch/patient` scope requested but no patient in context
    /// - `launch/encounter` scope requested but no encounter in context
    fn validate_launch_scopes(
        &self,
        scopes: &SmartScopes,
        context: &StoredLaunchContext,
    ) -> AuthResult<()> {
        // launch/patient requires patient in context
        if scopes.launch_patient && context.patient.is_none() {
            return Err(AuthError::invalid_scope(
                "launch/patient scope requested but no patient in launch context",
            ));
        }

        // launch/encounter requires encounter in context
        if scopes.launch_encounter && context.encounter.is_none() {
            return Err(AuthError::invalid_scope(
                "launch/encounter scope requested but no encounter in launch context",
            ));
        }

        Ok(())
    }

    /// Validates that standalone context selection matches the requested scopes.
    ///
    /// For standalone launches, the user must select patient/encounter context
    /// during the authorization flow if `launch/patient` or `launch/encounter`
    /// scopes were requested. This method validates that the required context
    /// was provided.
    ///
    /// # Arguments
    ///
    /// * `session` - The authorization session to validate
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `launch/patient` scope requested but no patient in launch_context
    /// - `launch/encounter` scope requested but no encounter in launch_context
    ///
    /// # Example
    ///
    /// ```ignore
    /// // After user selects patient in UI
    /// session_storage.update_launch_context(session.id, launch_context).await?;
    ///
    /// // Validate before issuing authorization code
    /// auth_service.validate_standalone_context(&session)?;
    /// ```
    pub fn validate_standalone_context(&self, session: &AuthorizationSession) -> AuthResult<()> {
        let scopes = SmartScopes::parse(&session.scope)
            .map_err(|e| AuthError::internal(format!("Failed to parse session scopes: {}", e)))?;

        // For standalone launch, check that required context was selected
        if scopes.is_standalone_with_context() {
            let launch_context = session.launch_context.as_ref();

            // launch/patient requires patient in context
            if scopes.launch_patient {
                let has_patient = launch_context
                    .map(|ctx| ctx.patient.is_some())
                    .unwrap_or(false);
                if !has_patient {
                    return Err(AuthError::invalid_grant(
                        "launch/patient scope requires patient selection",
                    ));
                }
            }

            // launch/encounter requires encounter in context
            if scopes.launch_encounter {
                let has_encounter = launch_context
                    .map(|ctx| ctx.encounter.is_some())
                    .unwrap_or(false);
                if !has_encounter {
                    return Err(AuthError::invalid_grant(
                        "launch/encounter scope requires encounter selection",
                    ));
                }
            }
        }

        Ok(())
    }

    /// Gets the session storage reference.
    ///
    /// Useful for operations that need direct access to session storage,
    /// such as updating user info after authentication.
    #[must_use]
    pub fn session_storage(&self) -> &Arc<dyn SessionStorage> {
        &self.session_storage
    }

    /// Gets the client storage reference.
    #[must_use]
    pub fn client_storage(&self) -> &Arc<dyn ClientStorage> {
        &self.client_storage
    }

    /// Gets the launch context storage reference.
    #[must_use]
    pub fn launch_context_storage(&self) -> Option<&Arc<dyn LaunchContextStorage>> {
        self.launch_context_storage.as_ref()
    }

    /// Gets the service configuration.
    #[must_use]
    pub fn config(&self) -> &AuthorizationConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::pkce::PkceVerifier;
    use crate::types::Client;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Mock client storage for testing.
    struct MockClientStorage {
        clients: RwLock<HashMap<String, Client>>,
    }

    impl MockClientStorage {
        fn new() -> Self {
            Self {
                clients: RwLock::new(HashMap::new()),
            }
        }

        fn add_client(&self, client: Client) {
            self.clients
                .write()
                .unwrap()
                .insert(client.client_id.clone(), client);
        }
    }

    #[async_trait::async_trait]
    impl ClientStorage for MockClientStorage {
        async fn find_by_client_id(&self, client_id: &str) -> AuthResult<Option<Client>> {
            Ok(self.clients.read().unwrap().get(client_id).cloned())
        }

        async fn create(&self, client: &Client) -> AuthResult<Client> {
            self.add_client(client.clone());
            Ok(client.clone())
        }

        async fn update(&self, _client_id: &str, client: &Client) -> AuthResult<Client> {
            self.add_client(client.clone());
            Ok(client.clone())
        }

        async fn delete(&self, client_id: &str) -> AuthResult<()> {
            self.clients.write().unwrap().remove(client_id);
            Ok(())
        }

        async fn list(&self, _limit: i64, _offset: i64) -> AuthResult<Vec<Client>> {
            Ok(self.clients.read().unwrap().values().cloned().collect())
        }

        async fn verify_secret(&self, _client_id: &str, _secret: &str) -> AuthResult<bool> {
            Ok(true)
        }

        async fn regenerate_secret(&self, client_id: &str) -> AuthResult<(Client, String)> {
            let mut clients = self.clients.write().unwrap();
            if let Some(client) = clients.get(client_id).cloned() {
                let new_secret = "new-test-secret".to_string();
                let mut updated = client;
                updated.client_secret = Some(new_secret.clone());
                clients.insert(client_id.to_string(), updated.clone());
                Ok((updated, new_secret))
            } else {
                Err(crate::error::AuthError::invalid_client(format!(
                    "Client not found: {}",
                    client_id
                )))
            }
        }
    }

    /// Mock session storage for testing.
    struct MockSessionStorage {
        sessions: RwLock<HashMap<String, AuthorizationSession>>,
    }

    impl MockSessionStorage {
        fn new() -> Self {
            Self {
                sessions: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl SessionStorage for MockSessionStorage {
        async fn create(&self, session: &AuthorizationSession) -> AuthResult<()> {
            self.sessions
                .write()
                .unwrap()
                .insert(session.code.clone(), session.clone());
            Ok(())
        }

        async fn find_by_code(&self, code: &str) -> AuthResult<Option<AuthorizationSession>> {
            Ok(self.sessions.read().unwrap().get(code).cloned())
        }

        async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<AuthorizationSession>> {
            Ok(self
                .sessions
                .read()
                .unwrap()
                .values()
                .find(|s| s.id == id)
                .cloned())
        }

        async fn consume(&self, code: &str) -> AuthResult<AuthorizationSession> {
            let mut sessions = self.sessions.write().unwrap();
            let session = sessions
                .get_mut(code)
                .ok_or_else(|| AuthError::invalid_grant("Code not found"))?;

            if session.is_consumed() {
                return Err(AuthError::invalid_grant("Code already consumed"));
            }

            if session.is_expired() {
                return Err(AuthError::invalid_grant("Code expired"));
            }

            session.consumed_at = Some(OffsetDateTime::now_utc());
            Ok(session.clone())
        }

        async fn update_user(&self, id: Uuid, user_id: &str) -> AuthResult<()> {
            let mut sessions = self.sessions.write().unwrap();
            for session in sessions.values_mut() {
                if session.id == id {
                    session.user_id = Some(user_id.to_string());
                    return Ok(());
                }
            }
            Err(AuthError::invalid_grant("Session not found"))
        }

        async fn update_launch_context(
            &self,
            id: Uuid,
            launch_context: crate::oauth::session::LaunchContext,
        ) -> AuthResult<()> {
            let mut sessions = self.sessions.write().unwrap();
            for session in sessions.values_mut() {
                if session.id == id {
                    session.launch_context = Some(launch_context);
                    return Ok(());
                }
            }
            Err(AuthError::invalid_grant("Session not found"))
        }

        async fn cleanup_expired(&self) -> AuthResult<u64> {
            let mut sessions = self.sessions.write().unwrap();
            let before = sessions.len();
            sessions.retain(|_, s| !s.is_expired());
            Ok((before - sessions.len()) as u64)
        }

        async fn delete_by_client(&self, client_id: &str) -> AuthResult<u64> {
            let mut sessions = self.sessions.write().unwrap();
            let before = sessions.len();
            sessions.retain(|_, s| s.client_id != client_id);
            Ok((before - sessions.len()) as u64)
        }
    }

    fn create_test_client() -> Client {
        Client {
            client_id: "test-client".to_string(),
            client_secret: None,
            name: "Test Client".to_string(),
            description: None,
            grant_types: vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
            redirect_uris: vec!["https://app.example.com/callback".to_string()],
            post_logout_redirect_uris: vec![],
            scopes: vec![], // Empty means all scopes allowed
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

    fn create_test_request() -> AuthorizationRequest {
        let verifier = PkceVerifier::generate();
        let challenge = PkceChallenge::from_verifier(&verifier);

        AuthorizationRequest {
            response_type: "code".to_string(),
            client_id: "test-client".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "openid patient/*.read".to_string(),
            state: "abcdefghijklmnopqrstuvwxyz".to_string(), // 26 chars = ~156 bits
            code_challenge: Some(challenge.into_inner()),
            code_challenge_method: Some("S256".to_string()),
            aud: "https://fhir.example.com/r4".to_string(),
            launch: None,
            nonce: None,
        }
    }

    fn create_service() -> (
        AuthorizationService,
        Arc<MockClientStorage>,
        Arc<MockSessionStorage>,
    ) {
        let client_storage = Arc::new(MockClientStorage::new());
        let session_storage = Arc::new(MockSessionStorage::new());

        let service = AuthorizationService::new(
            client_storage.clone(),
            session_storage.clone(),
            AuthorizationConfig::default(),
        );

        (service, client_storage, session_storage)
    }

    #[tokio::test]
    async fn test_authorize_success() {
        let (service, client_storage, session_storage) = create_service();
        client_storage.add_client(create_test_client());

        let request = create_test_request();
        let result = service.authorize(&request).await;

        assert!(result.is_ok());
        let session = result.unwrap();

        assert_eq!(session.client_id, "test-client");
        assert_eq!(session.redirect_uri, "https://app.example.com/callback");
        assert_eq!(session.scope, "openid patient/*.read");
        assert_eq!(session.code.len(), 43); // Base64url encoded 32 bytes
        assert!(!session.is_expired());
        assert!(!session.is_consumed());
        assert!(session.user_id.is_none());

        // Verify session was stored
        let stored = session_storage.find_by_code(&session.code).await.unwrap();
        assert!(stored.is_some());
    }

    #[tokio::test]
    async fn test_authorize_invalid_response_type() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        request.response_type = "token".to_string();

        let result = service.authorize(&request).await;
        assert!(matches!(
            result,
            Err(AuthError::UnsupportedResponseType { .. })
        ));
    }

    #[tokio::test]
    async fn test_authorize_unknown_client() {
        let (service, _, _) = create_service();

        let request = create_test_request();
        let result = service.authorize(&request).await;

        assert!(matches!(result, Err(AuthError::InvalidClient { .. })));
    }

    #[tokio::test]
    async fn test_authorize_inactive_client() {
        let (service, client_storage, _) = create_service();

        let mut client = create_test_client();
        client.active = false;
        client_storage.add_client(client);

        let request = create_test_request();
        let result = service.authorize(&request).await;

        assert!(matches!(result, Err(AuthError::InvalidClient { .. })));
    }

    #[tokio::test]
    async fn test_authorize_invalid_redirect_uri() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        request.redirect_uri = "https://evil.example.com/callback".to_string();

        let result = service.authorize(&request).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_authorize_unauthorized_grant_type() {
        let (service, client_storage, _) = create_service();

        let mut client = create_test_client();
        client.grant_types = vec![GrantType::ClientCredentials]; // No auth code
        client_storage.add_client(client);

        let request = create_test_request();
        let result = service.authorize(&request).await;

        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_authorize_invalid_pkce_method() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        request.code_challenge_method = Some("plain".to_string());

        let result = service.authorize(&request).await;
        assert!(matches!(result, Err(AuthError::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn test_authorize_invalid_pkce_challenge() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        request.code_challenge = Some("not-valid-base64!!!".to_string());

        let result = service.authorize(&request).await;
        assert!(matches!(result, Err(AuthError::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn test_authorize_insufficient_state_entropy() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        request.state = "short".to_string(); // Only 5 chars = ~30 bits

        let result = service.authorize(&request).await;
        assert!(matches!(result, Err(AuthError::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn test_authorize_missing_aud() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        request.aud = String::new();

        let result = service.authorize(&request).await;
        assert!(matches!(result, Err(AuthError::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn test_authorize_invalid_scope() {
        let (service, client_storage, _) = create_service();

        let mut client = create_test_client();
        client.scopes = vec!["openid".to_string()]; // Only openid allowed
        client_storage.add_client(client);

        let mut request = create_test_request();
        request.scope = "openid patient/*.read".to_string(); // patient/*.read not allowed

        let result = service.authorize(&request).await;
        assert!(matches!(result, Err(AuthError::InvalidScope { .. })));
    }

    #[tokio::test]
    async fn test_authorize_with_optional_params() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        // Note: launch parameter requires launch scope, so we only test nonce here
        request.nonce = Some("nonce-456".to_string());

        let result = service.authorize(&request).await;
        assert!(result.is_ok());

        let session = result.unwrap();
        assert_eq!(session.nonce, Some("nonce-456".to_string()));
    }

    #[tokio::test]
    async fn test_authorize_launch_scope_without_parameter() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        request.launch = None;
        request.scope = "launch openid patient/*.rs".to_string();

        let result = service.authorize(&request).await;
        // Should fail: launch scope without launch parameter
        assert!(matches!(result, Err(AuthError::InvalidScope { .. })));
    }

    #[tokio::test]
    async fn test_authorize_launch_parameter_without_scope() {
        let (service, client_storage, _) = create_service();
        client_storage.add_client(create_test_client());

        let mut request = create_test_request();
        request.launch = Some("launch123".to_string());
        request.scope = "openid patient/*.rs".to_string(); // No launch scope

        let result = service.authorize(&request).await;
        // Should fail: launch parameter without launch scope
        assert!(matches!(result, Err(AuthError::InvalidScope { .. })));
    }

    #[tokio::test]
    async fn test_config_defaults() {
        let config = AuthorizationConfig::default();
        assert_eq!(config.code_lifetime, Duration::minutes(10));
        assert_eq!(config.min_state_entropy_bits, 122);
        assert!(config.require_aud);
    }

    #[tokio::test]
    async fn test_config_builder() {
        let config = AuthorizationConfig::default()
            .with_code_lifetime(Duration::minutes(5))
            .with_min_state_entropy(64)
            .without_aud_requirement();

        assert_eq!(config.code_lifetime, Duration::minutes(5));
        assert_eq!(config.min_state_entropy_bits, 64);
        assert!(!config.require_aud);
    }

    #[test]
    fn test_validate_state_entropy_sufficient() {
        let (service, _, _) = create_service();

        // 21 characters * 6 bits = 126 bits > 122 bits
        let result = service.validate_state_entropy("abcdefghijklmnopqrstu");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_state_entropy_insufficient() {
        let (service, _, _) = create_service();

        // 10 characters * 6 bits = 60 bits < 122 bits
        let result = service.validate_state_entropy("abcdefghij");
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------------
    // Standalone Context Validation Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_validate_standalone_context_no_context_needed() {
        let (service, _, _) = create_service();

        // No launch/patient or launch/encounter scope - no context needed
        let session = AuthorizationSession {
            id: Uuid::new_v4(),
            code: "test-code".to_string(),
            client_id: "test-client".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "patient/Patient.rs openid".to_string(),
            state: "test-state".to_string(),
            code_challenge: Some("test-challenge".to_string()),
            code_challenge_method: Some("S256".to_string()),
            user_id: None,
            launch_context: None,
            nonce: None,
            aud: "https://fhir.example.com".to_string(),
            created_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + Duration::minutes(10),
            consumed_at: None,
        };

        let result = service.validate_standalone_context(&session);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_standalone_context_patient_provided() {
        let (service, _, _) = create_service();

        // launch/patient requested and patient provided
        let session = AuthorizationSession {
            id: Uuid::new_v4(),
            code: "test-code".to_string(),
            client_id: "test-client".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "launch/patient patient/Patient.rs".to_string(),
            state: "test-state".to_string(),
            code_challenge: Some("test-challenge".to_string()),
            code_challenge_method: Some("S256".to_string()),
            user_id: None,
            launch_context: Some(crate::oauth::session::LaunchContext {
                patient: Some("patient-123".to_string()),
                encounter: None,
                fhir_context: vec![],
                need_patient_banner: true,
                smart_style_url: None,
                intent: None,
            }),
            nonce: None,
            aud: "https://fhir.example.com".to_string(),
            created_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + Duration::minutes(10),
            consumed_at: None,
        };

        let result = service.validate_standalone_context(&session);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_standalone_context_patient_missing() {
        let (service, _, _) = create_service();

        // launch/patient requested but no patient selected
        let session = AuthorizationSession {
            id: Uuid::new_v4(),
            code: "test-code".to_string(),
            client_id: "test-client".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "launch/patient patient/Patient.rs".to_string(),
            state: "test-state".to_string(),
            code_challenge: Some("test-challenge".to_string()),
            code_challenge_method: Some("S256".to_string()),
            user_id: None,
            launch_context: None, // No context set
            nonce: None,
            aud: "https://fhir.example.com".to_string(),
            created_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + Duration::minutes(10),
            consumed_at: None,
        };

        let result = service.validate_standalone_context(&session);
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[test]
    fn test_validate_standalone_context_encounter_missing() {
        let (service, _, _) = create_service();

        // launch/encounter requested but no encounter selected
        let session = AuthorizationSession {
            id: Uuid::new_v4(),
            code: "test-code".to_string(),
            client_id: "test-client".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "launch/encounter patient/Encounter.rs".to_string(),
            state: "test-state".to_string(),
            code_challenge: Some("test-challenge".to_string()),
            code_challenge_method: Some("S256".to_string()),
            user_id: None,
            launch_context: Some(crate::oauth::session::LaunchContext {
                patient: Some("patient-123".to_string()), // Has patient but no encounter
                encounter: None,
                fhir_context: vec![],
                need_patient_banner: true,
                smart_style_url: None,
                intent: None,
            }),
            nonce: None,
            aud: "https://fhir.example.com".to_string(),
            created_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + Duration::minutes(10),
            consumed_at: None,
        };

        let result = service.validate_standalone_context(&session);
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }
}
