// ID handling for FHIR resources
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdError {
    #[error("Invalid ID: {0}")]
    Invalid(String),

    #[error("ID too long (max 64 characters): {0}")]
    TooLong(usize),

    #[error("ID too short (min 1 character)")]
    TooShort,
}

/// Generates a new UUID-based ID for a FHIR resource.
///
/// This is the default ID generation strategy when users don't provide an ID.
pub fn generate_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Validates a FHIR resource ID according to the FHIR specification.
///
/// FHIR IDs must:
/// - Be between 1 and 64 characters
/// - Contain only: A-Z, a-z, 0-9, -, .
/// - Not start or end with a period
///
/// See: http://hl7.org/fhir/datatypes.html#id
pub fn validate_id(id: &str) -> Result<(), IdError> {
    // Check length constraints
    if id.is_empty() {
        return Err(IdError::TooShort);
    }

    if id.len() > 64 {
        return Err(IdError::TooLong(id.len()));
    }

    // FHIR spec: Any combination of upper- or lower-case ASCII letters ('A'..'Z', and 'a'..'z', numerals ('0'..'9'),
    // '-' and '.', with a length limit of 64 characters. (This might be an integer, an un-prefixed OID, UUID or any other identifier pattern that meets these constraints.)
    for (idx, ch) in id.chars().enumerate() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' => {
                // These are always allowed
            }
            '.' => {
                // Period is allowed but not at start or end
                if idx == 0 || idx == id.len() - 1 {
                    return Err(IdError::Invalid(
                        "ID cannot start or end with a period".to_string(),
                    ));
                }
            }
            _ => {
                return Err(IdError::Invalid(format!(
                    "Invalid character '{}' at position {}. Only A-Z, a-z, 0-9, -, and . are allowed",
                    ch, idx
                )));
            }
        }
    }

    Ok(())
}
