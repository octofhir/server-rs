//! Convert SearchParams to FhirQueryBuilder for search execution.
//!
//! This module bridges the modern `SearchParams` type (from octofhir-storage)
//! to the SQL query builder, enabling search via the FhirStorage trait.

use crate::chaining::{build_chained_search, is_chained_parameter, parse_chained_parameter};
use crate::include::{is_include_parameter, is_revinclude_parameter};
use crate::parameters::SearchPrefix;
use crate::parser::{ParsedParam, ParsedValue};
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::{
    FhirQueryBuilder, IncludeSpec, JsonbPath, RevIncludeSpec, SearchCondition, SortOrder, SortSpec,
    SqlBuilder, SqlBuilderError, SqlValue, fhirpath_to_jsonb_path,
};
use crate::types::dispatch_search;
use octofhir_storage::{SearchParams, TotalMode};
use url::form_urlencoded;

/// How to handle unknown search parameters.
///
/// FHIR servers may choose to either reject unknown parameters (strict)
/// or ignore them and continue (lenient). The behavior is controlled by
/// the `Prefer: handling=strict|lenient` HTTP header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnknownParamHandling {
    /// Reject unknown parameters with 400 Bad Request error.
    Strict,
    /// Ignore unknown parameters and continue with search.
    /// The unknown parameters are collected and can be returned as warnings.
    #[default]
    Lenient,
}

impl UnknownParamHandling {
    /// Parse from Prefer header value (e.g., "handling=strict" or "handling=lenient").
    pub fn from_prefer_header(header: &str) -> Self {
        for part in header.split(';').map(str::trim) {
            if let Some(value) = part.strip_prefix("handling=") {
                match value.trim() {
                    "strict" => return Self::Strict,
                    "lenient" => return Self::Lenient,
                    _ => {}
                }
            }
        }
        Self::default()
    }
}

/// Configuration for search query building.
#[derive(Debug, Clone, Default)]
pub struct SearchConfig {
    /// How to handle unknown search parameters.
    pub unknown_param_handling: UnknownParamHandling,
}

/// Warning for an unknown search parameter.
#[derive(Debug, Clone)]
pub struct UnknownParamWarning {
    /// Name of the unknown parameter.
    pub name: String,
    /// Optional modifier that was specified.
    pub modifier: Option<String>,
}

/// Control parameters that don't generate search conditions.
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

/// Result of converting SearchParams to a query builder.
pub struct ConvertedQuery {
    /// The built FhirQueryBuilder
    pub builder: FhirQueryBuilder,
    /// Include specifications extracted from params
    pub includes: Vec<IncludeSpec>,
    /// RevInclude specifications extracted from params
    pub revincludes: Vec<RevIncludeSpec>,
    /// Whether to return total count
    pub total_mode: Option<TotalMode>,
    /// Unknown parameters that were encountered (when using lenient mode).
    pub unknown_params: Vec<UnknownParamWarning>,
}

/// Convert SearchParams to a FhirQueryBuilder with default (lenient) handling.
///
/// This function:
/// 1. Converts SearchParams.parameters to ParsedParam format
/// 2. Uses dispatch_search() to build SQL conditions
/// 3. Handles _sort, _count, _offset
/// 4. Extracts _include/_revinclude specifications
/// 5. Handles chained parameters
pub fn build_query_from_params(
    resource_type: &str,
    params: &SearchParams,
    registry: &SearchParameterRegistry,
    schema: &str,
) -> Result<ConvertedQuery, SqlBuilderError> {
    build_query_from_params_with_config(
        resource_type,
        params,
        registry,
        schema,
        &SearchConfig::default(),
    )
}

