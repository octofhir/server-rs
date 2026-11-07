//! UDAP Tiered OAuth for User Authentication (Security IG 2.0.0, "Tiered OAuth"): a client app
//! sends the `udap` scope and an `idp` parameter, and this server, the data holder, signs the
//! user in at that identity provider as an OpenID Connect client.
//!
//! For an identity provider not seen before, the server reads `{idp}/.well-known/udap`, trusts
//! it only when its `signed_metadata` validates up to an anchor of one of its communities and
//! its `scopes_supported` has `openid` and `udap`, then registers with it by UDAP Dynamic Client
//! Registration (a software statement signed with this server's certificate, scopes
//! `openid udap`). The registration is kept as an `IdentityProvider` resource of type `udap`, so
//! it is reused, survives a restart and can be adjusted (claim mapping, roles) like any other.
//! Sign-in itself then goes through the federation endpoints: authorization code with a nonce,
//! the token endpoint with a signed client assertion and `udap=1`, the ID token validated as
//! OpenID Connect says, and the user matched by `iss` and `sub`.

use std::sync::Arc;
use std::time::Duration;

use octofhir_storage::FhirStorage;
use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

use super::{MAX_TOKEN_LIFETIME_SECONDS, Trust, UdapMetadata};
use crate::config::TieredOAuthConfig;
use crate::federation::{IdentityProviderResource, IdentityProviderType, IdpAuthService, Protocol};

/// Why an identity provider cannot be used. The client app gets `invalid_idp` either way; the
/// reason goes to the log.
#[derive(Debug, thiserror::Error)]
pub enum TieredError {
    /// The provider is not trusted: no UDAP metadata, metadata that does not validate, or
    /// scopes without `openid` and `udap`.
    #[error("the identity provider is not trusted: {0}")]
    Untrusted(String),
    /// Trusted, but no client id could be obtained.
    #[error("cannot register with the identity provider: {0}")]
    Registration(String),
}

/// The claims of an identity provider's `signed_metadata` this server uses.
#[derive(Debug, Deserialize)]
struct SignedMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    registration_endpoint: Option<String>,
}

/// Registers with identity providers and keeps them as federation providers.
pub struct TieredOAuth {
    signer: Arc<UdapMetadata>,
    trust: Arc<Trust>,
    service: Arc<IdpAuthService>,
    storage: Arc<dyn FhirStorage>,
    /// Where identity providers send the browser back (the federation callback).
    callback_url: String,
    config: TieredOAuthConfig,
    http: reqwest::Client,
    /// Accept identity providers on plain HTTP (development and tests only).
    allow_http: bool,
}

impl TieredOAuth {
    /// # Panics
    ///
    /// Panics if the HTTP client cannot be built (no TLS backend).
    pub fn new(
        signer: Arc<UdapMetadata>,
        trust: Arc<Trust>,
        service: Arc<IdpAuthService>,
        storage: Arc<dyn FhirStorage>,
        callback_url: impl Into<String>,
        config: TieredOAuthConfig,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("octofhir/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client");
        Self {
            signer,
            trust,
            service,
            storage,
            callback_url: callback_url.into(),
            config,
            http,
            allow_http: false,
        }
    }

    /// Accept identity providers on plain HTTP, as `[auth.federation] allow_http` does for
    /// the other providers. For development and tests only.
    #[must_use]
    pub fn with_allow_http(mut self, allow: bool) -> Self {
        self.allow_http = allow;
        self
    }

    /// The federation provider for the identity provider at `idp`: the one registered before,
    /// or a new registration.
    ///
    /// # Errors
    ///
    /// Returns why the provider cannot be used.
    pub async fn provider_for(&self, idp: &str) -> Result<String, TieredError> {
        let idp = idp.trim_end_matches('/');
        let known = self.service.list_enabled_providers().await;
        if let Some(provider) = known.iter().find(|provider| {
            provider.protocol == Protocol::Udap
                && provider.issuer.as_str().trim_end_matches('/') == idp
        }) {
            return Ok(provider.id.clone());
        }
        let metadata = self.trusted_metadata(idp).await?;
        let client_id = self.register(idp, &metadata).await?;
        self.keep(idp, &metadata, &client_id).await
    }

