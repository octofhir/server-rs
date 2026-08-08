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
///
/// Uses UUID v7 (time-ordered). Resource tables store `id` as `TEXT PRIMARY KEY`,
/// and the v7 timestamp lives in the leading 48 bits — i.e. the first 12 hex
/// characters — so the canonical string sorts in creation order. Inserts land at
/// the right edge of the btree instead of scattering across it, which keeps page
/// splits and WAL churn down on bulk/bundle writes.
pub fn generate_id() -> String {
    uuid::Uuid::now_v7().to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_uuid_v7() {
        let id = generate_id();
        let parsed = uuid::Uuid::parse_str(&id).expect("generated id must be a valid UUID");
        assert_eq!(
            parsed.get_version_num(),
            7,
            "id must be UUID v7, got {}",
            id
        );
        validate_id(&id).expect("generated id must be a valid FHIR id");
    }

    #[test]
    fn generated_ids_sort_in_creation_order() {
        // The btree locality win depends on the canonical string being
        // lexicographically ordered by creation time, since resource ids are
        // stored as TEXT. Sleep past the v7 millisecond tick between samples.
        let mut prev = generate_id();
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            let next = generate_id();
            assert!(next > prev, "{next} should sort after {prev}");
            prev = next;
        }
    }
}
