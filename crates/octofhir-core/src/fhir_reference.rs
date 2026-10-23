//! FHIR Reference parsing utilities.
//!
//! This module provides a unified implementation for parsing FHIR reference strings
//! into their component parts (resource type, ID, and optional version).
//!
//! # Reference Formats
//!
//! FHIR references can appear in several formats:
//! - Relative: `Patient/123`
//! - Versioned: `Patient/123/_history/1`
//! - Absolute URL: `http://example.org/fhir/Patient/123`
//! - Contained: `#contained-id` (cannot be resolved externally)
//! - URN: `urn:uuid:xxx` or `urn:oid:xxx` (cannot be resolved externally)
//!
//! # Example
//!
//! ```
//! use octofhir_core::fhir_reference::{parse_reference, FhirReference};
//!
//! // Parse a simple relative reference
//! let result = parse_reference("Patient/123", None);
//! assert!(result.is_ok());
//! let reference = result.unwrap();
//! assert_eq!(reference.resource_type, "Patient");
//! assert_eq!(reference.id, "123");
//!
//! // Parse a versioned reference
//! let result = parse_reference("Patient/123/_history/2", None);
//! assert!(result.is_ok());
//! let reference = result.unwrap();
//! assert_eq!(reference.version, Some("2".to_string()));
//! ```

use std::fmt;

/// A successfully parsed FHIR reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FhirReference {
    /// The resource type (e.g., "Patient", "Observation")
    pub resource_type: String,
    /// The resource ID
    pub id: String,
    /// Optional version ID from `_history` suffix
    pub version: Option<String>,
}

impl FhirReference {
    /// Creates a new FhirReference.
    pub fn new(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            id: id.into(),
            version: None,
        }
    }

    /// Creates a new FhirReference with a version.
    pub fn with_version(
        resource_type: impl Into<String>,
        id: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            resource_type: resource_type.into(),
            id: id.into(),
            version: Some(version.into()),
        }
    }

    /// Returns the reference as a relative string (Type/id).
    pub fn to_relative(&self) -> String {
        format!("{}/{}", self.resource_type, self.id)
    }

    /// Returns the reference with version if present (Type/id/_history/version).
    pub fn to_versioned(&self) -> String {
        match &self.version {
            Some(v) => format!("{}/{}/_history/{}", self.resource_type, self.id, v),
            None => self.to_relative(),
        }
    }
}

impl fmt::Display for FhirReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_relative())
    }
}

/// Represents a reference that cannot be resolved locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnresolvableReference {
    /// A contained reference (starts with `#`)
    Contained(String),
    /// A URN reference (`urn:uuid:xxx` or `urn:oid:xxx`)
    Urn(String),
    /// An external server reference (different base URL)
    External(String),
    /// A malformed or invalid reference
    Invalid(String),
}

impl fmt::Display for UnresolvableReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contained(id) => write!(f, "contained reference: #{id}"),
            Self::Urn(urn) => write!(f, "URN reference: {urn}"),
            Self::External(url) => write!(f, "external reference: {url}"),
            Self::Invalid(reason) => write!(f, "invalid reference: {reason}"),
        }
    }
}

impl std::error::Error for UnresolvableReference {}

