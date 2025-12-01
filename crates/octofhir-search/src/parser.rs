use crate::parameters::{SearchModifier, SearchPrefix};
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

    /// Convenience: parse query and immediately convert to QueryFilter list (limited support)
    pub fn parse_query_to_filters(query: &str) -> Vec<QueryFilter> {
        let parsed = Self::parse_query(query);
        parsed.to_filters()
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

    /// Convert parsed parameters to storage filters (limited initial support)
    pub fn to_filters(&self) -> Vec<QueryFilter> {
        let mut filters = Vec::new();

        for p in &self.params {
            match p.name.as_str() {
                // _id: exact string match; if multiple values provided, take the first for now
                "_id" => {
                    if let Some(v) = p.values.first()
                        && !v.raw.is_empty()
                    {
                        filters.push(QueryFilter::Exact {
                            field: "_id".to_string(),
                            value: v.raw.clone(),
                        });
                    }
                }
                // _lastUpdated: date range; support ge, le, gt, lt, eq or no prefix (treated as eq)
                "_lastUpdated" => {
                    let mut start: Option<FhirDateTime> = None;
                    let mut end: Option<FhirDateTime> = None;

                    for v in &p.values {
                        if v.raw.is_empty() {
                            continue;
                        }
                        if let Ok(dt) = v.raw.parse::<FhirDateTime>() {
                            match v.prefix {
                                Some(SearchPrefix::Ge) => start = Some(dt),
                                Some(SearchPrefix::Gt) => start = Some(dt), // TODO: strict greater-than
                                Some(SearchPrefix::Le) => end = Some(dt),
                                Some(SearchPrefix::Lt) => end = Some(dt), // TODO: strict less-than
                                Some(SearchPrefix::Eq) | None => {
                                    start = Some(dt.clone());
                                    end = Some(dt);
                                }
                                _ => { /* unsupported prefixes (sa, eb, ap) ignored for now */ }
                            }
                        }
                    }

                    if start.is_some() || end.is_some() {
                        filters.push(QueryFilter::DateRange {
                            field: "_lastUpdated".to_string(),
                            start,
                            end,
                        });
                    }
                }
                // identifier: Token/Identifier match on identifier field, support system|value or value-only
                "identifier" => {
                    if let Some(v) = p.values.first()
                        && !v.raw.is_empty()
                    {
                        let mut parts = v.raw.splitn(2, '|');
                        let first = parts.next().unwrap_or("");
                        let second = parts.next();
                        let (system, value) = match second {
                            Some(val) => {
                                let sys_opt = if first.is_empty() {
                                    None
                                } else {
                                    Some(first.to_string())
                                };
                                (sys_opt, val.to_string())
                            }
                            None => (None, first.to_string()),
                        };
                        if !value.is_empty() {
                            filters.push(QueryFilter::Identifier {
                                field: "identifier".to_string(),
                                system,
                                value,
                            });
                        }
                    }
                }
                // Patient name-related parameters: minimal mapping to the 'name' field
                "name" | "family" | "given" => {
                    if let Some(v) = p.values.first()
                        && !v.raw.is_empty()
                    {
                        match p.modifier {
                            Some(SearchModifier::Exact) => {
                                filters.push(QueryFilter::Exact {
                                    field: "name".to_string(),
                                    value: v.raw.clone(),
                                });
                            }
                            _ => {
                                filters.push(QueryFilter::Contains {
                                    field: "name".to_string(),
                                    value: v.raw.clone(),
                                });
                            }
                        }
                    }
                }
                _ => { /* other params not yet mapped */ }
            }
        }

        filters
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
    use octofhir_storage::legacy::QueryFilter;

    #[test]
    fn parses_id_and_maps_to_exact_filter() {
        let filters = SearchParameterParser::parse_query_to_filters("_id=abc123");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::Exact { field, value } => {
                assert_eq!(field, "_id");
                assert_eq!(value, "abc123");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

    #[test]
    fn parses_last_updated_ge_to_date_range_start() {
        let filters =
            SearchParameterParser::parse_query_to_filters("_lastUpdated=ge2020-01-01T00:00:00Z");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::DateRange { field, start, end } => {
                assert_eq!(field, "_lastUpdated");
                assert!(start.is_some());
                assert!(end.is_none());
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

    #[test]
    fn parses_last_updated_eq_to_date_range_start_and_end() {
        let filters =
            SearchParameterParser::parse_query_to_filters("_lastUpdated=2020-01-01T00:00:00Z");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::DateRange { field, start, end } => {
                assert_eq!(field, "_lastUpdated");
                assert!(start.is_some());
                assert!(end.is_some());
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

    #[test]
    fn parses_identifier_system_and_value() {
        let filters = SearchParameterParser::parse_query_to_filters("identifier=http://sys|12345");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::Identifier {
                field,
                system,
                value,
            } => {
                assert_eq!(field, "identifier");
                assert_eq!(system.as_deref(), Some("http://sys"));
                assert_eq!(value, "12345");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

    #[test]
    fn parses_identifier_value_only() {
        let filters = SearchParameterParser::parse_query_to_filters("identifier=999");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::Identifier {
                field,
                system,
                value,
            } => {
                assert_eq!(field, "identifier");
                assert!(system.is_none());
                assert_eq!(value, "999");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

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

    #[test]
    fn maps_name_to_contains_by_default() {
        let filters = SearchParameterParser::parse_query_to_filters("name=John");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::Contains { field, value } => {
                assert_eq!(field, "name");
                assert_eq!(value, "John");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

    #[test]
    fn maps_family_exact_to_exact_filter_on_name_field() {
        let filters = SearchParameterParser::parse_query_to_filters("family:exact=Doe");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::Exact { field, value } => {
                assert_eq!(field, "name");
                assert_eq!(value, "Doe");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests_more {
    use super::*;
    use crate::parameters::SearchModifier;
    use octofhir_storage::legacy::QueryFilter;

    #[test]
    fn parses_comma_separated_id_uses_first_value_in_filter() {
        let filters = SearchParameterParser::parse_query_to_filters("_id=a1,b2,c3");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::Exact { field, value } => {
                assert_eq!(field, "_id");
                assert_eq!(value, "a1");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

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
    fn last_updated_with_ap_prefix_is_ignored_and_no_filter_emitted() {
        let filters = SearchParameterParser::parse_query_to_filters("_lastUpdated=ap2020-01-01");
        assert!(filters.is_empty());
    }

    #[test]
    fn comma_separated_values_with_spaces_are_trimmed() {
        let filters = SearchParameterParser::parse_query_to_filters("_id= a , b ");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::Exact { field, value } => {
                assert_eq!(field, "_id");
                assert_eq!(value, "a");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

    #[test]
    fn plus_is_decoded_to_space() {
        let parsed = SearchParameterParser::parse_query("name=John+Doe");
        assert_eq!(parsed.params.len(), 1);
        let p = &parsed.params[0];
        assert_eq!(p.values[0].raw, "John Doe");
    }

    #[test]
    fn multiple_values_for_name_use_first_in_filter_mapping() {
        let filters = SearchParameterParser::parse_query_to_filters("name=John,Jane");
        assert_eq!(filters.len(), 1);
        match &filters[0] {
            QueryFilter::Contains { field, value } => {
                assert_eq!(field, "name");
                assert_eq!(value, "John");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
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
    fn last_updated_with_ne_prefix_emits_no_filter() {
        let filters = SearchParameterParser::parse_query_to_filters("_lastUpdated=ne2020-01-01");
        assert!(filters.is_empty());
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

impl SearchParameterParser {
    /// Build a SearchQuery from a resource type and URL query string, applying _count defaults/limits.
    pub fn build_search_query(
        resource_type: ResourceType,
        query: &str,
        default_count: usize,
        max_count: usize,
    ) -> SearchQuery {
        let parsed = Self::parse_query(query);
        let filters = parsed.to_filters();
        let count = parsed.parse_count(default_count, max_count);
        let offset = parsed.parse_offset(0);
        let mut q = SearchQuery::new(resource_type).with_pagination(offset, count);
        for f in filters {
            q = q.with_filter(f);
        }
        // Apply sorting if _sort is present (support first occurrence only)
        if let Some(p) = parsed.params.iter().find(|p| p.name == "_sort")
            && let Some(v) = p.values.first()
        {
            let mut ascending = true;
            let mut field = v.raw.as_str();
            // Value form: -field for descending
            if let Some(stripped) = field.strip_prefix('-') {
                ascending = false;
                field = stripped;
            }
            // Modifier form: _sort:desc=field or _sort:asc=field
            if let Some(SearchModifier::Type(m)) = &p.modifier {
                if m.eq_ignore_ascii_case("desc") {
                    ascending = false;
                }
                if m.eq_ignore_ascii_case("asc") {
                    ascending = true;
                }
            }
            if !field.is_empty() {
                q = q.with_sort(field.to_string(), ascending);
            }
        }
        q
    }
}

#[cfg(test)]
mod tests_query_builder {
    use super::*;
    use octofhir_core::ResourceType;
    use octofhir_storage::legacy::QueryFilter;

    #[test]
    fn build_query_uses_default_and_filters() {
        let q =
            SearchParameterParser::build_search_query(ResourceType::Patient, "_id=abc", 10, 100);
        assert_eq!(q.resource_type, ResourceType::Patient);
        assert_eq!(q.count, 10);
        assert_eq!(q.offset, 0);
        assert_eq!(q.filters.len(), 1);
        match &q.filters[0] {
            QueryFilter::Exact { field, value } => {
                assert_eq!(field, "_id");
                assert_eq!(value, "abc");
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

    #[test]
    fn build_query_clamps_count() {
        let q =
            SearchParameterParser::build_search_query(ResourceType::Patient, "_count=250", 10, 100);
        assert_eq!(q.count, 100);
    }

    #[test]
    fn build_query_invalid_count_uses_default() {
        let q = SearchParameterParser::build_search_query(
            ResourceType::Patient,
            "_count=zero",
            10,
            100,
        );
        assert_eq!(q.count, 10);
    }
}

#[cfg(test)]
mod tests_sort {
    use super::*;
    use octofhir_core::ResourceType;

    #[test]
    fn sort_default_asc_with_field_value() {
        let q =
            SearchParameterParser::build_search_query(ResourceType::Patient, "_sort=_id", 10, 100);
        assert_eq!(q.sort_field.as_deref(), Some("_id"));
        assert!(q.sort_ascending);
    }

    #[test]
    fn sort_desc_with_hyphen() {
        let q =
            SearchParameterParser::build_search_query(ResourceType::Patient, "_sort=-_id", 10, 100);
        assert_eq!(q.sort_field.as_deref(), Some("_id"));
        assert!(!q.sort_ascending);
    }

    #[test]
    fn sort_desc_with_modifier_desc() {
        let q = SearchParameterParser::build_search_query(
            ResourceType::Patient,
            "_sort:desc=_lastUpdated",
            10,
            100,
        );
        assert_eq!(q.sort_field.as_deref(), Some("_lastUpdated"));
        assert!(!q.sort_ascending);
    }

    #[test]
    fn sort_asc_with_modifier_asc() {
        let q = SearchParameterParser::build_search_query(
            ResourceType::Patient,
            "_sort:asc=_lastUpdated",
            10,
            100,
        );
        assert_eq!(q.sort_field.as_deref(), Some("_lastUpdated"));
        assert!(q.sort_ascending);
    }
}

impl SearchParameterParser {
    /// Validate parameters and build a SearchQuery in one step.
    pub fn validate_and_build_search_query(
        resource_type: ResourceType,
        query: &str,
        default_count: usize,
        max_count: usize,
        allowed_params: &[&str],
        allowed_sort_fields: &[&str],
    ) -> Result<SearchQuery, SearchValidationError> {
        let parsed = Self::parse_query(query);
        parsed.validate(allowed_params, allowed_sort_fields, max_count)?;
        Ok(Self::build_search_query(
            resource_type,
            query,
            default_count,
            max_count,
        ))
    }
}

#[cfg(test)]
mod tests_validation_and_offset {
    use super::*;
    use octofhir_core::ResourceType;

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
    fn build_query_applies_offset() {
        let q = SearchParameterParser::build_search_query(
            ResourceType::Patient,
            "_offset=20&_count=5",
            10,
            100,
        );
        assert_eq!(q.offset, 20);
        assert_eq!(q.count, 5);
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