/// Convert SearchParams to a FhirQueryBuilder with configurable unknown parameter handling.
///
/// When `config.unknown_param_handling` is `Strict`, returns an error for unknown parameters.
/// When `Lenient`, unknown parameters are skipped and returned in the result for warning.
pub fn build_query_from_params_with_config(
    resource_type: &str,
    params: &SearchParams,
    registry: &SearchParameterRegistry,
    schema: &str,
    config: &SearchConfig,
) -> Result<ConvertedQuery, SqlBuilderError> {
    let mut builder = FhirQueryBuilder::new(resource_type, schema).with_alias("r");

    // Use SqlBuilder to accumulate conditions, then convert to SearchCondition::Raw
    let mut sql_builder = SqlBuilder::with_resource_column("r.resource");

    // Collect unknown parameters for warnings
    let mut unknown_params = Vec::new();

    // Convert SearchParams.parameters to ParsedParam and process
    for (key, values) in &params.parameters {
        // Skip control parameters
        if is_control_param(key) {
            continue;
        }

        // Handle chained parameters
        if is_chained_parameter(key) {
            handle_chained_param(
                &mut builder,
                &mut sql_builder,
                key,
                values,
                registry,
                schema,
                resource_type,
            )?;
            continue;
        }

        // Convert to ParsedParam format
        let mut parsed = convert_to_parsed_param(key, values);

        // Look up parameter definition in registry
        let Some(param_def) = registry.get(resource_type, &parsed.name) else {
            // Unknown parameter - handle based on policy
            match config.unknown_param_handling {
                UnknownParamHandling::Strict => {
                    return Err(SqlBuilderError::InvalidSearchValue(format!(
                        "Unknown search parameter: {}",
                        key
                    )));
                }
                UnknownParamHandling::Lenient => {
                    tracing::debug!(param = %parsed.name, "Unknown search parameter, skipping");
                    unknown_params.push(UnknownParamWarning {
                        name: parsed.name.clone(),
                        modifier: parsed.modifier.as_ref().map(|m| format!("{:?}", m)),
                    });
                    continue;
                }
            }
        };

        // Validate modifier compatibility with parameter type
        if let Some(ref modifier) = parsed.modifier {
            if !modifier.applicable_to(&param_def.param_type) {
                return Err(SqlBuilderError::InvalidSearchValue(format!(
                    "Modifier ':{:?}' is not valid for {} parameter '{}' (type: {:?})",
                    modifier, resource_type, parsed.name, param_def.param_type
                )));
            }
        }

        // Validate prefix compatibility with parameter type.
        // If a prefix is not applicable (e.g., UUID starting with "eb" parsed as "ends before"
        // for a reference param), revert it — treat the prefix chars as part of the value.
        for value in &mut parsed.values {
            if let Some(ref prefix) = value.prefix {
                if !prefix.applicable_to(&param_def.param_type) {
                    // Restore the prefix as part of the raw value
                    value.raw = format!("{}{}", prefix, value.raw);
                    value.prefix = None;
                }
            }
        }

        // Handle _id specially — it maps to the database column `r.id`, not JSONB
        if parsed.name == "_id" {
            let mut or_conditions = Vec::new();
            for value in &parsed.values {
                if !value.raw.is_empty() {
                    let p = sql_builder.add_text_param(&value.raw);
                    or_conditions.push(format!("r.id = ${p}"));
                }
            }
            if !or_conditions.is_empty() {
                sql_builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
            }
            continue;
        }

        // Use dispatch_search to build the condition
        dispatch_search(&mut sql_builder, &parsed, &param_def, resource_type)?;
    }

    // Add status filter to exclude deleted resources
    sql_builder.add_condition("r.status != 'deleted'");

    // Convert SqlBuilder conditions to SearchCondition::Raw
    if let Some(where_clause) = sql_builder.build_where_clause() {
        let params_vec: Vec<SqlValue> = sql_builder
            .params()
            .iter()
            .map(|p| match p {
                crate::sql_builder::SqlParam::Text(s) => SqlValue::Text(s.clone()),
                crate::sql_builder::SqlParam::Integer(i) => SqlValue::Integer(*i),
                crate::sql_builder::SqlParam::Float(f) => SqlValue::Float(*f),
                crate::sql_builder::SqlParam::Boolean(b) => SqlValue::Boolean(*b),
                crate::sql_builder::SqlParam::Json(s) => SqlValue::Json(s.clone()),
                crate::sql_builder::SqlParam::Timestamp(s) => SqlValue::Timestamp(s.clone()),
            })
            .collect();

        builder = builder.where_condition(SearchCondition::Raw {
            sql: where_clause,
            params: params_vec,
        });
    }

    // Handle pagination
    let limit = params.count.unwrap_or(10) as usize;
    let offset = params.offset.unwrap_or(0) as usize;
    // Request limit + 1 to determine if there are more results
    builder = builder.paginate(limit + 1, offset);

    // Handle sorting
    if let Some(sort_params) = &params.sort {
        for sort_param in sort_params {
            if let Some(sort_spec) = build_sort_spec(
                &sort_param.field,
                sort_param.descending,
                registry,
                resource_type,
            ) {
                builder = builder.sort_by(sort_spec);
            }
        }
    } else {
        // Default sort by _lastUpdated desc
        if let Ok(path) = JsonbPath::new(vec!["meta".to_string(), "lastUpdated".to_string()]) {
            builder = builder.sort_by(SortSpec::desc(path));
        }
    }

    // Extract _include specifications
    let includes = extract_include_specs(params, registry, resource_type);

    // Extract _revinclude specifications
    let revincludes = extract_revinclude_specs(params, registry, resource_type);

    Ok(ConvertedQuery {
        builder,
        includes,
        revincludes,
        total_mode: params.total,
        unknown_params,
    })
}