/// Parse a FHIR reference string into its components.
///
/// # Arguments
///
/// * `reference` - The reference string to parse
/// * `base_url` - Optional base URL of the current server. If provided, absolute URLs
///   matching this base will be treated as local references.
///
/// # Returns
///
/// * `Ok(FhirReference)` - Successfully parsed local reference
/// * `Err(UnresolvableReference)` - Reference cannot be resolved locally
///
/// # Examples
///
/// ```
/// use octofhir_core::fhir_reference::parse_reference;
///
/// // Relative reference
/// let r = parse_reference("Patient/123", None).unwrap();
/// assert_eq!(r.resource_type, "Patient");
/// assert_eq!(r.id, "123");
///
/// // Absolute URL with matching base
/// let r = parse_reference(
///     "http://localhost/fhir/Patient/123",
///     Some("http://localhost/fhir")
/// ).unwrap();
/// assert_eq!(r.resource_type, "Patient");
/// assert_eq!(r.id, "123");
///
/// // Contained reference returns error
/// let err = parse_reference("#contained", None).unwrap_err();
/// assert!(matches!(err, octofhir_core::fhir_reference::UnresolvableReference::Contained(_)));
/// ```
pub fn parse_reference(
    reference: &str,
    base_url: Option<&str>,
) -> Result<FhirReference, UnresolvableReference> {
    // Handle empty/whitespace references
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(UnresolvableReference::Invalid(
            "empty reference".to_string(),
        ));
    }

    // Skip contained references (#id)
    if let Some(contained_id) = reference.strip_prefix('#') {
        if contained_id.is_empty() {
            return Err(UnresolvableReference::Invalid(
                "contained reference id cannot be empty".to_string(),
            ));
        }
        return Err(UnresolvableReference::Contained(contained_id.to_string()));
    }

    // Skip URN references (urn:uuid:xxx, urn:oid:xxx)
    if reference.starts_with("urn:") {
        return Err(UnresolvableReference::Urn(reference.to_string()));
    }

    // Match a complete service-base segment, never just a host/path prefix.
    // FHIR REST identity treats HTTP and HTTPS as the same resource identity
    // when the server, explicit port and path agree.
    let path = if reference.contains("://") {
        let Some(base) = base_url else {
            return Err(UnresolvableReference::External(reference.to_owned()));
        };
        let base = base.trim_end_matches('/');
        let prefix = format!("{base}/");
        let alternate = base
            .strip_prefix("http://")
            .map(|rest| format!("https://{rest}/"))
            .or_else(|| {
                base.strip_prefix("https://")
                    .map(|rest| format!("http://{rest}/"))
            });
        reference
            .strip_prefix(&prefix)
            .or_else(|| {
                alternate
                    .as_deref()
                    .and_then(|prefix| reference.strip_prefix(prefix))
            })
            .ok_or_else(|| UnresolvableReference::External(reference.to_owned()))?
    } else {
        reference
    };
    // REST identity ignores the query component; conditional references have
    // no Type/id path and remain outside this literal-reference parser.
    let path = path.split('?').next().unwrap_or(path);
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() != 2 && !(parts.len() == 4 && parts[2] == "_history") {
        return Err(UnresolvableReference::Invalid(format!(
            "expected Type/id or Type/id/_history/version: {reference}"
        )));
    }
    let resource_type = parts[0];
    let id = parts[1];
    // Include logical-model names while keeping the type a safe identifier.
    // In particular, untrusted Reference strings must not supply SQL syntax
    // when a storage resolver selects the corresponding resource table.
    if !resource_type
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_uppercase)
        || !resource_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(UnresolvableReference::Invalid(format!(
            "invalid resource type: {resource_type}"
        )));
    }
    crate::validate_id(id).map_err(|error| UnresolvableReference::Invalid(error.to_string()))?;
    let version = if parts.len() == 4 {
        crate::validate_id(parts[3])
            .map_err(|error| UnresolvableReference::Invalid(error.to_string()))?;
        Some(parts[3].to_owned())
    } else {
        None
    };

    Ok(FhirReference {
        resource_type: resource_type.to_string(),
        id: id.to_string(),
        version,
    })
}

