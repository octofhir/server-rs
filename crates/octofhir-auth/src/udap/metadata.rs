//! The `.well-known/udap` document.

use std::sync::RwLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;
use x509_parser::certificate::X509Certificate;
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::FromDer;

use super::{B2B_EXTENSION, SIGNING_ALGORITHMS};
use crate::config::UdapConfig;

/// How long a signed document stays valid, and from what age it is signed anew.
const SIGNED_LIFETIME_SECONDS: i64 = 3600;
const RENEW_AFTER_SECONDS: i64 = 1800;

/// Where the document points.
#[derive(Debug, Clone)]
pub struct Endpoints {
    /// The FHIR base URL: the issuer of the document.
    pub base_url: String,
    pub authorization: String,
    pub token: String,
    pub registration: String,
}

struct Signed {
    at: i64,
    token: String,
}

/// Builds the document; the signature is reused while it is fresh, an RSA signature being
/// far dearer than the request.
pub struct UdapMetadata {
    endpoints: Endpoints,
    scopes: Vec<String>,
    key: EncodingKey,
    chain: Vec<String>,
    signed: RwLock<Option<Signed>>,
}

impl UdapMetadata {
    /// Reads the certificate and key of this server. The `iss` of the document has to be in
    /// the Subject Alternative Name of the certificate, so a mismatch stops here.
    ///
    /// # Errors
    ///
    /// Returns a description of what is missing or does not fit.
    pub fn from_config(config: &UdapConfig, endpoints: Endpoints) -> Result<Self, String> {
        let certificate_path = config
            .certificate
            .as_ref()
            .ok_or("auth.udap.certificate is required")?;
        let key_path = config
            .private_key
            .as_ref()
            .ok_or("auth.udap.private_key is required")?;
        let chain = CertificateDer::pem_file_iter(certificate_path)
            .map_err(|error| format!("{}: {error}", certificate_path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{}: {error}", certificate_path.display()))?;
        let leaf = chain
            .first()
            .ok_or_else(|| format!("{} holds no certificate", certificate_path.display()))?;
        let (_, parsed) = X509Certificate::from_der(leaf.as_ref())
            .map_err(|error| format!("{}: {error}", certificate_path.display()))?;
        let named =
            parsed
                .subject_alternative_name()
                .map_err(|error| format!("{}: {error}", certificate_path.display()))?
                .is_some_and(|extension| {
                    extension.value.general_names.iter().any(
                        |name| matches!(name, GeneralName::URI(uri) if *uri == endpoints.base_url),
                    )
                });
        if !named {
            return Err(format!(
                "The certificate has to carry {} as a URI Subject Alternative Name",
                endpoints.base_url
            ));
        }
        let pem =
            std::fs::read(key_path).map_err(|error| format!("{}: {error}", key_path.display()))?;
        // The document is signed with RS256, whatever else the server accepts.
        let key = EncodingKey::from_rsa_pem(&pem)
            .map_err(|error| format!("{}: {error}", key_path.display()))?;
        Ok(Self {
            endpoints,
            scopes: config.scopes.clone(),
            key,
            chain: chain
                .iter()
                .map(|certificate| STANDARD.encode(certificate.as_ref()))
                .collect(),
            signed: RwLock::new(None),
        })
    }

    fn sign(&self, now: i64) -> Result<String, String> {
        let mut header = Header::new(Algorithm::RS256);
        header.x5c = Some(self.chain.clone());
        let claims = json!({
            "iss": self.endpoints.base_url,
            "sub": self.endpoints.base_url,
            "iat": now,
            "exp": now + SIGNED_LIFETIME_SECONDS,
            "jti": Uuid::new_v4().to_string(),
            "authorization_endpoint": self.endpoints.authorization,
            "token_endpoint": self.endpoints.token,
            "registration_endpoint": self.endpoints.registration,
        });
        encode(&header, &claims, &self.key).map_err(|error| error.to_string())
    }

    fn signed_metadata(&self) -> Result<String, String> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        if let Ok(signed) = self.signed.read()
            && let Some(signed) = signed.as_ref()
            && now - signed.at < RENEW_AFTER_SECONDS
        {
            return Ok(signed.token.clone());
        }
        let token = self.sign(now)?;
        if let Ok(mut signed) = self.signed.write() {
            *signed = Some(Signed {
                at: now,
                token: token.clone(),
            });
        }
        Ok(token)
    }

    /// The document.
    ///
    /// # Errors
    ///
    /// Returns a description if signing fails.
    pub fn document(&self) -> Result<Value, String> {
        Ok(json!({
            "udap_versions_supported": ["1"],
            "udap_profiles_supported": ["udap_dcr", "udap_authn", "udap_authz"],
            "udap_authorization_extensions_supported": [B2B_EXTENSION],
            "udap_authorization_extensions_required": [B2B_EXTENSION],
            "udap_certifications_supported": [],
            "udap_certifications_required": [],
            "grant_types_supported": ["authorization_code", "refresh_token", "client_credentials"],
            "scopes_supported": self.scopes,
            "authorization_endpoint": self.endpoints.authorization,
            "token_endpoint": self.endpoints.token,
            "token_endpoint_auth_methods_supported": ["private_key_jwt"],
            "token_endpoint_auth_signing_alg_values_supported": SIGNING_ALGORITHMS,
            "registration_endpoint": self.endpoints.registration,
            "registration_endpoint_jwt_signing_alg_values_supported": SIGNING_ALGORITHMS,
            "signed_metadata": self.signed_metadata()?,
        }))
    }
}
