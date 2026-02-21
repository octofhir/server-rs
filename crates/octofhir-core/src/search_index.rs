//! Search index extraction engine.
//!
//! Extracts reference, date, and string values from FHIR resource JSON
//! for indexing in denormalized search tables.

use crate::fhir_reference::{NormalizedRef, normalize_reference_for_index};
use serde_json::Value;

/// An extracted reference ready for indexing.
#[derive(Debug, Clone)]
pub struct ExtractedReference {
    pub param_code: String,
    pub normalized: NormalizedRef,
    pub raw_reference: Option<String>,
}

/// An extracted date range ready for indexing.
#[derive(Debug, Clone)]
pub struct ExtractedDate {
    pub param_code: String,
    pub range_start: String, // ISO8601 timestamp
    pub range_end: String,   // ISO8601 timestamp
}

/// An extracted string value ready for indexing.
#[derive(Debug, Clone)]
pub struct ExtractedString {
    pub param_code: String,
    pub value_normalized: String,
    pub value_exact: String,
}

// ============================================================================
// Reference Extraction
// ============================================================================

/// Extract references from a FHIR resource using a search parameter's FHIRPath expression.
///
/// Navigates the resource JSON to find reference values, normalizes them, and
/// returns a list of extracted references for indexing.
pub fn extract_references(
    resource: &Value,
    resource_type: &str,
    param_code: &str,
    expression: &str,
    base_url: Option<&str>,
) -> Vec<ExtractedReference> {
    let path_segments = fhirpath_to_segments(expression, resource_type);
    if path_segments.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut values = Vec::new();
    navigate_json(resource, &path_segments, 0, &mut values);

    for ref_value in values {
        let raw_ref = ref_value
            .get("reference")
            .and_then(|r| r.as_str())
            .map(String::from);

        let normalized_refs = normalize_reference_for_index(&ref_value, base_url);
        for normalized in normalized_refs {
            results.push(ExtractedReference {
                param_code: param_code.to_string(),
                normalized,
                raw_reference: raw_ref.clone(),
            });
        }
    }

    results
}

// ============================================================================
// Date Extraction
// ============================================================================

/// Extract date values from a FHIR resource for indexing.
///
/// Converts FHIR date precision (year, month, day, instant) into explicit
/// start/end ranges for efficient B-tree range queries.
pub fn extract_dates(
    resource: &Value,
    resource_type: &str,
    param_code: &str,
    expression: &str,
) -> Vec<ExtractedDate> {
    let path_segments = fhirpath_to_segments(expression, resource_type);
    if path_segments.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut values = Vec::new();
    navigate_json(resource, &path_segments, 0, &mut values);

    for date_value in values {
        // Handle Period type (has start/end)
        if date_value.get("start").is_some() || date_value.get("end").is_some() {
            if let Some(range) = parse_period_to_range(&date_value) {
                results.push(ExtractedDate {
                    param_code: param_code.to_string(),
                    range_start: range.0,
                    range_end: range.1,
                });
            }
            continue;
        }

        // Handle simple date/dateTime string
        if let Some(date_str) = date_value.as_str()
            && let Some(range) = parse_date_to_range(date_str)
        {
            results.push(ExtractedDate {
                param_code: param_code.to_string(),
                range_start: range.0,
                range_end: range.1,
            });
        }
    }

    results
}

/// Parse a FHIR date string into a start/end range based on precision.
fn parse_date_to_range(date_str: &str) -> Option<(String, String)> {
    let trimmed = date_str.trim();
    let len = trimmed.len();

    // Year only: "2024"
    if len == 4 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        let year: i32 = trimmed.parse().ok()?;
        return Some((
            format!("{year}-01-01T00:00:00Z"),
            format!("{}-12-31T23:59:59.999Z", year),
        ));
    }

    // Year-Month: "2024-03"
    if len == 7 && trimmed.chars().nth(4) == Some('-') {
        let year: i32 = trimmed[..4].parse().ok()?;
        let month: u32 = trimmed[5..7].parse().ok()?;
        if !(1..=12).contains(&month) {
            return None;
        }
        let last_day = days_in_month(year, month);
        return Some((
            format!("{year}-{month:02}-01T00:00:00Z"),
            format!("{year}-{month:02}-{last_day:02}T23:59:59.999Z"),
        ));
    }

    // Full date: "2024-03-15"
    if len == 10 && !trimmed.contains('T') {
        return Some((
            format!("{trimmed}T00:00:00Z"),
            format!("{trimmed}T23:59:59.999Z"),
        ));
    }

    // DateTime with timezone
    if trimmed.contains('T') {
        // Already a precise instant — use as both start and end
        return Some((trimmed.to_string(), trimmed.to_string()));
    }

    None
}

