use crate::parameters::{SearchModifier, SearchParameter, SearchParameterType, SearchPrefix};
use crate::registry::SearchParameterRegistry;
use std::borrow::Cow;
use thiserror::Error;
use url::form_urlencoded;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedValue {
    pub prefix: Option<SearchPrefix>,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedParam {
    pub name: String,
    pub modifier: Option<SearchModifier>,
    pub values: Vec<ParsedValue>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedParameters {
    pub params: Vec<ParsedParam>,
}

use octofhir_core::{FhirDateTime, ResourceType};
use octofhir_storage::legacy::{QueryFilter, SearchQuery};

pub struct SearchParameterParser;

impl SearchParameterParser {
    /// Parse an application/x-www-form-urlencoded query string into ParsedParameters
    /// Example: "name:exact=John&_lastUpdated=ge2020-01-01"
    pub fn parse_query(query: &str) -> ParsedParameters {
        let mut result = ParsedParameters::default();
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            let (name, modifier) = Self::split_name_and_modifier(k);
            let mut values = Vec::new();
            // Support comma-separated values per FHIR search rules
            for raw_val in v.split(',') {
                let raw_val = raw_val.trim();
                if raw_val.is_empty() {
                    continue;
                }
                let (prefix, remainder) = Self::extract_prefix(raw_val);
                values.push(ParsedValue {
                    prefix,
                    raw: remainder.to_string(),
                });
            }
            result.params.push(ParsedParam {
                name: name.into_owned(),
                modifier,
                values,
            });
        }
        result
    }

    fn split_name_and_modifier(key: Cow<'_, str>) -> (Cow<'_, str>, Option<SearchModifier>) {
        if let Some((name, modifier)) = key.split_once(':') {
            // handle :missing specially; others map to enum variants
            let modifier = match modifier {
                "exact" => Some(SearchModifier::Exact),
                "contains" => Some(SearchModifier::Contains),
                "text" => Some(SearchModifier::Text),
                "in" => Some(SearchModifier::In),
                "not-in" => Some(SearchModifier::NotIn),
                "below" => Some(SearchModifier::Below),
                "above" => Some(SearchModifier::Above),
                "not" => Some(SearchModifier::Not),
                "identifier" => Some(SearchModifier::Identifier),
                "missing" => Some(SearchModifier::Missing),
                other if !other.is_empty() => Some(SearchModifier::Type(other.to_string())),
                _ => None,
            };
            (Cow::Owned(name.to_string()), modifier)
        } else {
            (key, None)
        }
    }

    fn extract_prefix(value: &str) -> (Option<SearchPrefix>, &str) {
        // The longest valid prefixes are two chars, check two, then one
        if value.len() >= 2 {
            let p2 = &value[..2];
            if let Some(prefix) = SearchPrefix::parse(p2) {
                return (Some(prefix), &value[2..]);
            }
        }
        if !value.is_empty() {
            let p1 = &value[..1];
            if let Some(prefix) = SearchPrefix::parse(p1) {
                return (Some(prefix), &value[1..]);
            }
        }
        (None, value)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SearchValidationError {
    #[error("Unknown search parameter: {0}")]
    UnknownParameter(String),
    #[error("Invalid value for {param}: {message}")]
    InvalidValue { param: String, message: String },
}

impl ParsedParameters {
    /// Validate parameters against a basic allow-list and constraints.
    pub fn validate(
        &self,
        allowed_params: &[&str],
        allowed_sort_fields: &[&str],
        max_count: usize,
    ) -> Result<(), SearchValidationError> {
        // Unknown params
        for p in &self.params {
            if !allowed_params.contains(&p.name.as_str()) {
                return Err(SearchValidationError::UnknownParameter(p.name.clone()));
            }
        }
        // _count must be positive integer within max
        if let Some(p) = self.params.iter().find(|p| p.name == "_count")
            && let Some(v) = p.values.first()
        {
            if let Ok(n) = v.raw.parse::<usize>() {
                if n == 0 {
                    return Err(SearchValidationError::InvalidValue {
                        param: "_count".to_string(),
                        message: "must be >= 1".to_string(),
                    });
                }
                if n > max_count {
                    return Err(SearchValidationError::InvalidValue {
                        param: "_count".to_string(),
                        message: format!("exceeds maximum of {max_count}"),
                    });
                }
            } else {
                return Err(SearchValidationError::InvalidValue {
                    param: "_count".to_string(),
                    message: "must be a positive integer".to_string(),
                });
            }
        }
        // _offset must be non-negative integer (usize)
        if let Some(p) = self.params.iter().find(|p| p.name == "_offset")
            && let Some(v) = p.values.first()
            && v.raw.parse::<usize>().is_err()
        {
            return Err(SearchValidationError::InvalidValue {
                param: "_offset".to_string(),
                message: "must be a non-negative integer".to_string(),
            });
        }
        // _sort field must be in allowed list if present
        if let Some(p) = self.params.iter().find(|p| p.name == "_sort") {
            if let Some(v) = p.values.first() {
                let mut field = v.raw.as_str();
                if let Some(stripped) = field.strip_prefix('-') {
                    field = stripped;
                }
                if field.is_empty() || !allowed_sort_fields.contains(&field) {
                    return Err(SearchValidationError::InvalidValue {
                        param: "_sort".to_string(),
                        message: format!(
                            "unsupported sort field '{}'; allowed: {}",
                            field,
                            allowed_sort_fields.join(", ")
                        ),
                    });
                }
            } else {
                return Err(SearchValidationError::InvalidValue {
                    param: "_sort".to_string(),
                    message: "missing sort field value".to_string(),
                });
            }
        }
        Ok(())
    }

    /// Parse the optional _count parameter with defaults and clamping.
    /// Returns an effective count value within [1..=max]. If missing/invalid, returns `default_`.
    pub fn parse_count(&self, default_: usize, max: usize) -> usize {
        // find first occurrence of _count
        if let Some(p) = self.params.iter().find(|p| p.name == "_count")
            && let Some(v) = p.values.first()
            && let Ok(n) = v.raw.parse::<usize>()
        {
            if n == 0 {
                return default_;
            }
            return n.min(max);
        }
        default_
    }

    /// Parse optional _offset parameter; returns default_ if missing/invalid.
    pub fn parse_offset(&self, default_: usize) -> usize {
        if let Some(p) = self.params.iter().find(|p| p.name == "_offset")
            && let Some(v) = p.values.first()
            && let Ok(n) = v.raw.parse::<usize>()
        {
            return n;
        }
        default_
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_missing_uses_default() {
        let parsed = SearchParameterParser::parse_query("_id=abc");
        assert_eq!(parsed.parse_count(10, 100), 10);
    }

    #[test]
    fn count_within_range_is_used() {
        let parsed = SearchParameterParser::parse_query("_count=25");
        assert_eq!(parsed.parse_count(10, 100), 25);
    }

    #[test]
    fn count_over_max_is_clamped() {
        let parsed = SearchParameterParser::parse_query("_count=250");
        assert_eq!(parsed.parse_count(10, 100), 100);
    }

    #[test]
    fn count_invalid_or_zero_uses_default() {
        let parsed_invalid = SearchParameterParser::parse_query("_count=abc");
        assert_eq!(parsed_invalid.parse_count(10, 100), 10);
        let parsed_zero = SearchParameterParser::parse_query("_count=0");
        assert_eq!(parsed_zero.parse_count(10, 100), 10);
    }
}

#[cfg(test)]
mod tests_parsing {
    use super::*;
    use crate::parameters::SearchModifier;

    #[test]
    fn parses_contains_modifier_for_name() {
        let parsed = SearchParameterParser::parse_query("name:contains=Jo");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.name, "name");
        assert_eq!(p.modifier, Some(SearchModifier::Contains));
        assert_eq!(p.values.len(), 1);
        assert_eq!(p.values[0].raw, "Jo");
    }

    #[test]
    fn parses_missing_modifier_boolean_value_as_raw() {
        let parsed = SearchParameterParser::parse_query("_id:missing=true");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.name, "_id");
        assert_eq!(p.modifier, Some(SearchModifier::Missing));
        assert_eq!(p.values.len(), 1);
        assert_eq!(p.values[0].raw, "true");
    }

    #[test]
    fn url_decoding_of_space_works() {
        let parsed = SearchParameterParser::parse_query("name=John%20Doe");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.name, "name");
        assert_eq!(p.values.len(), 1);
        assert_eq!(p.values[0].raw, "John Doe");
    }

    #[test]
    fn empty_value_produces_param_with_no_values() {
        let parsed = SearchParameterParser::parse_query("name=");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.name, "name");
        assert!(p.values.is_empty());
    }

    #[test]
    fn parses_multiple_params() {
        let parsed = SearchParameterParser::parse_query("_id=abc&_lastUpdated=ge2020-01-01");
        assert_eq!(parsed.params.len(), 2);
        assert!(parsed.params.iter().any(|p| p.name == "_id"));
        assert!(parsed.params.iter().any(|p| p.name == "_lastUpdated"));
    }

    #[test]
    fn type_modifier_is_parsed_into_type_variant() {
        let parsed = SearchParameterParser::parse_query("subject:Patient=123");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.name, "subject");
        match &p.modifier {
            Some(SearchModifier::Type(t)) => assert_eq!(t, "Patient"),
            other => panic!("expected Type modifier, got {other:?}"),
        }
        assert_eq!(p.values.len(), 1);
        assert_eq!(p.values[0].raw, "123");
    }

    #[test]
    fn multiple_count_params_use_first_occurrence() {
        let parsed = SearchParameterParser::parse_query("_count=5&_count=20");
        let effective = parsed.parse_count(10, 100);
        assert_eq!(effective, 5);
    }

    #[test]
    fn plus_is_decoded_to_space() {
        let parsed = SearchParameterParser::parse_query("name=John+Doe");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.values[0].raw, "John Doe");
    }

    #[test]
    fn parses_number_like_value_with_ge_prefix() {
        let parsed = SearchParameterParser::parse_query("value=ge5.5");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.name, "value");
        assert_eq!(p.values.len(), 1);
        assert_eq!(p.values[0].prefix, Some(SearchPrefix::Ge));
        assert_eq!(p.values[0].raw, "5.5");
    }

    #[test]
    fn unknown_prefix_is_not_parsed_and_kept_in_raw() {
        let parsed = SearchParameterParser::parse_query("foo=xx2020");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.name, "foo");
        assert_eq!(p.values.len(), 1);
        assert_eq!(p.values[0].prefix, None);
        assert_eq!(p.values[0].raw, "xx2020");
    }

    #[test]
    fn uri_style_value_is_url_decoded() {
        let parsed = SearchParameterParser::parse_query("uri=https%3A%2F%2Fexample.org%2Fabc");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.name, "uri");
        assert_eq!(p.values.len(), 1);
        assert_eq!(p.values[0].raw, "https://example.org/abc");
    }
}

