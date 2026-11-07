//! OCSP (RFC 6960): asks the responder a leaf certificate names in its Authority Information
//! Access whether the certificate is revoked.
//!
//! The request identifies the certificate by SHA-1 hashes of its issuer's name and key and its
//! serial number, as the lightweight profile (RFC 5019) has it. The answer counts only when it
//! is signed by the issuer, or by a responder certificate the issuer issued for OCSP signing,
//! and is current (`thisUpdate` not in the future, `nextUpdate` not past). Answers are kept until
//! their `nextUpdate` (an hour when there is none); a responder that cannot be reached is not
//! asked again for a few minutes.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use aws_lc_rs::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};
use rustls_pki_types::CertificateDer;
use time::OffsetDateTime;
use webpki::{ALL_VERIFICATION_ALGS, EndEntityCert};
use x509_cert::der::asn1::{Null, OctetString};
use x509_cert::der::{Any, Decode, Encode};
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{AlgorithmIdentifierOwned, ObjectIdentifier};
use x509_ocsp::{
    BasicOcspResponse, CertId, CertStatus, OcspRequest, OcspResponse, OcspResponseStatus, Request,
    TbsRequest, Version,
};
use x509_parser::certificate::X509Certificate;
use x509_parser::extensions::{GeneralName, ParsedExtension};
use x509_parser::oid_registry::OID_PKIX_ACCESS_DESCRIPTOR_OCSP;
use x509_parser::prelude::FromDer;

use super::MAX_CLOCK_SKEW_SECONDS;

/// `id-sha1`.
const SHA1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.14.3.2.26");
/// `id-pkix-ocsp-basic`.
const BASIC_RESPONSE: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.48.1.1");

const DEFAULT_LIFETIME: Duration = Duration::from_secs(3600);
const MAX_LIFETIME: Duration = Duration::from_secs(24 * 3600);
const FAILURE_LIFETIME: Duration = Duration::from_secs(300);
const MAX_SIZE: usize = 1024 * 1024;

/// What a responder said about a certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Good,
    /// Revoked, with the reason when the responder gave one.
    Revoked(String),
    /// The responder does not know the certificate.
    Unknown,
}

#[derive(Clone)]
enum Entry {
    Answer { status: Status, until: Instant },
    Failed { error: String, until: Instant },
}

/// Asks OCSP responders, and keeps their answers.
pub struct Ocsp {
    http: reqwest::Client,
    cache: Mutex<HashMap<Vec<u8>, Entry>>,
}

impl Default for Ocsp {
    fn default() -> Self {
        Self::new()
    }
}

impl Ocsp {
    /// # Panics
    ///
    /// Panics if the HTTP client cannot be built (no TLS backend).
    #[must_use]
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(concat!("octofhir/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("HTTP client"),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The status of `leaf`, issued by `issuer`: `None` when the leaf names no responder,
    /// otherwise the answer or why there is none.
    pub async fn status(
        &self,
        leaf: &CertificateDer<'_>,
        issuer: &CertificateDer<'_>,
    ) -> Option<Result<Status, String>> {
        let responders = responders(leaf);
        if responders.is_empty() {
            return None;
        }
        let key = leaf.as_ref().to_vec();
        if let Some(entry) = self.cached(&key) {
            return Some(match entry {
                Entry::Answer { status, .. } => Ok(status),
                Entry::Failed { error, .. } => Err(error),
            });
        }
        let mut failure = String::new();
        for responder in &responders {
            match self.ask(responder, leaf, issuer).await {
                Ok((status, lifetime)) => {
                    self.keep(
                        key,
                        Entry::Answer {
                            status: status.clone(),
                            until: Instant::now() + lifetime,
                        },
                    );
                    return Some(Ok(status));
                }
                Err(error) => failure = format!("{responder}: {error}"),
            }
        }
        self.keep(
            key,
            Entry::Failed {
                error: failure.clone(),
                until: Instant::now() + FAILURE_LIFETIME,
            },
        );
        Some(Err(failure))
    }

    fn cached(&self, key: &[u8]) -> Option<Entry> {
        let now = Instant::now();
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.get(key).cloned().filter(|entry| match entry {
            Entry::Answer { until, .. } | Entry::Failed { until, .. } => *until > now,
        })
    }

    fn keep(&self, key: Vec<u8>, entry: Entry) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        cache.retain(|_, entry| match entry {
            Entry::Answer { until, .. } | Entry::Failed { until, .. } => *until > now,
        });
        cache.insert(key, entry);
    }

    async fn ask(
        &self,
        responder: &str,
        leaf: &CertificateDer<'_>,
        issuer: &CertificateDer<'_>,
    ) -> Result<(Status, Duration), String> {
        let cert_id = cert_id(leaf, issuer)?;
        let request = OcspRequest {
            tbs_request: TbsRequest {
                version: Version::V1,
                requestor_name: None,
                request_list: vec![Request {
                    req_cert: cert_id.clone(),
                    single_request_extensions: None,
                }],
                request_extensions: None,
            },
            optional_signature: None,
        }
        .to_der()
        .map_err(|error| error.to_string())?;
        let response = self
            .http
            .post(responder)
            .header(reqwest::header::CONTENT_TYPE, "application/ocsp-request")
            .header(reqwest::header::ACCEPT, "application/ocsp-response")
            .body(request)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| error.to_string())?;
        let body = response.bytes().await.map_err(|error| error.to_string())?;
        if body.len() > MAX_SIZE {
            return Err("the response is too large".into());
        }
        read_response(
            &body,
            &cert_id,
            issuer,
            OffsetDateTime::now_utc().unix_timestamp(),
        )
    }
}

