//! UDAP Security for FHIR (FAST Security STU2, `hl7.fhir.us.udap-security` 2.0.0).
//!
//! Applications in a trust community hold a certificate instead of a shared secret. They
//! register themselves with a signed software statement and sign in with a JWT carrying the
//! certificate chain, so this server trusts community anchors rather than individual clients:
//!
//! - [`trust`] validates such a JWT: signature, certificate chain, Subject Alternative Name.
//! - [`statement`] holds the rules of a software statement at registration.
//! - [`assertion`] authenticates a registered client at the token endpoint.
//! - [`metadata`] builds the signed `.well-known/udap` document.

pub mod assertion;
pub mod metadata;
pub mod ocsp;
pub mod revocation;
pub mod statement;
pub mod tiered;
pub mod trust;

pub use metadata::UdapMetadata;
pub use trust::{Trust, TrustError, Verified};

/// `alg` values accepted on JWTs and declared in the metadata. RS256 is mandatory, ES256
/// recommended, and the two others optional.
pub const SIGNING_ALGORITHMS: [&str; 4] = ["RS256", "RS384", "ES256", "ES384"];

/// Authentication tokens and software statements live at most this long.
pub const MAX_TOKEN_LIFETIME_SECONDS: i64 = 300;

/// Clock difference allowed on `iat` and `exp`.
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 30;

/// Access tokens issued to these clients live at most this long.
pub const MAX_ACCESS_TOKEN_LIFETIME_SECONDS: i64 = 3600;

/// The extension client credentials requests have to carry.
pub const B2B_EXTENSION: &str = "hl7-b2b";