/// Parse a FHIR Period into start/end timestamps.
fn parse_period_to_range(period: &Value) -> Option<(String, String)> {
    let start = period
        .get("start")
        .and_then(|s| s.as_str())
        .and_then(|s| parse_date_to_range(s).map(|r| r.0));

    let end = period
        .get("end")
        .and_then(|e| e.as_str())
        .and_then(|e| parse_date_to_range(e).map(|r| r.1));

    match (start, end) {
        (Some(s), Some(e)) => Some((s, e)),
        (Some(s), None) => Some((s.clone(), s)), // Open-ended period
        (None, Some(e)) => Some((e.clone(), e)), // Start-less period (unusual)
        (None, None) => None,
    }
}

/// Get number of days in a month.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

// ============================================================================
// String Extraction
// ============================================================================

/// Extract string values from a FHIR resource for indexing.
pub fn extract_strings(
    resource: &Value,
    resource_type: &str,
    param_code: &str,
    expression: &str,
) -> Vec<ExtractedString> {
    let path_segments = fhirpath_to_segments(expression, resource_type);
    if path_segments.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut values = Vec::new();
    navigate_json(resource, &path_segments, 0, &mut values);

    for str_value in values {
        // Handle HumanName type
        if str_value.get("family").is_some() || str_value.get("given").is_some() {
            extract_human_name_strings(&str_value, param_code, &mut results);
            continue;
        }

        // Handle Address type
        if str_value.get("line").is_some() || str_value.get("city").is_some() {
            extract_address_strings(&str_value, param_code, &mut results);
            continue;
        }

        // Handle simple string
        if let Some(s) = str_value.as_str()
            && !s.is_empty()
        {
            results.push(ExtractedString {
                param_code: param_code.to_string(),
                value_normalized: normalize_string(s),
                value_exact: s.to_string(),
            });
        }
    }

    results
}

/// Extract string values from a HumanName object.
fn extract_human_name_strings(name: &Value, param_code: &str, results: &mut Vec<ExtractedString>) {
    if let Some(family) = name.get("family").and_then(|f| f.as_str())
        && !family.is_empty()
    {
        results.push(ExtractedString {
            param_code: param_code.to_string(),
            value_normalized: normalize_string(family),
            value_exact: family.to_string(),
        });
    }

    if let Some(given) = name.get("given").and_then(|g| g.as_array()) {
        for g in given {
            if let Some(s) = g.as_str()
                && !s.is_empty()
            {
                results.push(ExtractedString {
                    param_code: param_code.to_string(),
                    value_normalized: normalize_string(s),
                    value_exact: s.to_string(),
                });
            }
        }
    }

    if let Some(text) = name.get("text").and_then(|t| t.as_str())
        && !text.is_empty()
    {
        results.push(ExtractedString {
            param_code: param_code.to_string(),
            value_normalized: normalize_string(text),
            value_exact: text.to_string(),
        });
    }
}

/// Extract string values from an Address object.
fn extract_address_strings(addr: &Value, param_code: &str, results: &mut Vec<ExtractedString>) {
    for field in &["city", "state", "country", "postalCode", "district", "text"] {
        if let Some(val) = addr.get(field).and_then(|v| v.as_str())
            && !val.is_empty()
        {
            results.push(ExtractedString {
                param_code: param_code.to_string(),
                value_normalized: normalize_string(val),
                value_exact: val.to_string(),
            });
        }
    }

    if let Some(lines) = addr.get("line").and_then(|l| l.as_array()) {
        for line in lines {
            if let Some(s) = line.as_str()
                && !s.is_empty()
            {
                results.push(ExtractedString {
                    param_code: param_code.to_string(),
                    value_normalized: normalize_string(s),
                    value_exact: s.to_string(),
                });
            }
        }
    }
}

/// Normalize a string for search: lowercase and basic accent folding.
///
/// This is a simplified normalizer. For full Unicode NFD decomposition,
/// add the `unicode-normalization` crate.
pub fn normalize_string(s: &str) -> String {
    s.to_lowercase()
}

// ============================================================================
// JSON Navigation Helpers
// ============================================================================