#[cfg(test)]
mod tests_validation {
    use super::*;

    #[test]
    fn parse_offset_defaults_and_valid() {
        let p = SearchParameterParser::parse_query("");
        assert_eq!(p.parse_offset(0), 0);
        let p = SearchParameterParser::parse_query("_offset=15");
        assert_eq!(p.parse_offset(0), 15);
        let p = SearchParameterParser::parse_query("_offset=abc");
        assert_eq!(p.parse_offset(7), 7);
    }

    #[test]
    fn validate_unknown_param_fails() {
        let parsed = SearchParameterParser::parse_query("foo=bar");
        let allowed = [
            "_id",
            "_lastUpdated",
            "_count",
            "_offset",
            "_sort",
            "identifier",
            "name",
            "family",
            "given",
        ];
        let allowed_sort = ["_id", "_lastUpdated"];
        let err = parsed.validate(&allowed, &allowed_sort, 100).unwrap_err();
        match err {
            SearchValidationError::UnknownParameter(p) => assert_eq!(p, "foo"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn validate_count_and_sort_rules() {
        let allowed = [
            "_id",
            "_lastUpdated",
            "_count",
            "_offset",
            "_sort",
            "identifier",
            "name",
            "family",
            "given",
        ];
        let allowed_sort = ["_id", "_lastUpdated"];
        // invalid _count
        let p = SearchParameterParser::parse_query("_count=0");
        assert!(p.validate(&allowed, &allowed_sort, 100).is_err());
        let p = SearchParameterParser::parse_query("_count=abc");
        assert!(p.validate(&allowed, &allowed_sort, 100).is_err());
        let p = SearchParameterParser::parse_query("_count=1000");
        assert!(p.validate(&allowed, &allowed_sort, 100).is_err());
        // invalid _offset
        let p = SearchParameterParser::parse_query("_offset=-1");
        assert!(p.validate(&allowed, &allowed_sort, 100).is_err());
        // invalid _sort field
        let p = SearchParameterParser::parse_query("_sort=foo");
        assert!(p.validate(&allowed, &allowed_sort, 100).is_err());
        // valid case
        let p = SearchParameterParser::parse_query("_sort=-_id&_count=5");
        assert!(p.validate(&allowed, &allowed_sort, 100).is_ok());
    }
}

// ============================================================================
// Registry-Based Parameter Validation and Filter Building
// ============================================================================

/// Control parameters that are always allowed (not resource-specific)
const CONTROL_PARAMS: &[&str] = &[
    "_count",
    "_offset",
    "_sort",
    "_include",
    "_revinclude",
    "_summary",
    "_elements",
    "_total",
    "_contained",
    "_containedType",
];

impl ParsedParameters {
    /// Validate parameters using the search parameter registry.
    ///
    /// Checks that each parameter either:
    /// - Is a control parameter (e.g., _count, _offset, _sort)
    /// - Exists in the registry for the given resource type
    pub fn validate_with_registry(
        &self,
        resource_type: &str,
        registry: &SearchParameterRegistry,
        max_count: usize,
    ) -> Result<(), SearchValidationError> {
        // Validate each parameter
        for p in &self.params {
            // Allow control parameters
            if CONTROL_PARAMS.contains(&p.name.as_str()) {
                continue;
            }

            // Check if parameter exists in registry for this resource type
            if registry.get(resource_type, &p.name).is_none() {
                return Err(SearchValidationError::UnknownParameter(p.name.clone()));
            }
        }

        // Validate _count constraints
        if let Some(p) = self.params.iter().find(|p| p.name == "_count")
            && let Some(v) = p.values.first()
        {
            if let Ok(n) = v.raw.parse::<usize>() {
                if n == 0 {
                    return Err(SearchValidationError::InvalidValue {
                        param: "_count".to_string(),
                        message: "must be >= 1".to_string(),
                    });
                }
                if n > max_count {
                    return Err(SearchValidationError::InvalidValue {
                        param: "_count".to_string(),
                        message: format!("exceeds maximum of {max_count}"),
                    });
                }
            } else {
                return Err(SearchValidationError::InvalidValue {
                    param: "_count".to_string(),
                    message: "must be a positive integer".to_string(),
                });
            }
        }

        // Validate _offset
        if let Some(p) = self.params.iter().find(|p| p.name == "_offset")
            && let Some(v) = p.values.first()
            && v.raw.parse::<usize>().is_err()
        {
            return Err(SearchValidationError::InvalidValue {
                param: "_offset".to_string(),
                message: "must be a non-negative integer".to_string(),
            });
        }

        Ok(())
    }

    /// Convert parsed parameters to filters using the registry for type-based conversion.
    ///
    /// This method uses the parameter type from the registry to determine how to build
    /// the appropriate filter:
    /// - String -> Contains or Exact filter
    /// - Token -> Token filter
    /// - Date -> DateRange filter
    /// - Reference -> Reference filter
    /// - Number -> Number filter (if supported)
    /// - Quantity -> Quantity filter (if supported)
    pub fn to_filters_with_registry(
        &self,
        resource_type: &str,
        registry: &SearchParameterRegistry,
    ) -> Vec<QueryFilter> {
        let mut filters = Vec::new();

        for p in &self.params {
            // Skip control parameters - they don't generate filters
            if CONTROL_PARAMS.contains(&p.name.as_str()) {
                continue;
            }

            // Look up the parameter in the registry
            let Some(param_def) = registry.get(resource_type, &p.name) else {
                // Unknown parameter - skip (validation should have caught this)
                continue;
            };

            // Extract field name from FHIRPath expression
            let field = extract_field_name(&param_def);

            // Build filter based on parameter type
            match param_def.param_type {
                SearchParameterType::String => {
                    if let Some(filter) = build_string_filter(&p.name, &field, p) {
                        filters.push(filter);
                    }
                }
                SearchParameterType::Token => {
                    if let Some(filter) = build_token_filter(&field, p) {
                        filters.push(filter);
                    }
                }
                SearchParameterType::Date => {
                    if let Some(filter) = build_date_filter(&field, p) {
                        filters.push(filter);
                    }
                }
                SearchParameterType::Reference => {
                    if let Some(filter) = build_reference_filter(&field, p) {
                        filters.push(filter);
                    }
                }
                SearchParameterType::Number => {
                    // TODO: Implement number filter when storage supports it
                    tracing::debug!(param = %p.name, "number search not yet implemented");
                }
                SearchParameterType::Quantity => {
                    // TODO: Implement quantity filter when storage supports it
                    tracing::debug!(param = %p.name, "quantity search not yet implemented");
                }
                SearchParameterType::Uri => {
                    if let Some(filter) = build_uri_filter(&field, p) {
                        filters.push(filter);
                    }
                }
                SearchParameterType::Composite => {
                    // TODO: Implement composite search
                    tracing::debug!(param = %p.name, "composite search not yet implemented");
                }
                SearchParameterType::Special => {
                    // Special parameters like _id, _lastUpdated are handled separately
                    if let Some(filter) = build_special_filter(&p.name, p) {
                        filters.push(filter);
                    }
                }
            }
        }

        filters
    }
}

/// Extract field name from a SearchParameter's FHIRPath expression.
///
/// Examples:
/// - "Patient.name" -> "name"
/// - "Patient.identifier" -> "identifier"
/// - "Patient.birthDate" -> "birthDate"
fn extract_field_name(param: &SearchParameter) -> String {
    if let Some(expr) = &param.expression {
        // Handle simple paths: "Resource.field" -> "field"
        // Also handle patterns like "Patient.name.family" -> "name"
        // or "(Patient.name | Practitioner.name)" -> "name"
        let expr = expr.trim();

        // Handle union expressions: "(A.x | B.x)" - take first part
        let expr = if expr.starts_with('(') {
            expr.trim_start_matches('(')
                .split('|')
                .next()
                .unwrap_or(expr)
                .trim()
        } else {
            expr
        };

        // Handle .where() and .as() modifiers by taking prefix
        let expr = expr.split(".where(").next().unwrap_or(expr);
        let expr = expr.split(".as(").next().unwrap_or(expr);

        // Split by '.' and take second part (after resource type)
        let parts: Vec<&str> = expr.split('.').collect();
        if parts.len() >= 2 {
            return parts[1].to_string();
        }
    }

    // Fallback to parameter code
    param.code.clone()
}

/// Build a string-type filter (Contains or Exact based on modifier).
fn build_string_filter(_param_name: &str, field: &str, p: &ParsedParam) -> Option<QueryFilter> {
    let v = p.values.first()?;
    if v.raw.is_empty() {
        return None;
    }

    match p.modifier {
        Some(SearchModifier::Exact) => Some(QueryFilter::Exact {
            field: field.to_string(),
            value: v.raw.clone(),
        }),
        _ => Some(QueryFilter::Contains {
            field: field.to_string(),
            value: v.raw.clone(),
        }),
    }
}

/// Build a token-type filter (code/system matching).
fn build_token_filter(field: &str, p: &ParsedParam) -> Option<QueryFilter> {
    let v = p.values.first()?;
    if v.raw.is_empty() {
        return None;
    }

    // Token format: system|code or just code
    let (system, code) = if let Some(pos) = v.raw.find('|') {
        let sys = &v.raw[..pos];
        let code = &v.raw[pos + 1..];
        (
            if sys.is_empty() {
                None
            } else {
                Some(sys.to_string())
            },
            code.to_string(),
        )
    } else {
        (None, v.raw.clone())
    };

    // Handle boolean fields specially
    if field == "active" || field == "deceased" {
        let value = code.eq_ignore_ascii_case("true");
        return Some(QueryFilter::Boolean {
            field: field.to_string(),
            value,
        });
    }

    // Handle identifier specially
    if field == "identifier" {
        return Some(QueryFilter::Identifier {
            field: field.to_string(),
            system,
            value: code,
        });
    }

    Some(QueryFilter::Token {
        field: field.to_string(),
        system,
        code,
    })
}

/// Build a date-type filter (DateRange with optional prefixes).
fn build_date_filter(field: &str, p: &ParsedParam) -> Option<QueryFilter> {
    let mut start: Option<FhirDateTime> = None;
    let mut end: Option<FhirDateTime> = None;

    for v in &p.values {
        if v.raw.is_empty() {
            continue;
        }
        if let Ok(dt) = v.raw.parse::<FhirDateTime>() {
            match v.prefix {
                Some(SearchPrefix::Ge) | Some(SearchPrefix::Gt) => start = Some(dt),
                Some(SearchPrefix::Le) | Some(SearchPrefix::Lt) => end = Some(dt),
                Some(SearchPrefix::Eq) | None => {
                    start = Some(dt.clone());
                    end = Some(dt);
                }
                _ => { /* unsupported prefixes ignored */ }
            }
        }
    }

    if start.is_some() || end.is_some() {
        Some(QueryFilter::DateRange {
            field: field.to_string(),
            start,
            end,
        })
    } else {
        None
    }
}

/// Build a reference-type filter.
/// References are stored as strings, so we use Contains filter to match.
fn build_reference_filter(field: &str, p: &ParsedParam) -> Option<QueryFilter> {
    let v = p.values.first()?;
    if v.raw.is_empty() {
        return None;
    }

    // Reference can be: Type/id, url, or just id
    // Use Contains filter to match reference values
    Some(QueryFilter::Contains {
        field: field.to_string(),
        value: v.raw.clone(),
    })
}

/// Build a URI-type filter.
fn build_uri_filter(field: &str, p: &ParsedParam) -> Option<QueryFilter> {
    let v = p.values.first()?;
    if v.raw.is_empty() {
        return None;
    }

    // URI is treated as exact match
    Some(QueryFilter::Exact {
        field: field.to_string(),
        value: v.raw.clone(),
    })
}

/// Build filters for special parameters (_id, _lastUpdated).
fn build_special_filter(param_name: &str, p: &ParsedParam) -> Option<QueryFilter> {
    match param_name {
        "_id" => {
            let v = p.values.first()?;
            if v.raw.is_empty() {
                return None;
            }
            Some(QueryFilter::Exact {
                field: "_id".to_string(),
                value: v.raw.clone(),
            })
        }
        "_lastUpdated" => {
            let mut start: Option<FhirDateTime> = None;
            let mut end: Option<FhirDateTime> = None;

            for v in &p.values {
                if v.raw.is_empty() {
                    continue;
                }
                if let Ok(dt) = v.raw.parse::<FhirDateTime>() {
                    match v.prefix {
                        Some(SearchPrefix::Ge) | Some(SearchPrefix::Gt) => start = Some(dt),
                        Some(SearchPrefix::Le) | Some(SearchPrefix::Lt) => end = Some(dt),
                        Some(SearchPrefix::Eq) | None => {
                            start = Some(dt.clone());
                            end = Some(dt);
                        }
                        _ => {}
                    }
                }
            }

            if start.is_some() || end.is_some() {
                Some(QueryFilter::DateRange {
                    field: "_lastUpdated".to_string(),
                    start,
                    end,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

impl SearchParameterParser {
    /// Validate and build a SearchQuery using the registry for dynamic parameter lookup.
    ///
    /// This method:
    /// 1. Parses the query string
    /// 2. Validates parameters against the registry
    /// 3. Builds filters based on parameter types from the registry
    pub fn validate_and_build_with_registry(
        resource_type: ResourceType,
        query: &str,
        default_count: usize,
        max_count: usize,
        registry: &SearchParameterRegistry,
    ) -> Result<SearchQuery, SearchValidationError> {
        let parsed = Self::parse_query(query);
        let resource_type_str = resource_type.to_string();

        // Validate parameters using registry
        parsed.validate_with_registry(&resource_type_str, registry, max_count)?;

        // Build filters using registry
        let filters = parsed.to_filters_with_registry(&resource_type_str, registry);

        // Build the search query
        let count = parsed.parse_count(default_count, max_count);
        let offset = parsed.parse_offset(0);
        let mut q = SearchQuery::new(resource_type).with_pagination(offset, count);

        for f in filters {
            q = q.with_filter(f);
        }

        // Apply sorting if _sort is present
        if let Some(p) = parsed.params.iter().find(|p| p.name == "_sort")
            && let Some(v) = p.values.first()
        {
            let mut ascending = true;
            let mut field = v.raw.as_str();

            if let Some(stripped) = field.strip_prefix('-') {
                ascending = false;
                field = stripped;
            }

            if let Some(SearchModifier::Type(m)) = &p.modifier {
                if m.eq_ignore_ascii_case("desc") {
                    ascending = false;
                }
                if m.eq_ignore_ascii_case("asc") {
                    ascending = true;
                }
            }

            if !field.is_empty() {
                // Validate sort field exists in registry
                if registry.get(&resource_type_str, field).is_some()
                    || field == "_id"
                    || field == "_lastUpdated"
                {
                    q = q.with_sort(field.to_string(), ascending);
                }
            }
        }

        Ok(q)
    }
}
