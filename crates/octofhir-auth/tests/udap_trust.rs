//! Certificate paths a UDAP JWT can come with.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use octofhir_auth::config::{UdapCommunityConfig, UdapConfig};
use octofhir_auth::udap::{Trust, TrustError};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, CertificateRevocationListParams, DnType,
    IsCa, Issuer, KeyIdMethod, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, RevocationReason,
    RevokedCertParams, SanType, SerialNumber,
};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

const APP: &str = "https://app.example.org/";
const AUDIENCE: &str = "https://server.example.org/auth/token";

fn key() -> KeyPair {
    KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap()
}

struct Authority {
    issuer: Issuer<'static, KeyPair>,
    pem: String,
}

fn authority(name: &str) -> Authority {
    let key = key();
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, name);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let certificate = params.self_signed(&key).unwrap();
    Authority {
        pem: certificate.pem(),
        issuer: Issuer::new(params, key),
    }
}

fn params(serial: u64) -> CertificateParams {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, "app");
    params.subject_alt_names = vec![SanType::URI(APP.try_into().unwrap())];
    params.serial_number = Some(SerialNumber::from(serial));
    params
}

fn intermediate(root: &Authority) -> (Authority, Certificate) {
    let key = key();
    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "Intermediate");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let certificate = params.signed_by(&key, &root.issuer).unwrap();
    (
        Authority {
            pem: certificate.pem(),
            issuer: Issuer::new(params, key),
        },
        certificate,
    )
}

fn sign(key: &KeyPair, chain: &[&Certificate]) -> String {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let mut header = Header::new(Algorithm::ES256);
    header.x5c = Some(
        chain
            .iter()
            .map(|certificate| STANDARD.encode(certificate.der()))
            .collect(),
    );
    let claims = json!({"iss": APP, "sub": APP, "aud": AUDIENCE, "exp": now + 200});
    let pem = key.serialize_pem();
    encode(
        &header,
        &claims,
        &EncodingKey::from_ec_pem(pem.as_bytes()).unwrap(),
    )
    .unwrap()
}