/// Convenience function to extract just the resource type and ID as a tuple.
///
/// This is useful when you don't need the version information and want
/// a simple tuple for pattern matching.
pub fn parse_reference_simple(
    reference: &str,
    base_url: Option<&str>,
) -> Result<(String, String), UnresolvableReference> {
    let parsed = parse_reference(reference, base_url)?;
    Ok((parsed.resource_type, parsed.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_relative_reference() {
        let result = parse_reference("Patient/123", None);
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.resource_type, "Patient");
        assert_eq!(r.id, "123");
        assert_eq!(r.version, None);
    }

    #[test]
    fn test_versioned_reference() {
        let result = parse_reference("Patient/123/_history/2", None);
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.resource_type, "Patient");
        assert_eq!(r.id, "123");
        assert_eq!(r.version, Some("2".to_string()));
    }

    #[test]
    fn test_absolute_url_with_matching_base() {
        let result = parse_reference(
            "http://localhost:8888/fhir/Patient/123",
            Some("http://localhost:8888/fhir"),
        );
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.resource_type, "Patient");
        assert_eq!(r.id, "123");
    }

    #[test]
    fn test_absolute_url_with_trailing_slash_base() {
        let result = parse_reference(
            "http://localhost:8888/fhir/Patient/123",
            Some("http://localhost:8888/fhir/"),
        );
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.resource_type, "Patient");
        assert_eq!(r.id, "123");
    }

    #[test]
    fn test_absolute_url_without_base() {
        let result = parse_reference("http://localhost:8888/fhir/Patient/123", None);
        assert!(matches!(result, Err(UnresolvableReference::External(_))));
    }

    #[test]
    fn test_external_absolute_url() {
        let result = parse_reference(
            "http://other-server.com/fhir/Patient/123",
            Some("http://localhost:8888/fhir"),
        );
        assert!(matches!(result, Err(UnresolvableReference::External(_))));
    }

    #[test]
    fn test_contained_reference() {
        let result = parse_reference("#contained-id", None);
        assert!(
            matches!(result, Err(UnresolvableReference::Contained(id)) if id == "contained-id")
        );
    }

    #[test]
    fn test_invalid_empty_contained_reference() {
        let result = parse_reference("#", None);
        assert!(matches!(result, Err(UnresolvableReference::Invalid(_))));
    }

    #[test]
    fn test_urn_uuid_reference() {
        let result = parse_reference("urn:uuid:550e8400-e29b-41d4-a716-446655440000", None);
        assert!(matches!(result, Err(UnresolvableReference::Urn(_))));
    }

    #[test]
    fn test_urn_oid_reference() {
        let result = parse_reference("urn:oid:2.16.840.1.113883.4.642.3.1", None);
        assert!(matches!(result, Err(UnresolvableReference::Urn(_))));
    }

    #[test]
    fn test_invalid_lowercase_type() {
        let result = parse_reference("patient/123", None);
        assert!(matches!(result, Err(UnresolvableReference::Invalid(_))));
    }

    #[test]
    fn test_invalid_empty_id() {
        let result = parse_reference("Patient/", None);
        assert!(matches!(result, Err(UnresolvableReference::Invalid(_))));
    }

    #[test]
    fn test_invalid_no_slash() {
        let result = parse_reference("Patient123", None);
        assert!(matches!(result, Err(UnresolvableReference::Invalid(_))));
    }

    #[test]
    fn test_empty_reference() {
        let result = parse_reference("", None);
        assert!(matches!(result, Err(UnresolvableReference::Invalid(_))));
    }

    #[test]
    fn test_whitespace_reference() {
        let result = parse_reference("  ", None);
        assert!(matches!(result, Err(UnresolvableReference::Invalid(_))));
    }

    #[test]
    fn test_to_relative() {
        let r = FhirReference::new("Patient", "123");
        assert_eq!(r.to_relative(), "Patient/123");
    }

    #[test]
    fn test_to_versioned() {
        let r = FhirReference::with_version("Patient", "123", "2");
        assert_eq!(r.to_versioned(), "Patient/123/_history/2");
    }

    #[test]
    fn test_display() {
        let r = FhirReference::new("Patient", "123");
        assert_eq!(format!("{r}"), "Patient/123");
    }

    #[test]
    fn test_parse_reference_simple() {
        let result = parse_reference_simple("Patient/123", None);
        assert!(result.is_ok());
        let (rtype, id) = result.unwrap();
        assert_eq!(rtype, "Patient");
        assert_eq!(id, "123");
    }

    #[test]
    fn literal_references_preserve_valid_opaque_identity_and_reject_extra_paths() {
        for reference in ["Patient/.abc", "Patient/abc.", "Patient/./_history/.."] {
            assert!(parse_reference(reference, None).is_ok(), "{reference}");
        }
        let parsed = parse_reference("Patient/1/_history/01", None).unwrap();
        assert_eq!(parsed.version.as_deref(), Some("01"));
        for reference in [
            "Patient/1/extra",
            "Patient/1/_history",
            "Patient/1/_history/",
            "Patient/1/_history/2/extra",
            "Patient/1//2",
            "Patient/1#contained",
            "Patient/1%2F2",
            "Patient/1/_history/2%2F3",
            "Patient\"/1",
            "Patient;DROP/1",
            "Patient name/1",
        ] {
            assert!(
                matches!(
                    parse_reference(reference, None),
                    Err(UnresolvableReference::Invalid(_))
                ),
                "{reference}"
            );
        }
        assert!(parse_reference(&format!("Patient/{}", "x".repeat(65)), None).is_err());
        assert!(parse_reference(&format!("Patient/1/_history/{}", "x".repeat(65)), None).is_err());
    }

    #[test]
    fn absolute_references_require_the_complete_service_base() {
        let base = "http://example.test:8888/fhir";
        for reference in [
            "http://example.test:8888/fhir/Patient/p1",
            "https://example.test:8888/fhir/Patient/p1",
        ] {
            assert_eq!(
                parse_reference(reference, Some(base)).unwrap(),
                FhirReference::new("Patient", "p1")
            );
        }
        for reference in [
            "http://example.test:8888/fhir-evil/Patient/p1",
            "http://example.test:8888/fhirOther/Patient/p1",
            "http://example.test.evil:8888/fhir/Patient/p1",
            "http://example.test:9999/fhir/Patient/p1",
        ] {
            assert!(
                matches!(
                    parse_reference(reference, Some(base)),
                    Err(UnresolvableReference::External(_))
                ),
                "{reference}"
            );
        }
        assert!(parse_reference("http://example.test:8888/fhir//Patient/p1", Some(base)).is_err());
        let versioned = parse_reference(
            "https://example.test:8888/fhir/Patient/p1/_history/v1?_format=json",
            Some(base),
        )
        .unwrap();
        assert_eq!(versioned.to_versioned(), "Patient/p1/_history/v1");
    }
}