/// The OCSP responder URLs in the Authority Information Access of `certificate`.
#[must_use]
pub fn responders(certificate: &CertificateDer<'_>) -> Vec<String> {
    let Ok((_, certificate)) = X509Certificate::from_der(certificate.as_ref()) else {
        return Vec::new();
    };
    let mut urls = Vec::new();
    for extension in certificate.extensions() {
        if let ParsedExtension::AuthorityInfoAccess(access) = extension.parsed_extension() {
            for description in &access.accessdescs {
                if description.access_method == OID_PKIX_ACCESS_DESCRIPTOR_OCSP
                    && let GeneralName::URI(uri) = description.access_location
                    && (uri.starts_with("http://") || uri.starts_with("https://"))
                {
                    urls.push(uri.to_owned());
                }
            }
        }
    }
    urls
}

/// The certificate's identity in a request: hashes of the issuer's name and key, the serial.
fn cert_id(leaf: &CertificateDer<'_>, issuer: &CertificateDer<'_>) -> Result<CertId, String> {
    let (_, leaf) = X509Certificate::from_der(leaf.as_ref()).map_err(|error| error.to_string())?;
    let (_, issuer) =
        X509Certificate::from_der(issuer.as_ref()).map_err(|error| error.to_string())?;
    let name_hash = digest(&SHA1_FOR_LEGACY_USE_ONLY, leaf.issuer().as_raw());
    let key_hash = digest(
        &SHA1_FOR_LEGACY_USE_ONLY,
        issuer.public_key().subject_public_key.data.as_ref(),
    );
    Ok(CertId {
        hash_algorithm: AlgorithmIdentifierOwned {
            oid: SHA1,
            parameters: Some(Any::from(Null)),
        },
        issuer_name_hash: OctetString::new(name_hash.as_ref()).map_err(|e| e.to_string())?,
        issuer_key_hash: OctetString::new(key_hash.as_ref()).map_err(|e| e.to_string())?,
        serial_number: SerialNumber::new(leaf.raw_serial()).map_err(|e| e.to_string())?,
    })
}

/// Reads a responder's answer about `cert_id` at `now` (Unix seconds): the status and how long
/// it holds.
fn read_response(
    body: &[u8],
    cert_id: &CertId,
    issuer: &CertificateDer<'_>,
    now: i64,
) -> Result<(Status, Duration), String> {
    let response = OcspResponse::from_der(body).map_err(|error| error.to_string())?;
    if response.response_status != OcspResponseStatus::Successful {
        return Err(format!(
            "the responder answered {:?}",
            response.response_status
        ));
    }
    let bytes = response
        .response_bytes
        .ok_or("the answer has no response")?;
    if bytes.response_type != BASIC_RESPONSE {
        return Err("the answer is not a basic OCSP response".into());
    }
    let basic =
        BasicOcspResponse::from_der(bytes.response.as_bytes()).map_err(|e| e.to_string())?;
    verify_signer(&basic, issuer)?;
    let single = basic
        .tbs_response_data
        .responses
        .iter()
        .find(|single| &single.cert_id == cert_id)
        .ok_or("the answer is about another certificate")?;
    let this_update = seconds(&single.this_update);
    if this_update > now + MAX_CLOCK_SKEW_SECONDS {
        return Err("the answer is from the future".into());
    }
    let next_update = single.next_update.as_ref().map(seconds);
    if next_update.is_some_and(|next| next < now - MAX_CLOCK_SKEW_SECONDS) {
        return Err("the answer is out of date".into());
    }
    let lifetime = next_update.map_or(DEFAULT_LIFETIME, |next| {
        Duration::from_secs((next - now).max(0) as u64).min(MAX_LIFETIME)
    });
    let status = match &single.cert_status {
        CertStatus::Good(_) => Status::Good,
        CertStatus::Revoked(info) => Status::Revoked(
            info.revocation_reason
                .map(|reason| format!("{reason:?}"))
                .unwrap_or_else(|| "unspecified".into()),
        ),
        CertStatus::Unknown(_) => Status::Unknown,
    };
    Ok((status, lifetime))
}