fn write(directory: &Path, name: &str, content: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn trust(directory: &Path, anchor: &Authority, lists: &[PathBuf]) -> Trust {
    let anchors = vec![write(directory, "anchor.pem", &anchor.pem)];
    Trust::from_config(&UdapConfig {
        communities: vec![UdapCommunityConfig {
            uri: "urn:example:community".to_owned(),
            anchors,
            revocation_lists: lists.to_vec(),
        }],
        ..UdapConfig::default()
    })
    .unwrap()
}

fn verify(trust: &Trust, token: &str) -> Result<Vec<String>, TrustError> {
    trust
        .verify::<Value>(token, AUDIENCE)
        .map(|verified| verified.subject_uris)
}

#[test]
fn a_chain_through_an_intermediate_leads_to_the_anchor() {
    let directory = tempfile::tempdir().unwrap();
    let root = authority("Root");
    let (middle, middle_certificate) = intermediate(&root);
    let key = key();
    let leaf = params(1).signed_by(&key, &middle.issuer).unwrap();
    let trust = trust(directory.path(), &root, &[]);

    assert_eq!(
        verify(&trust, &sign(&key, &[&leaf, &middle_certificate])).unwrap(),
        [APP]
    );
    // Without the intermediate there is no path.
    assert!(matches!(
        verify(&trust, &sign(&key, &[&leaf])),
        Err(TrustError::Untrusted(_))
    ));
}

#[test]
fn another_anchor_is_not_trusted() {
    let directory = tempfile::tempdir().unwrap();
    let key = key();
    let leaf = params(1)
        .signed_by(&key, &authority("Other").issuer)
        .unwrap();
    let trust = trust(directory.path(), &authority("Root"), &[]);
    assert!(matches!(
        verify(&trust, &sign(&key, &[&leaf])),
        Err(TrustError::Untrusted(_))
    ));
}

#[test]
fn an_expired_certificate_is_not_trusted() {
    let directory = tempfile::tempdir().unwrap();
    let root = authority("Root");
    let key = key();
    let mut expired = params(1);
    let now = OffsetDateTime::now_utc();
    expired.not_before = now - Duration::days(30);
    expired.not_after = now - Duration::days(1);
    let leaf = expired.signed_by(&key, &root.issuer).unwrap();
    let trust = trust(directory.path(), &root, &[]);
    assert!(matches!(
        verify(&trust, &sign(&key, &[&leaf])),
        Err(TrustError::Untrusted(_))
    ));
}

#[test]
fn a_signature_by_another_key_does_not_verify() {
    let directory = tempfile::tempdir().unwrap();
    let root = authority("Root");
    let leaf = params(1).signed_by(&key(), &root.issuer).unwrap();
    let trust = trust(directory.path(), &root, &[]);
    assert!(matches!(
        verify(&trust, &sign(&key(), &[&leaf])),
        Err(TrustError::Signature(_))
    ));
}

fn revocation_list(root: &Authority, serial: u64, next_update: OffsetDateTime) -> String {
    let now = OffsetDateTime::now_utc();
    CertificateRevocationListParams {
        this_update: now - Duration::days(2),
        next_update,
        crl_number: SerialNumber::from(1u64),
        issuing_distribution_point: None,
        revoked_certs: vec![RevokedCertParams {
            serial_number: SerialNumber::from(serial),
            revocation_time: now - Duration::days(1),
            reason_code: Some(RevocationReason::KeyCompromise),
            invalidity_date: None,
        }],
        key_identifier_method: KeyIdMethod::Sha256,
    }
    .signed_by(&root.issuer)
    .unwrap()
    .pem()
    .unwrap()
}

#[test]
fn a_revoked_certificate_is_not_trusted() {
    let directory = tempfile::tempdir().unwrap();
    let root = authority("Root");
    let next_update = OffsetDateTime::now_utc() + Duration::days(7);
    let list = write(
        directory.path(),
        "revoked.crl",
        &revocation_list(&root, 7, next_update),
    );
    let trust = trust(directory.path(), &root, &[list]);

    let key = key();
    let revoked = params(7).signed_by(&key, &root.issuer).unwrap();
    assert!(matches!(
        verify(&trust, &sign(&key, &[&revoked])),
        Err(TrustError::Untrusted(_))
    ));
    let good = params(8).signed_by(&key, &root.issuer).unwrap();
    assert!(verify(&trust, &sign(&key, &[&good])).is_ok());
}

#[test]
fn an_expired_revocation_list_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let root = authority("Root");
    let next_update = OffsetDateTime::now_utc() - Duration::hours(1);
    let list = write(
        directory.path(),
        "stale.crl",
        &revocation_list(&root, 7, next_update),
    );
    let trust = trust(directory.path(), &root, &[list]);
    let key = key();
    let good = params(8).signed_by(&key, &root.issuer).unwrap();
    assert!(matches!(
        verify(&trust, &sign(&key, &[&good])),
        Err(TrustError::Untrusted(_))
    ));
}

#[test]
fn a_jwt_without_x5c_or_with_a_symmetric_algorithm_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let trust = trust(directory.path(), &authority("Root"), &[]);
    let claims = json!({"iss": APP, "sub": APP, "aud": AUDIENCE, "exp": 4_000_000_000i64});
    let secret = EncodingKey::from_secret(b"secret");
    let symmetric = encode(&Header::new(Algorithm::HS256), &claims, &secret).unwrap();
    assert!(matches!(
        verify(&trust, &symmetric),
        Err(TrustError::Malformed(_))
    ));
    let key = key();
    let pem = key.serialize_pem();
    let bare = encode(
        &Header::new(Algorithm::ES256),
        &claims,
        &EncodingKey::from_ec_pem(pem.as_bytes()).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        verify(&trust, &bare),
        Err(TrustError::Malformed(_))
    ));
}
