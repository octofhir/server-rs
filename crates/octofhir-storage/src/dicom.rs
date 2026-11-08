//! Authoritative connector manifest recorded with its FHIR transaction.
//!
//! # Invariants
//! This is an internal storage capability, never a field accepted from FHIR JSON.
//! Only metadata identifiers and a digest are persisted; no DICOM payload is stored.

/// A connector-owned manifest and immutable patient binding.
pub struct DicomManifest {
    pub connection: String,
    pub study_uid: String,
    pub resource_id: String,
    pub patient_id: String,
    pub source_hash: String,
    pub manifest_hash: String,
}
