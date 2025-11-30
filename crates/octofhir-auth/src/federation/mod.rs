//! External identity provider federation.
//!
//! This module provides integration with external identity providers:
//!
//! - OpenID Connect discovery and validation
//! - External IdP configuration
//! - Token exchange for federated identities
//! - User identity mapping and linking
//! - User provisioning from IdP authentication
//! - JWK set fetching and caching
//!
//! # OIDC Discovery
//!
//! The [`OidcDiscoveryClient`] and [`DiscoveryCache`] provide functionality
//! to fetch and cache OpenID Connect provider metadata from the
//! `.well-known/openid-configuration` endpoint.
//!
//! ```ignore
//! use octofhir_auth::federation::{DiscoveryCache, DiscoveryCacheConfig};
//! use url::Url;
//!
//! let cache = DiscoveryCache::new(DiscoveryCacheConfig::default());
//! let issuer = Url::parse("https://auth.example.com")?;
//!
//! let doc = cache.get(&issuer).await?;
//! println!("Authorization endpoint: {}", doc.authorization_endpoint);
//! ```
//!
//! # Provider JWKS
//!
//! The [`ProviderJwksCache`] fetches and caches JWKS from external identity
//! providers for validating ID tokens and access tokens.
//!
//! ```ignore
//! use octofhir_auth::federation::{ProviderJwksCache, ProviderJwksCacheConfig};
//! use url::Url;
//!
//! let cache = ProviderJwksCache::new(ProviderJwksCacheConfig::default());
//! let jwks_uri = Url::parse("https://auth.example.com/.well-known/jwks.json")?;
//!
//! // Get key by kid
//! let (key, alg) = cache.get_key(&jwks_uri, "key-1").await?;
//!
//! // Or get all signing keys (for tokens without kid)
//! let keys = cache.find_signing_keys(&jwks_uri).await?;
//! ```
//!
//! # External IdP Authentication
//!
//! The [`IdpAuthService`] provides a complete authentication flow for external
//! identity providers using OpenID Connect.
//!
//! ```ignore
//! use octofhir_auth::federation::{IdpAuthService, IdpAuthServiceConfig, IdentityProviderConfig};
//! use url::Url;
//!
//! let config = IdpAuthServiceConfig::new(
//!     Url::parse("https://my-app.com/oauth/callback")?,
//! );
//! let service = IdpAuthService::new(config);
//!
//! // Register a provider
//! let provider = IdentityProviderConfig::new(
//!     "google",
//!     "Google",
//!     Url::parse("https://accounts.google.com")?,
//!     "your-client-id",
//! );
//! service.register_provider(provider).await;
//!
//! // Start authentication flow
//! let auth_request = service.start_auth("google", "state", "nonce").await?;
//! // Redirect user to auth_request.authorization_url...
//! ```
//!
//! # Client JWKS
//!
//! The [`ClientJwksCache`] provides caching for client JWKS used in
//! `private_key_jwt` authentication for backend services.
//!
//! # User Identity Linking
//!
//! The [`UserIdentity`] type and associated helper functions manage external
//! identity provider links for users. These are stored in the user's attributes.
//!
//! ```ignore
//! use octofhir_auth::federation::{UserIdentity, add_identity, get_identities};
//! use octofhir_auth::storage::User;
//!
//! let mut user = User::new("testuser");
//!
//! // Add an identity from Google
//! let identity = UserIdentity::new("google", "abc123")
//!     .with_email("user@gmail.com");
//! add_identity(&mut user, identity);
//!
//! // Get all identities
//! let identities = get_identities(&user);
//! assert_eq!(identities.len(), 1);
//! ```
//!
//! # User Provisioning
//!
//! The [`provisioning`] module provides functions for creating users from
//! IdP authentication results.
//!
//! ```ignore
//! use octofhir_auth::federation::{
//!     ProvisioningConfig, create_user_from_auth_result,
//! };
//!
//! let config = ProvisioningConfig::new()
//!     .with_auto_provision(true)
//!     .with_default_role("user");
//!
//! let user = create_user_from_auth_result(&auth_result, &config);
//! println!("Created user: {}", user.username);
//! ```

pub mod auth;
pub mod client_jwks;
pub mod discovery;
pub mod error;
pub mod identity;
pub mod jwks;
pub mod oidc;
pub mod provider;
pub mod provisioning;
pub mod resources;

pub use auth::{
    IdTokenClaims, IdpAuthRequest, IdpAuthResult, IdpAuthService, IdpAuthServiceConfig,
    TokenResponse,
};
pub use client_jwks::{ClientJwksCache, JwksCacheConfig};
pub use discovery::{DiscoveryCache, DiscoveryCacheConfig, DiscoveryError, OidcDiscoveryClient};
pub use error::IdpError;
pub use identity::{
    IDENTITIES_KEY, UserIdentity, add_identity, find_identity, find_identity_by_provider,
    get_identities, has_identity_for_provider, remove_identity, set_identities,
};
pub use jwks::{JwksError, ProviderJwksCache, ProviderJwksCacheConfig};
pub use oidc::OidcDiscoveryDocument;
pub use provider::{FhirUserType, IdentityProviderConfig, MappedUser, UserMappingConfig};
pub use provisioning::{
    ProvisioningAction, ProvisioningConfig, ProvisioningError, ProvisioningResult,
    create_fhir_resource_json, create_identity_from_auth_result, create_user_from_auth_result,
    determine_username, has_provider_identity,
};
pub use resources::{
    IdentityProviderResource, IdentityProviderType, Reference, UserIdentityElement,
    UserMappingElement, UserResource, UserValidationError,
    ValidationError as ResourceValidationError,
};
