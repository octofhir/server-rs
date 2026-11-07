//! Whether a JWT comes from a member of a trust community.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, CertificateRevocationListDer, TrustAnchor, UnixTime};
use serde::de::DeserializeOwned;
use time::OffsetDateTime;
use webpki::{
    ALL_VERIFICATION_ALGS, CertRevocationList, EndEntityCert, ExpirationPolicy, KeyUsage,
    OwnedCertRevocationList, RevocationCheckDepth, RevocationOptionsBuilder, UnknownStatusPolicy,
    anchor_from_trusted_cert,
};
use x509_parser::certificate::X509Certificate;
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::FromDer;
use x509_parser::public_key::PublicKey;

use super::MAX_CLOCK_SKEW_SECONDS;
use crate::config::{UdapCommunityConfig, UdapConfig};

/// Why a JWT was refused. The kinds map to different error codes on the wire.
#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    /// Not a well formed JWT of this profile.
    #[error("{0}")]
    Malformed(String),
    /// The signature does not verify with the leaf certificate.
    #[error("Invalid signature: {0}")]
    Signature(String),
    /// No valid chain leads to a community anchor, or the certificate does not name the sender.
    #[error("{0}")]
    Untrusted(String),
    /// Audience, expiry or another claim is wrong.
    #[error("{0}")]
    Claims(String),
}

/// A JWT that passed: its claims, who the certificate names, and whose community it is.
#[derive(Debug)]
pub struct Verified<C> {
    pub claims: C,
    /// `uniformResourceIdentifier` entries of the Subject Alternative Name extension.
    pub subject_uris: Vec<String>,
    /// URI of the community whose anchor the chain leads to.
    pub community: String,
}

struct Community {
    uri: String,
    anchors: Vec<TrustAnchor<'static>>,
    revocations: Vec<CertRevocationList<'static>>,
}

impl Community {
    fn load(config: &UdapCommunityConfig) -> Result<Self, String> {
        let mut anchors = Vec::new();
        for path in &config.anchors {
            for certificate in CertificateDer::pem_file_iter(path)
                .map_err(|error| format!("{}: {error}", path.display()))?
            {
                let certificate =
                    certificate.map_err(|error| format!("{}: {error}", path.display()))?;
                let anchor = anchor_from_trusted_cert(&certificate)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                anchors.push(anchor.to_owned());
            }
        }
        if anchors.is_empty() {
            return Err(format!("Community {} has no trust anchors", config.uri));
        }
        let mut revocations = Vec::new();
        for path in &config.revocation_lists {
            for list in CertificateRevocationListDer::pem_file_iter(path)
                .map_err(|error| format!("{}: {error}", path.display()))?
            {
                let list = list.map_err(|error| format!("{}: {error}", path.display()))?;
                let owned = OwnedCertRevocationList::from_der(list.as_ref())
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                revocations.push(CertRevocationList::from(owned));
            }
        }
        Ok(Self {
            uri: config.uri.clone(),
            anchors,
            revocations,
        })
    }

    fn accepts(
        &self,
        leaf: &EndEntityCert<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<(), webpki::Error> {
        let lists: Vec<&CertRevocationList<'_>> = self.revocations.iter().collect();
        // A list that has run out says nothing about today, so it counts as a failure.
        let revocation = RevocationOptionsBuilder::new(&lists).ok().map(|builder| {
            builder
                .with_depth(RevocationCheckDepth::Chain)
                .with_status_policy(UnknownStatusPolicy::Allow)
                .with_expiration_policy(ExpirationPolicy::Enforce)
                .build()
        });
        leaf.verify_for_usage(
            ALL_VERIFICATION_ALGS,
            &self.anchors,
            intermediates,
            now,
            KeyUsage::client_auth(),
            revocation,
            None,
        )
        .map(|_| ())
    }
}

/// The trust communities this server belongs to.
pub struct Trust {
    communities: Vec<Community>,
}

impl Trust {
    /// Reads the anchors and revocation lists.
    ///
    /// # Errors
    ///
    /// Returns a description of the first file that cannot be used.
    pub fn from_config(config: &UdapConfig) -> Result<Self, String> {
        let communities = config
            .communities
            .iter()
            .map(Community::load)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { communities })
    }

    /// Whether this URI names one of the communities.
    #[must_use]
    pub fn has_community(&self, uri: &str) -> bool {
        self.communities
            .iter()
            .any(|community| community.uri == uri)
    }

    /// Validates a JWT: the signature with the key of the leaf in `x5c`, the claims common to
    /// all UDAP tokens, then the chain up to an anchor.
    ///
    /// # Errors
    ///
    /// Returns the [`TrustError`] kind of the first check that failed.
    pub fn verify<C: DeserializeOwned>(
        &self,
        token: &str,
        audience: &str,
    ) -> Result<Verified<C>, TrustError> {
        self.verify_with(token, Some(audience), None)
    }

