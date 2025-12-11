//! FHIR custom scalar implementations for GraphQL.
//!
//! Each scalar type corresponds to a FHIR primitive type and includes:
//! - Input parsing with validation
//! - Output serialization
//! - Comprehensive error messages
//!
//! Reference: <https://www.hl7.org/fhir/datatypes.html#primitive>

use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};
use std::sync::LazyLock;

// =============================================================================
// Regex patterns for validation
// =============================================================================

/// FHIR instant regex: YYYY-MM-DDThh:mm:ss.sss+zz:zz
static INSTANT_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^([0-9]([0-9]([0-9][1-9]|[1-9]0)|[1-9]00)|[1-9]000)-(0[1-9]|1[0-2])-(0[1-9]|[1-2][0-9]|3[0-1])T([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)(\.[0-9]+)?(Z|(\+|-)((0[0-9]|1[0-3]):[0-5][0-9]|14:00))$"
    ).expect("Invalid instant regex")
});

/// FHIR dateTime regex: YYYY, YYYY-MM, YYYY-MM-DD or YYYY-MM-DDThh:mm:ss+zz:zz
static DATETIME_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^([0-9]([0-9]([0-9][1-9]|[1-9]0)|[1-9]00)|[1-9]000)(-(0[1-9]|1[0-2])(-(0[1-9]|[1-2][0-9]|3[0-1])(T([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)(\.[0-9]+)?(Z|(\+|-)((0[0-9]|1[0-3]):[0-5][0-9]|14:00)))?)?)?$"
    ).expect("Invalid dateTime regex")
});

/// FHIR date regex: YYYY, YYYY-MM, YYYY-MM-DD
static DATE_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^([0-9]([0-9]([0-9][1-9]|[1-9]0)|[1-9]00)|[1-9]000)(-(0[1-9]|1[0-2])(-(0[1-9]|[1-2][0-9]|3[0-1]))?)?$",
    )
    .expect("Invalid date regex")
});

/// FHIR time regex: hh:mm:ss.sss
static TIME_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)(\.[0-9]+)?$")
        .expect("Invalid time regex")
});

/// FHIR ID regex: [A-Za-z0-9\-\.]{1,64}
static ID_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[A-Za-z0-9\-\.]{1,64}$").expect("Invalid id regex"));

/// FHIR OID regex: urn:oid:[0-2](\.(0|[1-9][0-9]*))+
static OID_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^urn:oid:[0-2](\.(0|[1-9][0-9]*))+$").expect("Invalid oid regex")
});

/// FHIR UUID regex: urn:uuid:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}
static UUID_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^urn:uuid:[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )
    .expect("Invalid uuid regex")
});

// =============================================================================
// FhirInstant - xs:dateTime with timezone (required)
// =============================================================================

/// FHIR `instant` type - a timestamp with timezone.
///
/// Format: `YYYY-MM-DDThh:mm:ss.sss+zz:zz`
///
/// An instant in time, represented as a string with a mandatory timezone.
/// This is used for recording precisely when something happened.
///
/// # Examples
/// - `2024-01-15T10:30:00.000Z`
/// - `2024-01-15T10:30:00+01:00`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirInstant(pub String);

