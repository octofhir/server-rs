//! Text normalization utilities for FHIR search semantics.

/// Normalize a string for FHIR R4 search semantics.
///
/// Per FHIR R4 §3.1.1.5.6 (search.html#string), string parameter matching is
/// "by default" case-insensitive and accent-insensitive. We implement this by:
///   1. Lowercasing (Unicode-aware via `char::to_lowercase`).
///   2. Decomposing to NFD so combining marks split from base characters.
///   3. Stripping Unicode combining marks (categories Mn, Mc, Me).
///
/// Examples:
///   "Müller"  → "muller"
///   "García"  → "garcia"
///   "Renée"   → "renee"
///
/// Both indexed values and query values must go through this function so the
/// stored form and the lookup form match.
pub fn normalize_string(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Returns true for Unicode combining marks (general categories Mn, Mc, Me).
///
/// Combining marks are the diacritic glyphs that NFD decomposition splits off
/// from base characters (e.g., "é" → "e" + U+0301 COMBINING ACUTE ACCENT).
/// Stripping them yields accent-insensitive matching.
fn is_combining_mark(c: char) -> bool {
    matches!(
        c as u32,
        // Combining Diacritical Marks
        0x0300..=0x036F
        // Combining Diacritical Marks Extended
        | 0x1AB0..=0x1AFF
        // Combining Diacritical Marks Supplement
        | 0x1DC0..=0x1DFF
        // Combining Diacritical Marks for Symbols
        | 0x20D0..=0x20FF
        // Combining Half Marks
        | 0xFE20..=0xFE2F
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_string() {
        assert_eq!(normalize_string("Smith"), "smith");
        assert_eq!(normalize_string("HELLO"), "hello");
        assert_eq!(normalize_string("Müller"), "muller");
        assert_eq!(normalize_string("García Renée"), "garcia renee");
    }
}