/// Convert a FHIRPath expression to JSON path segments.
///
/// Simplified conversion for common patterns like `Observation.subject`,
/// `Patient.name.family`, etc. Strips resource type prefix and FHIRPath functions.
fn fhirpath_to_segments(expression: &str, resource_type: &str) -> Vec<String> {
    // Handle union expressions — find the one matching our resource type
    let expr = if expression.contains('|') {
        expression
            .split('|')
            .map(|s| s.trim())
            .find(|s| s.starts_with(&format!("{resource_type}.")))
            .or_else(|| expression.split('|').next().map(|s| s.trim()))
            .unwrap_or(expression)
    } else {
        expression
    };

    // Strip `as Type` casting
    let expr = if let Some(idx) = expr.find(" as ") {
        expr[..idx]
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
    } else {
        expr
    };

    // Strip resource type prefix
    let expr = expr
        .strip_prefix(&format!("{resource_type}."))
        .or_else(|| expr.strip_prefix("Resource."))
        .or_else(|| expr.strip_prefix("DomainResource."))
        .or_else(|| {
            if let Some(idx) = expr.find('.') {
                let prefix = &expr[..idx];
                if prefix
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_uppercase())
                    .unwrap_or(false)
                {
                    return Some(&expr[idx + 1..]);
                }
            }
            None
        })
        .unwrap_or(expr);

    // Strip FHIRPath function calls
    let expr = strip_fhirpath_functions(expr);

    expr.split('.')
        .filter(|s| !s.is_empty())
        .map(|s| {
            // Remove array subscripts
            if let Some(base) = s.strip_suffix(']')
                && let Some((name, _)) = base.split_once('[')
            {
                return name.to_string();
            }
            s.to_string()
        })
        .collect()
}