#[Scalar(name = "FhirInstant")]
impl ScalarType for FhirInstant {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                if INSTANT_REGEX.is_match(&s) {
                    Ok(FhirInstant(s))
                } else {
                    Err(InputValueError::custom(format!(
                        "Invalid FHIR instant format: '{}'. Expected format: YYYY-MM-DDThh:mm:ss.sss+zz:zz (timezone required)",
                        s
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirDateTime - partial date/time with optional timezone
// =============================================================================

/// FHIR `dateTime` type - a date, date-time, or partial date.
///
/// Format: `YYYY`, `YYYY-MM`, `YYYY-MM-DD`, or `YYYY-MM-DDThh:mm:ss+zz:zz`
///
/// A date, date-time, or partial date (e.g., just year or year + month).
/// If hours and minutes are specified, a timezone offset SHALL be populated.
///
/// # Examples
/// - `2024` (year only)
/// - `2024-01` (year and month)
/// - `2024-01-15` (full date)
/// - `2024-01-15T10:30:00Z` (date-time with timezone)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirDateTime(pub String);

#[Scalar(name = "FhirDateTime")]
impl ScalarType for FhirDateTime {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                if DATETIME_REGEX.is_match(&s) {
                    Ok(FhirDateTime(s))
                } else {
                    Err(InputValueError::custom(format!(
                        "Invalid FHIR dateTime format: '{}'. Expected: YYYY, YYYY-MM, YYYY-MM-DD, or YYYY-MM-DDThh:mm:ss+zz:zz",
                        s
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirDate - date without time
// =============================================================================

/// FHIR `date` type - a date or partial date.
///
/// Format: `YYYY`, `YYYY-MM`, or `YYYY-MM-DD`
///
/// A date or partial date (e.g., just year or year + month).
/// There is no time zone.
///
/// # Examples
/// - `2024` (year only)
/// - `2024-01` (year and month)
/// - `2024-01-15` (full date)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirDate(pub String);

#[Scalar(name = "FhirDate")]
impl ScalarType for FhirDate {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                if DATE_REGEX.is_match(&s) {
                    Ok(FhirDate(s))
                } else {
                    Err(InputValueError::custom(format!(
                        "Invalid FHIR date format: '{}'. Expected: YYYY, YYYY-MM, or YYYY-MM-DD",
                        s
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirTime - time of day without date
// =============================================================================

/// FHIR `time` type - a time of day.
///
/// Format: `hh:mm:ss` or `hh:mm:ss.sss`
///
/// A time during the day, with no date specified.
/// There is no timezone; the time zone is specified in the context.
///
/// # Examples
/// - `10:30:00`
/// - `10:30:00.000`
/// - `14:45:30.123`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirTime(pub String);

#[Scalar(name = "FhirTime")]
impl ScalarType for FhirTime {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                if TIME_REGEX.is_match(&s) {
                    Ok(FhirTime(s))
                } else {
                    Err(InputValueError::custom(format!(
                        "Invalid FHIR time format: '{}'. Expected: hh:mm:ss or hh:mm:ss.sss",
                        s
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirUri - Uniform Resource Identifier
// =============================================================================

/// FHIR `uri` type - a Uniform Resource Identifier.
///
/// A Uniform Resource Identifier (RFC 3986). URIs are case-sensitive.
/// URIs can be absolute or relative and may have an optional fragment.
///
/// # Examples
/// - `http://hl7.org/fhir/Patient`
/// - `urn:uuid:53fefa32-fcbb-4ff8-8a92-55ee120877b7`
/// - `#local-reference`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirUri(pub String);

#[Scalar(name = "FhirUri")]
impl ScalarType for FhirUri {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                // FHIR URIs are quite permissive, just ensure non-empty
                if s.is_empty() {
                    Err(InputValueError::custom("FHIR uri cannot be empty"))
                } else {
                    Ok(FhirUri(s))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirUrl - Uniform Resource Locator
// =============================================================================

/// FHIR `url` type - a Uniform Resource Locator.
///
/// A URL is a URI that must be resolvable to a network location.
/// This is more restrictive than `uri`.
///
/// # Examples
/// - `http://hl7.org/fhir/Patient`
/// - `https://example.com/api/Patient/123`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirUrl(pub String);

#[Scalar(name = "FhirUrl")]
impl ScalarType for FhirUrl {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                // Validate it's a proper URL
                match url::Url::parse(&s) {
                    Ok(_) => Ok(FhirUrl(s)),
                    Err(e) => Err(InputValueError::custom(format!(
                        "Invalid FHIR url: '{}'. {}",
                        s, e
                    ))),
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirCanonical - canonical URL reference
// =============================================================================

/// FHIR `canonical` type - a canonical URL reference.
///
/// A canonical URL reference to a FHIR resource by its canonical URL,
/// optionally with a version: `url|version`.
///
/// # Examples
/// - `http://hl7.org/fhir/StructureDefinition/Patient`
/// - `http://hl7.org/fhir/ValueSet/observation-codes|4.0.1`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirCanonical(pub String);

#[Scalar(name = "FhirCanonical")]
impl ScalarType for FhirCanonical {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                if s.is_empty() {
                    Err(InputValueError::custom("FHIR canonical cannot be empty"))
                } else {
                    // Split by | to check base URL
                    let base = s.split('|').next().unwrap_or(&s);
                    if url::Url::parse(base).is_ok() {
                        Ok(FhirCanonical(s))
                    } else {
                        Err(InputValueError::custom(format!(
                            "Invalid FHIR canonical URL: '{}'",
                            s
                        )))
                    }
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirOid - OID (object identifier)
// =============================================================================

/// FHIR `oid` type - an OID represented as a URI.
///
/// An OID represented as a URI (RFC 3001).
///
/// # Examples
/// - `urn:oid:1.2.3.4.5`
/// - `urn:oid:2.16.840.1.113883.6.96`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirOid(pub String);

#[Scalar(name = "FhirOid")]
impl ScalarType for FhirOid {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                if OID_REGEX.is_match(&s) {
                    Ok(FhirOid(s))
                } else {
                    Err(InputValueError::custom(format!(
                        "Invalid FHIR oid format: '{}'. Expected: urn:oid:[0-2](.[0-9]+)+",
                        s
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirUuid - UUID as a URI
// =============================================================================

/// FHIR `uuid` type - a UUID represented as a URI.
///
/// A UUID (RFC 4122) represented as a URI.
///
/// # Examples
/// - `urn:uuid:c757873d-ec9a-4326-a141-556f43239520`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirUuid(pub String);

#[Scalar(name = "FhirUuid")]
impl ScalarType for FhirUuid {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                if UUID_REGEX.is_match(&s) {
                    Ok(FhirUuid(s))
                } else {
                    Err(InputValueError::custom(format!(
                        "Invalid FHIR uuid format: '{}'. Expected: urn:uuid:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
                        s
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirId - FHIR resource ID
// =============================================================================

/// FHIR `id` type - a resource identifier.
///
/// Any combination of upper- or lower-case ASCII letters, numerals, '-' and '.',
/// with a length limit of 64 characters.
///
/// # Examples
/// - `patient-123`
/// - `abc.def`
/// - `A1-B2.C3`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirId(pub String);

#[Scalar(name = "FhirId")]
impl ScalarType for FhirId {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                if ID_REGEX.is_match(&s) {
                    Ok(FhirId(s))
                } else {
                    Err(InputValueError::custom(format!(
                        "Invalid FHIR id: '{}'. Must be 1-64 characters, containing only [A-Za-z0-9.-]",
                        s
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirBase64Binary - base64-encoded binary data
// =============================================================================

/// FHIR `base64Binary` type - base64-encoded binary data.
///
/// A stream of bytes, base64 encoded (RFC 4648).
///
/// # Examples
/// - `SGVsbG8gV29ybGQ=` (decodes to "Hello World")
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirBase64Binary(pub String);

#[Scalar(name = "FhirBase64Binary")]
impl ScalarType for FhirBase64Binary {
    fn parse(value: Value) -> InputValueResult<Self> {
        use base64::Engine;
        match value {
            Value::String(s) => {
                // Validate base64 by attempting to decode
                match base64::engine::general_purpose::STANDARD.decode(&s) {
                    Ok(_) => Ok(FhirBase64Binary(s)),
                    Err(e) => Err(InputValueError::custom(format!(
                        "Invalid base64 encoding: {}",
                        e
                    ))),
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirMarkdown - markdown-formatted text
// =============================================================================

/// FHIR `markdown` type - a string that may contain markdown.
///
/// A FHIR string that may contain markdown (GFM) formatting for
/// rendering as styled text.
///
/// # Examples
/// - `This is **bold** and *italic* text.`
/// - `# Heading\n\n- List item 1\n- List item 2`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirMarkdown(pub String);

#[Scalar(name = "FhirMarkdown")]
impl ScalarType for FhirMarkdown {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => Ok(FhirMarkdown(s)),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirPositiveInt - positive integer (> 0)
// =============================================================================

/// FHIR `positiveInt` type - a positive integer.
///
/// Any positive integer in the range 1 to 2,147,483,647.
///
/// # Examples
/// - `1`
/// - `42`
/// - `2147483647`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FhirPositiveInt(pub i32);

#[Scalar(name = "FhirPositiveInt")]
impl ScalarType for FhirPositiveInt {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::Number(n) => {
                let num = n
                    .as_i64()
                    .ok_or_else(|| InputValueError::custom("Expected integer for positiveInt"))?;

                if num > 0 && num <= i64::from(i32::MAX) {
                    Ok(FhirPositiveInt(num as i32))
                } else if num <= 0 {
                    Err(InputValueError::custom(format!(
                        "FHIR positiveInt must be > 0, got {}",
                        num
                    )))
                } else {
                    Err(InputValueError::custom(format!(
                        "FHIR positiveInt exceeds maximum value (2147483647), got {}",
                        num
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::Number(self.0.into())
    }
}

// =============================================================================
// FhirUnsignedInt - non-negative integer (>= 0)
// =============================================================================

/// FHIR `unsignedInt` type - a non-negative integer.
///
/// Any non-negative integer in the range 0 to 2,147,483,647.
///
/// # Examples
/// - `0`
/// - `1`
/// - `2147483647`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FhirUnsignedInt(pub i32);

#[Scalar(name = "FhirUnsignedInt")]
impl ScalarType for FhirUnsignedInt {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::Number(n) => {
                let num = n
                    .as_i64()
                    .ok_or_else(|| InputValueError::custom("Expected integer for unsignedInt"))?;

                if num >= 0 && num <= i64::from(i32::MAX) {
                    Ok(FhirUnsignedInt(num as i32))
                } else if num < 0 {
                    Err(InputValueError::custom(format!(
                        "FHIR unsignedInt must be >= 0, got {}",
                        num
                    )))
                } else {
                    Err(InputValueError::custom(format!(
                        "FHIR unsignedInt exceeds maximum value (2147483647), got {}",
                        num
                    )))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::Number(self.0.into())
    }
}

// =============================================================================
// FhirDecimal - arbitrary precision decimal
// =============================================================================

/// FHIR `decimal` type - a rational number with implicit precision.
///
/// A rational number with implicit precision. FHIR decimals are represented
/// as strings to preserve precision. The value may include a leading sign
/// and decimal point.
///
/// # Examples
/// - `3.14159`
/// - `-0.001`
/// - `100.00`
#[derive(Debug, Clone, PartialEq)]
pub struct FhirDecimal(pub String);

#[Scalar(name = "FhirDecimal")]
impl ScalarType for FhirDecimal {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                // Validate it parses as a decimal
                if s.parse::<f64>().is_ok() {
                    Ok(FhirDecimal(s))
                } else {
                    Err(InputValueError::custom(format!(
                        "Invalid FHIR decimal: '{}'",
                        s
                    )))
                }
            }
            Value::Number(n) => {
                // Accept numbers and convert to string (preserving representation)
                Ok(FhirDecimal(n.to_string()))
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        // Return as string to preserve precision
        Value::String(self.0.clone())
    }
}

// =============================================================================
// FhirXhtml - restricted XHTML content
// =============================================================================

/// FHIR `xhtml` type - restricted XHTML content for Narrative.
///
/// Limited xhtml content, defined by a set of rules in the FHIR specification.
/// This is used for the `text.div` element of resources.
///
/// # Examples
/// - `<div xmlns="http://www.w3.org/1999/xhtml"><p>Patient summary</p></div>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhirXhtml(pub String);

#[Scalar(name = "FhirXhtml")]
impl ScalarType for FhirXhtml {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                // Basic validation: must contain xhtml namespace
                if s.contains("http://www.w3.org/1999/xhtml") || s.is_empty() {
                    Ok(FhirXhtml(s))
                } else {
                    Err(InputValueError::custom(
                        "FHIR xhtml must contain the XHTML namespace (http://www.w3.org/1999/xhtml)",
                    ))
                }
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // FhirInstant tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_instant_valid() {
        let valid = vec![
            "2024-01-15T10:30:00Z",
            "2024-01-15T10:30:00.000Z",
            "2024-01-15T10:30:00.123Z",
            "2024-01-15T10:30:00+01:00",
            "2024-01-15T10:30:00-05:00",
            "2024-01-15T10:30:00.999+14:00",
        ];
        for s in valid {
            let result = FhirInstant::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid instant: {}", s);
        }
    }

    #[test]
    fn test_fhir_instant_invalid() {
        let invalid = vec![
            "2024-01-15",           // no time
            "2024-01-15T10:30:00",  // no timezone
            "2024-01-15T25:30:00Z", // invalid hour
            "2024-01-15T10:60:00Z", // invalid minute
            "not-a-date",           // not a date at all
        ];
        for s in invalid {
            let result = FhirInstant::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid instant: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirDateTime tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_datetime_valid() {
        let valid = vec![
            "2024",
            "2024-01",
            "2024-01-15",
            "2024-01-15T10:30:00Z",
            "2024-01-15T10:30:00.000+01:00",
        ];
        for s in valid {
            let result = FhirDateTime::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid dateTime: {}", s);
        }
    }

    #[test]
    fn test_fhir_datetime_invalid() {
        let invalid = vec![
            "24",                  // year too short
            "2024-13",             // invalid month
            "2024-01-32",          // invalid day
            "2024-01-15T10:30:00", // time without timezone
        ];
        for s in invalid {
            let result = FhirDateTime::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid dateTime: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirDate tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_date_valid() {
        let valid = vec!["2024", "2024-01", "2024-01-15", "1999-12-31"];
        for s in valid {
            let result = FhirDate::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid date: {}", s);
        }
    }

    #[test]
    fn test_fhir_date_invalid() {
        let invalid = vec![
            "24",
            "2024-13",
            "2024-01-32",
            "2024-01-15T10:30:00Z", // contains time
        ];
        for s in invalid {
            let result = FhirDate::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid date: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirTime tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_time_valid() {
        let valid = vec!["10:30:00", "00:00:00", "23:59:59", "10:30:00.123"];
        for s in valid {
            let result = FhirTime::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid time: {}", s);
        }
    }

    #[test]
    fn test_fhir_time_invalid() {
        let invalid = vec!["25:00:00", "10:60:00", "10:30", "10:30:00Z"];
        for s in invalid {
            let result = FhirTime::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid time: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirId tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_id_valid() {
        let valid = vec!["patient-123", "abc.def", "A1-B2.C3", "x"];
        for s in valid {
            let result = FhirId::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid id: {}", s);
        }
    }

    #[test]
    fn test_fhir_id_invalid() {
        let too_long = "x".repeat(65);
        let invalid: Vec<&str> = vec![
            "",            // empty
            "patient_123", // underscore not allowed
            "patient/123", // slash not allowed
            &too_long,     // too long
        ];
        for s in invalid {
            let result = FhirId::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid id: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirOid tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_oid_valid() {
        let valid = vec!["urn:oid:1.2.3", "urn:oid:2.16.840.1.113883.6.96"];
        for s in valid {
            let result = FhirOid::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid oid: {}", s);
        }
    }

    #[test]
    fn test_fhir_oid_invalid() {
        let invalid = vec!["1.2.3", "urn:oid:3.1.2", "urn:uuid:1.2.3"];
        for s in invalid {
            let result = FhirOid::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid oid: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirUuid tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_uuid_valid() {
        let valid = vec!["urn:uuid:c757873d-ec9a-4326-a141-556f43239520"];
        for s in valid {
            let result = FhirUuid::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid uuid: {}", s);
        }
    }

    #[test]
    fn test_fhir_uuid_invalid() {
        let invalid = vec![
            "c757873d-ec9a-4326-a141-556f43239520", // missing urn:uuid:
            "urn:uuid:not-a-uuid",
        ];
        for s in invalid {
            let result = FhirUuid::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid uuid: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirPositiveInt tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_positive_int_valid() {
        let valid = vec![1i64, 42, 2147483647];
        for n in valid {
            let result = FhirPositiveInt::parse(Value::Number(n.into()));
            assert!(result.is_ok(), "Expected valid positiveInt: {}", n);
        }
    }

    #[test]
    fn test_fhir_positive_int_invalid() {
        let invalid = vec![0i64, -1, -100];
        for n in invalid {
            let result = FhirPositiveInt::parse(Value::Number(n.into()));
            assert!(result.is_err(), "Expected invalid positiveInt: {}", n);
        }
    }

    // -------------------------------------------------------------------------
    // FhirUnsignedInt tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_unsigned_int_valid() {
        let valid = vec![0i64, 1, 42, 2147483647];
        for n in valid {
            let result = FhirUnsignedInt::parse(Value::Number(n.into()));
            assert!(result.is_ok(), "Expected valid unsignedInt: {}", n);
        }
    }

    #[test]
    fn test_fhir_unsigned_int_invalid() {
        let invalid = vec![-1i64, -100];
        for n in invalid {
            let result = FhirUnsignedInt::parse(Value::Number(n.into()));
            assert!(result.is_err(), "Expected invalid unsignedInt: {}", n);
        }
    }

    // -------------------------------------------------------------------------
    // FhirDecimal tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_decimal_valid() {
        // String form
        let valid_strings = vec!["3.14159", "-0.001", "100.00", "0", "1e10"];
        for s in valid_strings {
            let result = FhirDecimal::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid decimal string: {}", s);
        }

        // Integer number form (floats don't implement Into<Number> directly)
        let result = FhirDecimal::parse(Value::Number(314i64.into()));
        assert!(result.is_ok(), "Expected valid decimal number");
    }

    #[test]
    fn test_fhir_decimal_invalid() {
        let invalid = vec!["not-a-number", "abc123"];
        for s in invalid {
            let result = FhirDecimal::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid decimal: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirBase64Binary tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_base64_valid() {
        let valid = vec![
            "SGVsbG8gV29ybGQ=", // "Hello World"
            "dGVzdA==",         // "test"
            "",                 // empty is valid
        ];
        for s in valid {
            let result = FhirBase64Binary::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid base64: {}", s);
        }
    }

    #[test]
    fn test_fhir_base64_invalid() {
        let invalid = vec!["!!invalid!!", "not base64 at all!!!"];
        for s in invalid {
            let result = FhirBase64Binary::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid base64: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirUrl tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_url_valid() {
        let valid = vec![
            "http://example.com",
            "https://hl7.org/fhir/Patient",
            "ftp://files.example.com/data",
        ];
        for s in valid {
            let result = FhirUrl::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid url: {}", s);
        }
    }

    #[test]
    fn test_fhir_url_invalid() {
        let invalid = vec!["not a url", "#fragment-only", "relative/path"];
        for s in invalid {
            let result = FhirUrl::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid url: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // FhirXhtml tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_xhtml_valid() {
        let valid = vec![
            r#"<div xmlns="http://www.w3.org/1999/xhtml"><p>Hello</p></div>"#,
            "", // empty is valid
        ];
        for s in valid {
            let result = FhirXhtml::parse(Value::String(s.to_string()));
            assert!(result.is_ok(), "Expected valid xhtml: {}", s);
        }
    }

    #[test]
    fn test_fhir_xhtml_invalid() {
        let invalid = vec!["<div><p>No namespace</p></div>"];
        for s in invalid {
            let result = FhirXhtml::parse(Value::String(s.to_string()));
            assert!(result.is_err(), "Expected invalid xhtml: {}", s);
        }
    }

    // -------------------------------------------------------------------------
    // Output serialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_scalar_to_value() {
        assert_eq!(
            FhirInstant("2024-01-15T10:30:00Z".to_string()).to_value(),
            Value::String("2024-01-15T10:30:00Z".to_string())
        );
        assert_eq!(FhirPositiveInt(42).to_value(), Value::Number(42.into()));
        assert_eq!(
            FhirDecimal("3.14159".to_string()).to_value(),
            Value::String("3.14159".to_string())
        );
    }
}