    /// The identity provider's endpoints, when it is trusted.
    async fn trusted_metadata(&self, idp: &str) -> Result<SignedMetadata, TieredError> {
        let untrusted = TieredError::Untrusted;
        let base = Url::parse(idp).map_err(|error| untrusted(format!("{idp}: {error}")))?;
        if base.scheme() != "https" && !(self.allow_http && base.scheme() == "http") {
            return Err(untrusted(format!("{idp} is not an https URL")));
        }
        let document: Value = self
            .http
            .get(format!("{idp}/.well-known/udap"))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| untrusted(format!("no UDAP metadata: {error}")))?
            .json()
            .await
            .map_err(|error| untrusted(format!("UDAP metadata is not JSON: {error}")))?;
        let scopes: Vec<&str> = document["scopes_supported"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !scopes.contains(&"openid") || !scopes.contains(&"udap") {
            return Err(untrusted(
                "scopes_supported does not have both openid and udap".into(),
            ));
        }
        let signed = document["signed_metadata"]
            .as_str()
            .ok_or_else(|| untrusted("the metadata is not signed".into()))?;
        let verified = self
            .trust
            .verify_metadata::<SignedMetadata>(signed, idp)
            .map_err(|error| untrusted(error.to_string()))?;
        tracing::info!(idp, community = %verified.community, "Trusted a Tiered OAuth identity provider");
        Ok(verified.claims)
    }

    /// Registers this server as an authorization code client, and returns its client id.
    async fn register(&self, idp: &str, metadata: &SignedMetadata) -> Result<String, TieredError> {
        let failed = TieredError::Registration;
        let endpoint = metadata
            .registration_endpoint
            .as_deref()
            .ok_or_else(|| failed("the provider offers no registration endpoint".into()))?;
        let iss = self.signer.base_url();
        let client_name = self.config.client_name.clone().unwrap_or_else(|| {
            Url::parse(iss)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_else(|| iss.to_owned())
        });
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let mut statement = json!({
            "iss": iss,
            "sub": iss,
            "aud": endpoint,
            "iat": now,
            "exp": now + MAX_TOKEN_LIFETIME_SECONDS,
            "jti": Uuid::new_v4().to_string(),
            "client_name": client_name,
            "redirect_uris": [self.callback_url],
            "contacts": self.config.contacts,
            "grant_types": ["authorization_code"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "private_key_jwt",
            "scope": "openid udap",
        });
        if let Some(logo) = &self.config.logo_uri {
            statement["logo_uri"] = json!(logo);
        }
        let software_statement = self.signer.sign_claims(&statement).map_err(failed)?;
        let response = self
            .http
            .post(endpoint)
            .json(&json!({
                "software_statement": software_statement,
                "certifications": [],
                "udap": "1",
            }))
            .send()
            .await
            .map_err(|error| failed(error.to_string()))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_default();
        if !status.is_success() {
            return Err(failed(format!(
                "{status}: {} {}",
                body["error"].as_str().unwrap_or_default(),
                body["error_description"].as_str().unwrap_or_default()
            )));
        }
        let client_id = body["client_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| failed("the registration response has no client_id".into()))?;
        tracing::info!(
            idp,
            client_id,
            "Registered with a Tiered OAuth identity provider"
        );
        Ok(client_id.to_owned())
    }

    /// Keeps the registration as an `IdentityProvider` resource and starts using it.
    async fn keep(
        &self,
        idp: &str,
        metadata: &SignedMetadata,
        client_id: &str,
    ) -> Result<String, TieredError> {
        let failed = |error: String| TieredError::Registration(error);
        let name = Url::parse(idp)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| idp.to_owned());
        let resource = json!({
            "resourceType": "IdentityProvider",
            "name": name,
            "type": IdentityProviderType::Udap.as_str(),
            "issuer": idp,
            "clientId": client_id,
            "active": true,
            "scopes": ["openid", "udap"],
            "authorizationEndpoint": metadata.authorization_endpoint,
            "tokenEndpoint": metadata.token_endpoint,
        });
        let stored = self
            .storage
            .create(&resource)
            .await
            .map_err(|error| failed(error.to_string()))?;
        let mut resource: IdentityProviderResource =
            serde_json::from_value(stored.resource).map_err(|error| failed(error.to_string()))?;
        resource.id = Some(stored.id);
        let config = resource
            .to_config()
            .map_err(|error| failed(error.to_string()))?;
        let id = config.id.clone();
        self.service.register_provider(config).await;
        Ok(id)
    }
}
