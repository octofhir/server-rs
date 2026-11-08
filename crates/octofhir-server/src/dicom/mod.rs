//! Built-in, vendor-independent DICOMweb integration.
//!
//! # Contents
//! Configuration, bounded WADO-RS metadata retrieval and release-aware FHIR mapping.
//! The patient-scoped import operation lives in `operations::dicom`.
//!
//! # Invariants
//! PACS owns DICOM objects. Pixel data is never persisted here; an opt-in gateway
//! streams authorized instances directly to callers.
//! Patient identity is checked using an explicitly configured assigning authority.
//! FHIR writes use the caller's existing authorization and transaction pipeline.
//!
//! # See also
//! `docs/src/content/docs/dicom.mdx` and `plans/dicom-integration.md`.

pub(crate) mod attestation;
pub(crate) mod client;
mod config;
pub(crate) mod gateway;
pub(crate) mod import;
pub use gateway::DicomGatewayConfig;
pub(crate) mod mapping;
pub(crate) mod security;
pub(crate) mod sync;
pub(crate) mod ups;
pub use sync::{DicomSyncConfig, DiscoveryMode};
pub use ups::{DicomUpsConfig, ResultKind, ResultMapping};

pub use config::{DicomConfig, DicomConnection};

/// A metadata/protocol failure; diagnostics never include remote bodies or credentials.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DicomError {
    #[error("Invalid DICOM metadata: {0}")]
    Metadata(String),
    #[error("DICOMweb request failed")]
    Transport,
    #[error("DICOM study metadata is incomplete; retry later")]
    Partial,
    #[error("DICOMweb returned HTTP {0}")]
    Status(u16),
    #[error("DICOMweb metadata exceeds configured limits")]
    Limit,
    #[error("DICOMweb configuration error: {0}")]
    Configuration(String),
}

/// UIDs are opaque dot-separated decimal components, up to 64 characters.
pub(crate) fn validate_uid(uid: &str) -> Result<(), DicomError> {
    if uid.is_empty()
        || uid.len() > 64
        || uid.split('.').any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return Err(DicomError::Metadata("invalid UID".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uids_cannot_change_request_paths() {
        for uid in ["", "1..2", "1.02", "1/2", "../metadata", "1?x=2", "1.2."] {
            assert!(validate_uid(uid).is_err(), "{uid}");
        }
        assert!(validate_uid(&"1".repeat(65)).is_err());
        assert!(validate_uid("1.2.840.10008.5.1.4.1.1.2").is_ok());
    }
}