/// Check if a parameter is a control parameter.
fn is_control_param(name: &str) -> bool {
    CONTROL_PARAMS.contains(&name) || is_include_parameter(name) || is_revinclude_parameter(name)
}

/// Convert a key-value pair to ParsedParam format.
fn convert_to_parsed_param(key: &str, values: &[String]) -> ParsedParam {
    // Parse name and modifier from key (e.g., "name:exact" -> name, Some(Exact))
    let (name, modifier) = if let Some((n, m)) = key.split_once(':') {
        let modifier = match m {
            "exact" => Some(crate::parameters::SearchModifier::Exact),
            "contains" => Some(crate::parameters::SearchModifier::Contains),
            "text" => Some(crate::parameters::SearchModifier::Text),
            "in" => Some(crate::parameters::SearchModifier::In),
            "not-in" => Some(crate::parameters::SearchModifier::NotIn),
            "below" => Some(crate::parameters::SearchModifier::Below),
            "above" => Some(crate::parameters::SearchModifier::Above),
            "not" => Some(crate::parameters::SearchModifier::Not),
            "identifier" => Some(crate::parameters::SearchModifier::Identifier),
            "missing" => Some(crate::parameters::SearchModifier::Missing),
            other if !other.is_empty() => {
                Some(crate::parameters::SearchModifier::Type(other.to_string()))
            }
            _ => None,
        };
        (n.to_string(), modifier)
    } else {
        (key.to_string(), None)
    };

    // Parse values with prefix extraction
    let parsed_values: Vec<ParsedValue> = values
        .iter()
        .flat_map(|v| {
            // Handle comma-separated values (OR semantics within same param)
            v.split(',').map(|part| {
                let part = part.trim();
                let (prefix, raw) = extract_prefix(part);
                ParsedValue {
                    prefix,
                    raw: raw.to_string(),
                }
            })
        })
        .filter(|pv| !pv.raw.is_empty())
        .collect();

    ParsedParam {
        name,
        modifier,
        values: parsed_values,
    }
}

/// Extract prefix from a value string.
fn extract_prefix(value: &str) -> (Option<SearchPrefix>, &str) {
    // Use character iteration to safely handle multi-byte UTF-8 characters.
    // FHIR prefixes are ASCII only (eq, ne, gt, lt, ge, le, sa, eb, ap).
    let mut chars = value.chars();
    if let Some(c1) = chars.next()
        && let Some(c2) = chars.next()
        && c1.is_ascii_lowercase()
        && c2.is_ascii_lowercase()
    {
        let prefix_str: String = [c1, c2].iter().collect();
        if let Some(prefix) = SearchPrefix::parse(&prefix_str) {
            // Safe to slice: we know c1 and c2 are ASCII (1 byte each)
            return (Some(prefix), &value[2..]);
        }
    }
    (None, value)
}

