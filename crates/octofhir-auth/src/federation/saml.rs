//! SAML 2.0 sign-in, as a service provider: HTTP-Redirect for the `AuthnRequest`, HTTP-POST for
//! the `Response`.
//!
//! The XML signatures are checked by `saml-rs`. A response is taken only when its `Response` or
//! its `Assertion` is signed by a key of the provider's metadata, its audience is this server,
//! and it answers a request this server made; an assertion is accepted once.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

use saml_rs::{
    AcsEndpoint, AudienceValidationPolicy, AuthnRequestSigningPolicy, BrowserInput, CertificatePem,
    Credentials, EntityId, FormField, IdpDescriptor, MetadataTrustPolicy, PendingAuthnRequest,
    PrivateKeyPem, RelayStateParam, ReplayCache, ReplayKey, ReplayPolicy, Saml, SamlError,
    SamlValidationContext, Sp, SpConfig, SpValidationPolicy, SsoResponse, StartSso,
};
use serde_json::{Map, Value, json};

/// This server as a SAML service provider.
pub struct ServiceProvider {
    sp: Saml<Sp>,
    /// Assertions already taken, until they would expire anyway.
    seen: Mutex<Seen>,
}

impl std::fmt::Debug for ServiceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceProvider").finish_non_exhaustive()
    }
}

/// A sign-in sent to a provider.
pub struct Started {
    pub pending: PendingAuthnRequest,
    /// Where to send the browser.
    pub redirect: String,
}

#[derive(Default)]
struct Seen(HashMap<ReplayKey, SystemTime>);

impl ReplayCache for Seen {
    fn check_and_store(&mut self, key: ReplayKey, expires_at: SystemTime) -> Result<(), SamlError> {
        let now = SystemTime::now();
        self.0.retain(|_, until| *until > now);
        if self.0.contains_key(&key) {
            return Err(SamlError::ReplayDetected {
                key: key.cache_key(),
            });
        }
        self.0.insert(key, expires_at);
        Ok(())
    }
}

impl ServiceProvider {
    /// `entity_id` names this server to providers; responses come to `acs`. With a certificate
    /// and its key, requests are signed.
    pub fn new(
        entity_id: &str,
        acs: &str,
        signing: Option<(String, String)>,
    ) -> Result<Self, SamlError> {
        let signs = signing.is_some();
        let credentials = match signing {
            Some((certificate, key)) => Credentials {
                signing_key: Some(PrivateKeyPem::new(key)),
                signing_certificate: Some(CertificatePem::new(certificate)),
                ..Credentials::default()
            },
            None => Credentials::default(),
        };
        let validation = SpValidationPolicy {
            // A bearer assertion has to name its audience (SAML Profiles 4.1.4.2).
            audience: AudienceValidationPolicy::Validate,
            authn_requests: if signs {
                AuthnRequestSigningPolicy::Sign
            } else {
                AuthnRequestSigningPolicy::DoNotSignForCompatibility
            },
            ..SpValidationPolicy::recommended()
        };
        let config = SpConfig::builder(EntityId::try_new(entity_id)?)
            .acs_endpoint(AcsEndpoint::post(acs)?)
            .credentials(credentials)
            .validation(validation)
            .build()?;
        Ok(Self {
            sp: Saml::sp(config)?,
            seen: Mutex::default(),
        })
    }

    /// The metadata to give providers.
    pub fn metadata(&self) -> &str {
        self.sp.metadata_xml()
    }

    /// The provider that `metadata` describes, which has to be `entity_id`.
    pub fn provider(entity_id: &str, metadata: &str) -> Result<IdpDescriptor, SamlError> {
        // The metadata is what an administrator put in; nothing else vouches for it.
        IdpDescriptor::from_metadata_xml_for(
            EntityId::try_new(entity_id)?,
            metadata,
            MetadataTrustPolicy::UnsignedForCompatibility,
        )
    }

    /// An `AuthnRequest` for `provider`; `relay_state` comes back with the response.
    pub fn start(&self, provider: &IdpDescriptor, relay_state: &str) -> Result<Started, SamlError> {
        let relay_state = RelayStateParam::try_from_option(Some(relay_state))?;
        let started = self
            .sp
            .start_sso(provider, StartSso::redirect().relay_state(relay_state))?;
        Ok(Started {
            redirect: started.outbound.redirect_url()?.to_owned(),
            pending: started.pending,
        })
    }

    /// The claims of a verified response: `sub` is the NameID, each attribute is a claim.
    pub fn finish(
        &self,
        provider: &IdpDescriptor,
        pending: &PendingAuthnRequest,
        saml_response: &str,
        relay_state: &str,
    ) -> Result<Value, SamlError> {
        let fields = vec![
            FormField::new("SAMLResponse", saml_response),
            FormField::new("RelayState", relay_state),
        ];
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        let validation =
            SamlValidationContext::new(SystemTime::now(), ReplayPolicy::RequireCache(&mut *seen));
        let session = self.sp.finish_sso(
            provider,
            pending,
            BrowserInput::<SsoResponse>::post(fields),
            validation,
        )?;
        let mut claims = Map::new();
        for attribute in session.attributes().as_slice() {
            let values: Vec<Value> = attribute
                .values()
                .iter()
                .map(|value| json!(value.as_str()))
                .collect();
            claims.insert(attribute.name().to_owned(), Value::Array(values));
        }
        claims.insert("sub".to_owned(), json!(session.name_id().value()));
        Ok(Value::Object(claims))
    }
}
