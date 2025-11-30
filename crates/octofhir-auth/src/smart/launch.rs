//! SMART on FHIR launch context storage.
//!
//! This module provides types for storing and managing SMART launch contexts
//! from EHR launches. Launch contexts are short-lived (typically 10 minutes)
//! and keyed by the opaque `launch_id` parameter provided by the EHR.
//!
//! # Flow
//!
//! 1. EHR creates launch context and sends `launch=<launch_id>` to app
//! 2. App includes `launch` parameter in authorization request
//! 3. Server retrieves launch context by `launch_id`
//! 4. Launch context is consumed during token exchange
//!
//! # Distinction from Session LaunchContext
//!
//! This `StoredLaunchContext` is temporary storage with independent TTL,
//! separate from the `LaunchContext` embedded in authorization sessions
//! (`oauth/session.rs`).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

// ============================================================================
// Launch Context Types
// ============================================================================

/// SMART launch context stored temporarily during EHR launch flow.
///
/// Contains clinical context (patient, encounter, etc.) from the EHR
/// that should be passed to the launched app.
///
/// # Lifecycle
///
/// - Created when EHR initiates launch
/// - Retrieved when app begins authorization
/// - Consumed (deleted) during token exchange
/// - Automatically cleaned up after TTL expires
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredLaunchContext {
    /// Opaque launch parameter from EHR.
    /// This is the key used to store/retrieve the context.
    pub launch_id: String,

    /// Current patient ID (FHIR resource ID).
    /// Included in token response as `patient` claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient: Option<String>,

    /// Current encounter ID (FHIR resource ID).
    /// Included in token response as `encounter` claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encounter: Option<String>,

    /// Additional FHIR context items.
    /// Included in token response as `fhirContext` claim (SMART v2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fhir_context: Vec<FhirContextItem>,

    /// Whether to display patient banner in the app.
    /// When true, app should show patient demographics.
    #[serde(default)]
    pub need_patient_banner: bool,

    /// URL to SMART styling information.
    /// Apps can use this to match the EHR's look and feel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smart_style_url: Option<String>,

    /// Launch intent describing why the app was launched.
    /// Examples: "reconcile-medications", "review-results"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,

    /// Tenant identifier for multi-tenant systems.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

impl StoredLaunchContext {
    /// Creates a new launch context with a patient.
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::launch::StoredLaunchContext;
    ///
    /// let ctx = StoredLaunchContext::with_patient("launch123", "patient456");
    /// assert_eq!(ctx.launch_id, "launch123");
    /// assert_eq!(ctx.patient, Some("patient456".to_string()));
    /// assert!(ctx.need_patient_banner);
    /// ```
    #[must_use]
    pub fn with_patient(launch_id: impl Into<String>, patient_id: impl Into<String>) -> Self {
        Self {
            launch_id: launch_id.into(),
            patient: Some(patient_id.into()),
            encounter: None,
            fhir_context: Vec::new(),
            need_patient_banner: true,
            smart_style_url: None,
            intent: None,
            tenant: None,
        }
    }

    /// Creates a new launch context with patient and encounter.
    ///
    /// Automatically adds the encounter to `fhir_context` with role "current-encounter".
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::launch::StoredLaunchContext;
    ///
    /// let ctx = StoredLaunchContext::with_patient_and_encounter(
    ///     "launch123",
    ///     "patient456",
    ///     "encounter789",
    /// );
    /// assert_eq!(ctx.encounter, Some("encounter789".to_string()));
    /// assert_eq!(ctx.fhir_context.len(), 1);
    /// assert_eq!(ctx.fhir_context[0].reference, "Encounter/encounter789");
    /// ```
    #[must_use]
    pub fn with_patient_and_encounter(
        launch_id: impl Into<String>,
        patient_id: impl Into<String>,
        encounter_id: impl Into<String>,
    ) -> Self {
        let encounter_id = encounter_id.into();
        Self {
            launch_id: launch_id.into(),
            patient: Some(patient_id.into()),
            encounter: Some(encounter_id.clone()),
            fhir_context: vec![FhirContextItem {
                reference: format!("Encounter/{}", encounter_id),
                role: Some("current-encounter".to_string()),
            }],
            need_patient_banner: true,
            smart_style_url: None,
            intent: None,
            tenant: None,
        }
    }

    /// Adds additional FHIR context to the launch context.
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::launch::StoredLaunchContext;
    ///
    /// let mut ctx = StoredLaunchContext::with_patient("launch123", "patient456");
    /// ctx.add_context("Observation/obs123".to_string(), Some("launch-context".to_string()));
    /// assert_eq!(ctx.fhir_context.len(), 1);
    /// ```
    pub fn add_context(&mut self, reference: String, role: Option<String>) {
        self.fhir_context.push(FhirContextItem { reference, role });
    }

    /// Returns true if this is an empty launch context (no patient or context).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patient.is_none()
            && self.encounter.is_none()
            && self.fhir_context.is_empty()
            && self.intent.is_none()
    }
}

/// A FHIR context item in the launch context.
///
/// Represents a FHIR resource reference with an optional role
/// describing how the resource relates to the launch context.
///
/// # SMART v2 fhirContext
///
/// This corresponds to items in the `fhirContext` array in SMART v2
/// token responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FhirContextItem {
    /// FHIR resource reference (e.g., "Patient/123", "Encounter/456").
    pub reference: String,

    /// Optional role describing the resource's relationship.
    /// Examples: "current-patient", "current-encounter", "launch-context"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl FhirContextItem {
    /// Creates a new FHIR context item.
    #[must_use]
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
            role: None,
        }
    }

    /// Creates a new FHIR context item with a role.
    #[must_use]
    pub fn with_role(reference: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
            role: Some(role.into()),
        }
    }
}