fn seconds(time: &x509_ocsp::OcspGeneralizedTime) -> i64 {
    time.0.to_unix_duration().as_secs() as i64
}

/// The answer is signed by the issuer, or by a responder certificate the issuer issued for
/// OCSP signing (RFC 6960 §4.2.2.2).
fn verify_signer(basic: &BasicOcspResponse, issuer: &CertificateDer<'_>) -> Result<(), String> {
    let tbs = basic
        .tbs_response_data
        .to_der()
        .map_err(|error| error.to_string())?;
    let algorithm = basic
        .signature_algorithm
        .to_der()
        .map_err(|error| error.to_string())?;
    let signature = basic.signature.raw_bytes();
    if verify(issuer.as_ref(), &algorithm, &tbs, signature) {
        return Ok(());
    }
    for certificate in basic.certs.iter().flatten() {
        let der = certificate.to_der().map_err(|error| error.to_string())?;
        if delegated_by(&der, issuer)? && verify(&der, &algorithm, &tbs, signature) {
            return Ok(());
        }
    }
    Err("the answer is not signed by the issuer or a responder it designated".into())
}

/// Whether `responder` is a certificate the issuer issued for OCSP signing.
fn delegated_by(responder: &[u8], issuer: &CertificateDer<'_>) -> Result<bool, String> {
    let (_, parsed) = X509Certificate::from_der(responder).map_err(|e| e.to_string())?;
    let signs_ocsp = parsed.extensions().iter().any(|extension| {
        matches!(
            extension.parsed_extension(),
            ParsedExtension::ExtendedKeyUsage(usage) if usage.ocsp_signing
        )
    });
    if !signs_ocsp {
        return Ok(false);
    }
    let (_, issuer_parsed) =
        X509Certificate::from_der(issuer.as_ref()).map_err(|e| e.to_string())?;
    if parsed.issuer().as_raw() != issuer_parsed.subject().as_raw() {
        return Ok(false);
    }
    // The responder certificate's own signature, by the issuer's key.
    let certificate = x509_cert::Certificate::from_der(responder).map_err(|e| e.to_string())?;
    let algorithm = certificate
        .signature_algorithm
        .to_der()
        .map_err(|e| e.to_string())?;
    let tbs = certificate
        .tbs_certificate
        .to_der()
        .map_err(|e| e.to_string())?;
    Ok(verify(
        issuer.as_ref(),
        &algorithm,
        &tbs,
        certificate.signature.raw_bytes(),
    ))
}

/// Whether `signature` over `message` verifies with the key of `signer` by the algorithm whose
/// AlgorithmIdentifier is `algorithm` (DER).
fn verify(signer: &[u8], algorithm: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let signer = CertificateDer::from(signer.to_vec());
    let Ok(signer) = EndEntityCert::try_from(&signer) else {
        return false;
    };
    // pki-types gives the AlgorithmIdentifier contents, without the SEQUENCE header.
    let Some(contents) = sequence_contents(algorithm) else {
        return false;
    };
    ALL_VERIFICATION_ALGS
        .iter()
        .filter(|candidate| candidate.signature_alg_id().as_ref() == contents)
        .any(|candidate| {
            signer
                .verify_signature(*candidate, message, signature)
                .is_ok()
        })
}

/// The contents of a DER SEQUENCE.
fn sequence_contents(der: &[u8]) -> Option<&[u8]> {
    let (&tag, rest) = der.split_first()?;
    if tag != 0x30 {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    if first < 0x80 {
        return rest.get(..usize::from(first));
    }
    let count = usize::from(first & 0x7f);
    let (length, rest) = rest.split_at_checked(count)?;
    let length = length
        .iter()
        .fold(0usize, |length, byte| (length << 8) | usize::from(*byte));
    rest.get(..length)
}