    /// Validates the `signed_metadata` of a server at `issuer`: like [`Trust::verify`], with no
    /// audience (the document is for anyone), `iss` equal to `issuer`, and `issuer` among the
    /// URIs of the certificate.
    ///
    /// # Errors
    ///
    /// Returns the [`TrustError`] kind of the first check that failed.
    pub fn verify_metadata<C: DeserializeOwned>(
        &self,
        token: &str,
        issuer: &str,
    ) -> Result<Verified<C>, TrustError> {
        let verified = self.verify_with(token, None, Some(issuer))?;
        if !verified.subject_uris.iter().any(|uri| uri == issuer) {
            return Err(TrustError::Claims(format!(
                "{issuer} is not a URI in the Subject Alternative Name of the certificate"
            )));
        }
        Ok(verified)
    }

    fn verify_with<C: DeserializeOwned>(
        &self,
        token: &str,
        audience: Option<&str>,
        issuer: Option<&str>,
    ) -> Result<Verified<C>, TrustError> {
        let header = decode_header(token)
            .map_err(|error| TrustError::Malformed(format!("Invalid JWT: {error}")))?;
        if !matches!(
            header.alg,
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::ES256 | Algorithm::ES384
        ) {
            return Err(TrustError::Malformed(format!(
                "Algorithm {:?} is not supported",
                header.alg
            )));
        }
        let chain = header
            .x5c
            .filter(|chain| !chain.is_empty())
            .ok_or_else(|| TrustError::Malformed("The JWT has no x5c header".into()))?
            .iter()
            .map(|certificate| {
                STANDARD
                    .decode(certificate)
                    .map(CertificateDer::from)
                    .map_err(|error| TrustError::Malformed(format!("Invalid x5c: {error}")))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let (_, leaf) = X509Certificate::from_der(chain[0].as_ref())
            .map_err(|error| TrustError::Malformed(format!("Invalid certificate: {error}")))?;
        let key = public_key(&leaf)?;
        let subject_uris = subject_uris(&leaf)?;

        let mut validation = Validation::new(header.alg);
        validation.leeway = MAX_CLOCK_SKEW_SECONDS as u64;
        match audience {
            Some(audience) => {
                validation.set_audience(&[audience]);
                validation.set_required_spec_claims(&["iss", "sub", "aud", "exp"]);
            }
            None => {
                validation.validate_aud = false;
                validation.set_required_spec_claims(&["iss", "sub", "exp"]);
            }
        }
        if let Some(issuer) = issuer {
            validation.set_issuer(&[issuer]);
        }
        let claims = decode::<C>(token, &key, &validation)
            .map_err(|error| match error.kind() {
                ErrorKind::InvalidSignature
                | ErrorKind::InvalidAlgorithm
                | ErrorKind::InvalidKeyFormat
                | ErrorKind::InvalidEcdsaKey
                | ErrorKind::InvalidRsaKey(_) => TrustError::Signature(error.to_string()),
                _ => TrustError::Claims(error.to_string()),
            })?
            .claims;

        let community = self.community_of(&chain)?;
        Ok(Verified {
            claims,
            subject_uris,
            community,
        })
    }

    fn community_of(&self, chain: &[CertificateDer<'_>]) -> Result<String, TrustError> {
        let leaf = EndEntityCert::try_from(&chain[0])
            .map_err(|error| TrustError::Untrusted(format!("Invalid certificate: {error}")))?;
        let now = OffsetDateTime::now_utc().unix_timestamp().max(0);
        let now = UnixTime::since_unix_epoch(Duration::from_secs(now as u64));
        let mut failure = None;
        for community in &self.communities {
            match community.accepts(&leaf, &chain[1..], now) {
                Ok(()) => return Ok(community.uri.clone()),
                Err(error) => failure = Some(error),
            }
        }
        Err(TrustError::Untrusted(match failure {
            Some(error) => format!("The certificate is not trusted: {error}"),
            None => "This server belongs to no trust community".to_owned(),
        }))
    }
}

fn public_key(leaf: &X509Certificate<'_>) -> Result<DecodingKey, TrustError> {
    let info = leaf.public_key();
    let bits = info.subject_public_key.data.as_ref();
    match info.parsed() {
        Ok(PublicKey::RSA(_)) => Ok(DecodingKey::from_rsa_der(bits)),
        Ok(PublicKey::EC(_)) => Ok(DecodingKey::from_ec_der(bits)),
        _ => Err(TrustError::Malformed(
            "The certificate holds neither an RSA nor an EC key".into(),
        )),
    }
}

fn subject_uris(leaf: &X509Certificate<'_>) -> Result<Vec<String>, TrustError> {
    let names = leaf
        .subject_alternative_name()
        .map_err(|error| TrustError::Malformed(format!("Invalid certificate: {error}")))?;
    Ok(names
        .into_iter()
        .flat_map(|extension| &extension.value.general_names)
        .filter_map(|name| match name {
            GeneralName::URI(uri) => Some((*uri).to_owned()),
            _ => None,
        })
        .collect())
}