/// Strip FHIRPath function calls, keeping property paths.
fn strip_fhirpath_functions(expr: &str) -> String {
    let mut result = String::with_capacity(expr.len());
    let mut i = 0;
    let bytes = expr.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'(' {
            let mut depth = 1;
            i += 1;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            // Remove function name
            if let Some(dot_pos) = result.rfind('.') {
                let func = &result[dot_pos + 1..];
                if is_fhirpath_function(func) {
                    result.truncate(dot_pos);
                }
            } else if is_fhirpath_function(&result) {
                result.clear();
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    while result.ends_with('.') {
        result.pop();
    }
    while result.starts_with('.') {
        result.remove(0);
    }

    result
}

fn is_fhirpath_function(name: &str) -> bool {
    matches!(
        name,
        "where"
            | "resolve"
            | "ofType"
            | "exists"
            | "empty"
            | "first"
            | "last"
            | "as"
            | "is"
            | "not"
            | "all"
            | "any"
            | "count"
            | "distinct"
            | "single"
            | "type"
    )
}

/// Navigate JSON following path segments, collecting leaf values.
///
/// Handles arrays transparently — if a segment points to an array,
/// recurse into each element. Also handles FHIR polymorphic fields
/// (e.g., `effective` matches `effectiveDateTime`, `effectivePeriod`).
fn navigate_json(value: &Value, segments: &[String], depth: usize, results: &mut Vec<Value>) {
    if depth >= segments.len() {
        // Reached the target — collect this value
        match value {
            Value::Array(arr) => {
                for item in arr {
                    results.push(item.clone());
                }
            }
            Value::Null => {}
            _ => results.push(value.clone()),
        }
        return;
    }

    let segment = &segments[depth];

    match value {
        Value::Object(obj) => {
            if let Some(child) = obj.get(segment.as_str()) {
                navigate_json(child, segments, depth + 1, results);
            } else {
                // Try FHIR polymorphic field names: segment + Type suffix
                // e.g., "effective" matches "effectiveDateTime", "effectivePeriod"
                for (key, child) in obj {
                    if key.len() > segment.len()
                        && key.starts_with(segment.as_str())
                        && key.as_bytes()[segment.len()].is_ascii_uppercase()
                    {
                        navigate_json(child, segments, depth + 1, results);
                    }
                }
            }
        }
        Value::Array(arr) => {
            // Transparently iterate arrays
            for item in arr {
                if let Some(child) = item.get(segment.as_str()) {
                    navigate_json(child, segments, depth + 1, results);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_references_simple() {
        let resource = json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "subject": {
                "reference": "Patient/123"
            }
        });

        let refs = extract_references(
            &resource,
            "Observation",
            "subject",
            "Observation.subject",
            None,
        );
        assert_eq!(refs.len(), 1);
        assert!(
            matches!(&refs[0].normalized, NormalizedRef::Local { target_type, target_id }
            if target_type == "Patient" && target_id == "123")
        );
        assert_eq!(refs[0].param_code, "subject");
    }

    #[test]
    fn test_extract_references_array() {
        let resource = json!({
            "resourceType": "Observation",
            "performer": [
                { "reference": "Practitioner/456" },
                { "reference": "Organization/789" }
            ]
        });

        let refs = extract_references(
            &resource,
            "Observation",
            "performer",
            "Observation.performer",
            None,
        );
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_extract_references_identifier() {
        let resource = json!({
            "resourceType": "Observation",
            "subject": {
                "identifier": {
                    "system": "http://hospital.org",
                    "value": "MRN123"
                }
            }
        });

        let refs = extract_references(
            &resource,
            "Observation",
            "subject",
            "Observation.subject",
            None,
        );
        assert_eq!(refs.len(), 1);
        assert!(
            matches!(&refs[0].normalized, NormalizedRef::Identifier { system, value }
            if system.as_deref() == Some("http://hospital.org") && value == "MRN123")
        );
    }

    #[test]
    fn test_extract_dates_simple() {
        let resource = json!({
            "resourceType": "Observation",
            "effectiveDateTime": "2024-03-15"
        });

        let dates = extract_dates(
            &resource,
            "Observation",
            "date",
            "Observation.effectiveDateTime",
        );
        assert_eq!(dates.len(), 1);
        assert!(dates[0].range_start.starts_with("2024-03-15T00:00:00"));
        assert!(dates[0].range_end.starts_with("2024-03-15T23:59:59"));
    }

    #[test]
    fn test_extract_dates_year() {
        let resource = json!({
            "resourceType": "Patient",
            "birthDate": "1990"
        });

        let dates = extract_dates(&resource, "Patient", "birthdate", "Patient.birthDate");
        assert_eq!(dates.len(), 1);
        assert!(dates[0].range_start.starts_with("1990-01-01"));
        assert!(dates[0].range_end.starts_with("1990-12-31"));
    }

    #[test]
    fn test_extract_dates_period() {
        let resource = json!({
            "resourceType": "Encounter",
            "period": {
                "start": "2024-01-01",
                "end": "2024-01-15"
            }
        });

        let dates = extract_dates(&resource, "Encounter", "date", "Encounter.period");
        assert_eq!(dates.len(), 1);
        assert!(dates[0].range_start.starts_with("2024-01-01T00:00:00"));
        assert!(dates[0].range_end.starts_with("2024-01-15T23:59:59"));
    }

    #[test]
    fn test_extract_strings_simple() {
        let resource = json!({
            "resourceType": "Patient",
            "name": [{
                "family": "Smith",
                "given": ["John", "James"]
            }]
        });

        let strings = extract_strings(&resource, "Patient", "name", "Patient.name");
        // family + 2 given = 3
        assert_eq!(strings.len(), 3);
        assert!(strings.iter().any(|s| s.value_exact == "Smith"));
        assert!(strings.iter().any(|s| s.value_exact == "John"));
    }

    #[test]
    fn test_normalize_string() {
        assert_eq!(normalize_string("Smith"), "smith");
        assert_eq!(normalize_string("HELLO"), "hello");
    }

    #[test]
    fn test_fhirpath_to_segments() {
        let segs = fhirpath_to_segments("Observation.subject", "Observation");
        assert_eq!(segs, vec!["subject"]);

        let segs = fhirpath_to_segments("Patient.name.family", "Patient");
        assert_eq!(segs, vec!["name", "family"]);

        let segs = fhirpath_to_segments(
            "Observation.subject.where(resolve() is Patient)",
            "Observation",
        );
        assert_eq!(segs, vec!["subject"]);
    }

    #[test]
    fn test_parse_date_ranges() {
        let range = parse_date_to_range("2024").unwrap();
        assert!(range.0.starts_with("2024-01-01"));
        assert!(range.1.starts_with("2024-12-31"));

        let range = parse_date_to_range("2024-02").unwrap();
        assert!(range.0.starts_with("2024-02-01"));
        assert!(range.1.starts_with("2024-02-29")); // leap year

        let range = parse_date_to_range("2023-02").unwrap();
        assert!(range.1.starts_with("2023-02-28")); // non-leap year
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 2), 29); // leap year
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
    }
}
