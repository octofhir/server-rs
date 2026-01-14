use crate::parameters::{SearchModifier, SearchPrefix};
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
}