/// Handle chained search parameter (e.g., patient.name=John).
///
/// Uses the `search_idx_reference` index table for B-tree lookups instead of
/// runtime JSONB extraction and CONCAT matching.
fn handle_chained_param(
    _builder: &mut FhirQueryBuilder,
    sql_builder: &mut SqlBuilder,
    key: &str,
    values: &[String],
    registry: &SearchParameterRegistry,
    _schema: &str,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    let value = values.first().map(|s| s.as_str()).unwrap_or("");

    match parse_chained_parameter(key, value, registry, resource_type) {
        Ok(chained) => {
            tracing::debug!(
                chain = ?chained.chain,
                final_param = %chained.final_param,
                "Processing chained parameter"
            );
            build_chained_search(sql_builder, &chained, resource_type)
                .map_err(|e| SqlBuilderError::InvalidSearchValue(e.to_string()))
        }
        Err(e) => {
            tracing::warn!(key = %key, error = %e, "Failed to parse chained parameter");
            Ok(()) // Skip invalid chained parameters
        }
    }
}

/// Build a SortSpec from a sort parameter.
fn build_sort_spec(
    field: &str,
    descending: bool,
    registry: &SearchParameterRegistry,
    resource_type: &str,
) -> Option<SortSpec> {
    // Handle special sort fields
    let path_segments = match field {
        "_lastUpdated" | "_id" => {
            // These are stored directly on the row, not in JSONB
            // Return None and handle separately in the query
            return None;
        }
        _ => {
            // Look up the field in the registry
            let param_def = registry.get(resource_type, field)?;
            let expr = param_def.expression.as_deref()?;
            fhirpath_to_jsonb_path(expr, resource_type)
        }
    };

    let path = JsonbPath::new(path_segments).ok()?;
    let order = if descending {
        SortOrder::Desc
    } else {
        SortOrder::Asc
    };

    Some(SortSpec::new(path, order))
}

/// Extract _include specifications from params.
fn extract_include_specs(
    params: &SearchParams,
    registry: &SearchParameterRegistry,
    _resource_type: &str,
) -> Vec<IncludeSpec> {
    let mut specs = Vec::new();

    if let Some(includes) = params.parameters.get("_include") {
        for value in includes {
            // Format: SourceType:searchParam or SourceType:searchParam:TargetType
            let parts: Vec<&str> = value.split(':').collect();
            if parts.len() >= 2 {
                let source = parts[0];
                let param = parts[1];
                let target = parts.get(2).map(|s| s.to_string());

                let mut spec = IncludeSpec::new(source, param);
                if let Some(t) = target {
                    spec = spec.with_target(t);
                } else if let Some(param_def) = registry.get(source, param) {
                    // Resolve target type from registry when not explicitly provided.
                    // For params with a single target (e.g., patient → Patient), set it.
                    // For multi-target params (e.g., subject → Patient|Group), keep unresolved
                    // and resolve dynamically from index rows at execution time.
                    if param_def.target.len() == 1 {
                        spec = spec.with_target(&param_def.target[0]);
                    }
                }
                specs.push(spec);
            }
        }
    }

    specs
}

/// Extract _revinclude specifications from params.
fn extract_revinclude_specs(
    params: &SearchParams,
    _registry: &SearchParameterRegistry,
    _resource_type: &str,
) -> Vec<RevIncludeSpec> {
    let mut specs = Vec::new();

    if let Some(revincludes) = params.parameters.get("_revinclude") {
        for value in revincludes {
            // Format: SourceType:searchParam or SourceType:searchParam:TargetType
            let parts: Vec<&str> = value.split(':').collect();
            if parts.len() >= 2 {
                let source = parts[0];
                let param = parts[1];
                let target = parts.get(2).map(|s| s.to_string());

                let mut spec = RevIncludeSpec::new(source, param);
                if let Some(t) = target {
                    spec = spec.with_target(t);
                }
                specs.push(spec);
            }
        }
    }

    specs
}