// ============================================================================
// Launch ID Generation
// ============================================================================

/// Default TTL for launch contexts in seconds (10 minutes).
pub const DEFAULT_LAUNCH_CONTEXT_TTL: u64 = 600;

/// Generates a cryptographically secure launch ID.
///
/// The ID is 256 bits (32 bytes) of random data, encoded as
/// base64url without padding (43 characters).
///
/// # Security
///
/// Uses the system's cryptographically secure random number generator.
/// The resulting ID has 256 bits of entropy, making it practically
/// impossible to guess.
///
/// # Examples
///
/// ```
/// use octofhir_auth::smart::launch::generate_launch_id;
///
/// let id1 = generate_launch_id();
/// let id2 = generate_launch_id();
///
/// assert_eq!(id1.len(), 43); // 32 bytes base64url encoded
/// assert_ne!(id1, id2);      // IDs are unique
/// ```
#[must_use]
pub fn generate_launch_id() -> String {
    let mut bytes = [0u8; 32];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launch_context_creation() {
        let ctx = StoredLaunchContext::with_patient("launch123", "patient456");
        assert_eq!(ctx.launch_id, "launch123");
        assert_eq!(ctx.patient, Some("patient456".to_string()));
        assert!(ctx.need_patient_banner);
        assert!(ctx.encounter.is_none());
        assert!(ctx.fhir_context.is_empty());
    }

    #[test]
    fn test_launch_context_with_encounter() {
        let ctx = StoredLaunchContext::with_patient_and_encounter(
            "launch123",
            "patient456",
            "encounter789",
        );
        assert_eq!(ctx.launch_id, "launch123");
        assert_eq!(ctx.patient, Some("patient456".to_string()));
        assert_eq!(ctx.encounter, Some("encounter789".to_string()));
        assert_eq!(ctx.fhir_context.len(), 1);
        assert_eq!(ctx.fhir_context[0].reference, "Encounter/encounter789");
        assert_eq!(
            ctx.fhir_context[0].role,
            Some("current-encounter".to_string())
        );
    }

    #[test]
    fn test_add_context() {
        let mut ctx = StoredLaunchContext::with_patient("launch123", "patient456");
        ctx.add_context(
            "Observation/obs123".to_string(),
            Some("launch-context".to_string()),
        );
        assert_eq!(ctx.fhir_context.len(), 1);
        assert_eq!(ctx.fhir_context[0].reference, "Observation/obs123");
        assert_eq!(ctx.fhir_context[0].role, Some("launch-context".to_string()));
    }

    #[test]
    fn test_launch_context_is_empty() {
        let empty = StoredLaunchContext::default();
        assert!(empty.is_empty());

        let with_patient = StoredLaunchContext::with_patient("launch123", "patient456");
        assert!(!with_patient.is_empty());
    }

    #[test]
    fn test_launch_id_generation() {
        let id1 = generate_launch_id();
        let id2 = generate_launch_id();

        // IDs should be unique
        assert_ne!(id1, id2);

        // 32 bytes = 256 bits, base64url encoded = 43 characters (no padding)
        assert_eq!(id1.len(), 43);
        assert_eq!(id2.len(), 43);

        // Should only contain URL-safe base64 characters
        assert!(
            id1.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn test_launch_id_uniqueness() {
        let ids: Vec<String> = (0..100).map(|_| generate_launch_id()).collect();

        // All IDs should be unique
        let mut unique_ids = ids.clone();
        unique_ids.sort();
        unique_ids.dedup();
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn test_launch_context_serialization() {
        let ctx = StoredLaunchContext::with_patient("launch123", "patient456");
        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: StoredLaunchContext = serde_json::from_str(&json).unwrap();

        assert_eq!(ctx.launch_id, deserialized.launch_id);
        assert_eq!(ctx.patient, deserialized.patient);
        assert_eq!(ctx.need_patient_banner, deserialized.need_patient_banner);
    }

    #[test]
    fn test_launch_context_full_serialization() {
        let mut ctx = StoredLaunchContext::with_patient_and_encounter(
            "launch123",
            "patient456",
            "encounter789",
        );
        ctx.smart_style_url = Some("https://ehr.example.com/smart-style.json".to_string());
        ctx.intent = Some("reconcile-medications".to_string());
        ctx.tenant = Some("tenant-abc".to_string());

        let json = serde_json::to_string_pretty(&ctx).unwrap();
        let deserialized: StoredLaunchContext = serde_json::from_str(&json).unwrap();

        assert_eq!(ctx.launch_id, deserialized.launch_id);
        assert_eq!(ctx.patient, deserialized.patient);
        assert_eq!(ctx.encounter, deserialized.encounter);
        assert_eq!(ctx.fhir_context.len(), deserialized.fhir_context.len());
        assert_eq!(ctx.smart_style_url, deserialized.smart_style_url);
        assert_eq!(ctx.intent, deserialized.intent);
        assert_eq!(ctx.tenant, deserialized.tenant);
    }

    #[test]
    fn test_fhir_context_item() {
        let item = FhirContextItem::new("Patient/123");
        assert_eq!(item.reference, "Patient/123");
        assert!(item.role.is_none());

        let item = FhirContextItem::with_role("Encounter/456", "current-encounter");
        assert_eq!(item.reference, "Encounter/456");
        assert_eq!(item.role, Some("current-encounter".to_string()));
    }

    #[test]
    fn test_fhir_context_item_serialization() {
        let item = FhirContextItem::with_role("Encounter/456", "current-encounter");
        let json = serde_json::to_string(&item).unwrap();
        let deserialized: FhirContextItem = serde_json::from_str(&json).unwrap();

        assert_eq!(item.reference, deserialized.reference);
        assert_eq!(item.role, deserialized.role);
    }
}