/// Parse a URL query string into SearchParams.
///
/// This converts a query string like `name=John&birthdate=ge2000-01-01&_count=10`
/// into a `SearchParams` struct suitable for `FhirStorage::search()`.
///
/// # Arguments
///
/// * `query` - URL-encoded query string (without leading `?`)
/// * `default_count` - Default page size if `_count` not specified
/// * `max_count` - Maximum allowed page size
///
/// # Returns
///
/// Returns a `SearchParams` struct with parsed parameters.
pub fn parse_query_string(query: &str, default_count: u32, max_count: u32) -> SearchParams {
    let mut params = SearchParams::new();

    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let key = key.to_string();
        let value = value.to_string();

        match key.as_str() {
            "_count" => {
                if let Ok(n) = value.parse::<u32>() {
                    let count = n.min(max_count).max(1);
                    params = params.with_count(count);
                }
            }
            "_offset" => {
                if let Ok(n) = value.parse::<u32>() {
                    params = params.with_offset(n);
                }
            }
            "_sort" => {
                // Parse sort string: "-date,name" -> sort by date desc, name asc
                for sort_field in value.split(',') {
                    let sort_field = sort_field.trim();
                    if let Some(field) = sort_field.strip_prefix('-') {
                        params = params.with_sort(field, true);
                    } else {
                        params = params.with_sort(sort_field, false);
                    }
                }
            }
            "_total" => match value.as_str() {
                "accurate" => params = params.with_total(TotalMode::Accurate),
                "estimate" => params = params.with_total(TotalMode::Estimate),
                "none" => params = params.with_total(TotalMode::None),
                _ => {}
            },
            _ => {
                // Regular search parameter - add to parameters map
                params = params.with_param(&key, &value);
            }
        }
    }

    // Apply default count if not specified
    if params.count.is_none() {
        params = params.with_count(default_count);
    }

    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_to_parsed_param_simple() {
        let parsed = convert_to_parsed_param("name", &["John".to_string()]);
        assert_eq!(parsed.name, "name");
        assert!(parsed.modifier.is_none());
        assert_eq!(parsed.values.len(), 1);
        assert_eq!(parsed.values[0].raw, "John");
    }

    #[test]
    fn test_convert_to_parsed_param_with_modifier() {
        let parsed = convert_to_parsed_param("name:exact", &["John".to_string()]);
        assert_eq!(parsed.name, "name");
        assert!(matches!(
            parsed.modifier,
            Some(crate::parameters::SearchModifier::Exact)
        ));
    }

    #[test]
    fn test_convert_to_parsed_param_with_prefix() {
        let parsed = convert_to_parsed_param("birthdate", &["ge2000-01-01".to_string()]);
        assert_eq!(parsed.name, "birthdate");
        assert_eq!(parsed.values.len(), 1);
        assert_eq!(parsed.values[0].prefix, Some(SearchPrefix::Ge));
        assert_eq!(parsed.values[0].raw, "2000-01-01");
    }

    #[test]
    fn test_convert_to_parsed_param_comma_separated() {
        let parsed = convert_to_parsed_param("status", &["active,completed".to_string()]);
        assert_eq!(parsed.values.len(), 2);
        assert_eq!(parsed.values[0].raw, "active");
        assert_eq!(parsed.values[1].raw, "completed");
    }

    #[test]
    fn test_is_control_param() {
        assert!(is_control_param("_count"));
        assert!(is_control_param("_offset"));
        assert!(is_control_param("_include"));
        assert!(!is_control_param("name"));
        assert!(!is_control_param("birthdate"));
    }

    #[test]
    fn test_extract_prefix() {
        assert_eq!(extract_prefix("ge2000"), (Some(SearchPrefix::Ge), "2000"));
        assert_eq!(extract_prefix("le100"), (Some(SearchPrefix::Le), "100"));
        assert_eq!(extract_prefix("John"), (None, "John"));
    }

    #[test]
    fn test_parse_query_string_include() {
        let params =
            parse_query_string("code=8867-4&_include=Observation:subject&_count=5", 10, 100);
        // _include should be in the parameters map
        assert!(
            params.parameters.contains_key("_include"),
            "params.parameters should contain _include, got keys: {:?}",
            params.parameters.keys().collect::<Vec<_>>()
        );
        let include_values = params.parameters.get("_include").unwrap();
        assert_eq!(include_values, &["Observation:subject"]);
    }

    #[test]
    fn test_extract_include_specs_from_params() {
        let registry = SearchParameterRegistry::new();
        let params =
            parse_query_string("code=8867-4&_include=Observation:subject&_count=5", 10, 100);
        let specs = extract_include_specs(&params, &registry, "Observation");
        assert!(
            !specs.is_empty(),
            "include specs should not be empty, params had keys: {:?}",
            params.parameters.keys().collect::<Vec<_>>()
        );
        assert_eq!(specs[0].source_type, "Observation");
        assert_eq!(specs[0].param_name, "subject");
    }

    #[test]
    fn test_extract_revinclude_specs_from_params() {
        let registry = SearchParameterRegistry::new();
        let params = parse_query_string(
            "family=Smith&_revinclude=Observation:subject&_count=5",
            10,
            100,
        );
        let specs = extract_revinclude_specs(&params, &registry, "Patient");
        assert!(!specs.is_empty(), "revinclude specs should not be empty");
        assert_eq!(specs[0].source_type, "Observation");
        assert_eq!(specs[0].param_name, "subject");
    }

    #[test]
    fn test_build_query_includes_populated() {
        let registry = SearchParameterRegistry::new();
        let params =
            parse_query_string("code=8867-4&_include=Observation:subject&_count=5", 10, 100);
        let converted =
            build_query_from_params("Observation", &params, &registry, "public").unwrap();
        assert!(
            !converted.includes.is_empty(),
            "converted.includes should not be empty"
        );
        assert_eq!(converted.includes[0].source_type, "Observation");
        assert_eq!(converted.includes[0].param_name, "subject");
    }

    #[test]
    fn test_identifier_system_value_through_query_builder() {
        use crate::parameters::{ElementTypeHint, SearchParameter, SearchParameterType};
        use std::sync::Arc;

        // Set up registry with identifier search parameter
        let mut registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "identifier",
                "http://hl7.org/fhir/SearchParameter/Patient-identifier",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.identifier")
            .with_element_type_hint(ElementTypeHint::Array("Identifier".to_string())),
        );

        let params = parse_query_string("identifier=http://test.org|debug-123&_count=5", 10, 100);

        let converted = build_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        // Should contain @> containment for identifier system|value
        assert!(
            built.sql.contains("@>"),
            "Expected @> containment in SQL for identifier system|value, got: {}",
            built.sql
        );
        // Check that the param is the right JSON
        let json_params: Vec<_> = built
            .params
            .iter()
            .filter(|p| matches!(p, SqlValue::Json(_)))
            .collect();
        assert!(
            !json_params.is_empty(),
            "Expected at least one JSON param for identifier containment"
        );
        if let SqlValue::Json(j) = &json_params[0] {
            assert!(
                j.contains("http://test.org") && j.contains("debug-123"),
                "Expected JSON with system/value, got: {j}"
            );
        }
    }

    #[test]
    fn test_identifier_without_element_hint_still_works() {
        // Test fallback: even without element_type_hint, identifier search
        // should use identifier-specific containment (not coding wrapper)
        let mut registry = SearchParameterRegistry::new();
        registry.register(
            crate::parameters::SearchParameter::new(
                "identifier",
                "http://hl7.org/fhir/SearchParameter/Patient-identifier",
                crate::parameters::SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.identifier"),
            // Note: no element_type_hint — defaults to Unknown
        );

        let params = parse_query_string("identifier=http://test.org|debug-123&_count=5", 10, 100);

        let converted = build_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        // Should use @> containment with identifier-style JSON (system/value, NOT coding)
        assert!(built.sql.contains("@>"), "Expected @> containment");
        let json_params: Vec<_> = built
            .params
            .iter()
            .filter_map(|p| match p {
                SqlValue::Json(j) => Some(j.as_str()),
                _ => None,
            })
            .collect();
        assert!(!json_params.is_empty(), "Expected JSON param");
        assert!(
            json_params[0].contains("\"value\"") && json_params[0].contains("\"system\""),
            "Expected identifier JSON with system/value, got: {}",
            json_params[0]
        );
        assert!(
            !json_params[0].contains("coding"),
            "Should NOT contain 'coding' for identifier search, got: {}",
            json_params[0]
        );
    }
}
