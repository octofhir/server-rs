//! Search type implementations for FHIR search parameters.
//!
//! This module provides implementations for FHIR search parameter types:
//! - String: Case-insensitive text search with modifiers
//! - Token: Coded value search (Coding, CodeableConcept, Identifier)
//! - Number: Numeric search with comparison prefixes
//! - Date: Date/datetime search with precision handling
//! - Reference: Reference search with type modifiers
//! - URI: URI search with hierarchical modifiers
//! - Composite: Combined search on multiple components
//! - Special: Location-based (_near), full-text (_text, _content), and advanced search
//!
//! Each type module provides functions to build SQL conditions for PostgreSQL JSONB queries.

pub mod composite;
pub mod date;
pub mod date_ast;
pub mod number;
pub mod special;
pub mod string;
pub mod token;
pub mod uri;

pub use composite::CompositeComponent;
pub use composite::{CompositeValue, parse_composite_value};
pub use composite::{
    build_composite_search, build_composite_search_with_specs,
    build_composite_search_with_specs_jsonb_fallback,
};
#[cfg(test)]
pub use date::build_period_search;
pub use date::{
    DateRange, build_date_search, build_indexed_date_inplace, parse_date_range,
};
pub use number::{
    build_gin_quantity_search, build_number_search, build_quantity_search,
};
pub use special::{
    NearParameter, SpecialParameterType, build_content_search, build_filter_search,
    build_list_search, build_near_search, build_text_search, detect_special_type,
    parse_near_parameter,
};
pub use string::{build_indexed_string_inplace};
pub use string::{build_array_string_search, build_human_name_search, build_string_search};
#[cfg(test)]
pub use token::build_token_search_with_terminology;
pub use token::{
    build_code_search, build_gin_code_search, build_gin_identifier_search, build_gin_token_search,
    build_identifier_search, build_token_coding_array_search, build_token_search, parse_token_value,
};
pub use uri::{build_uri_array_search, build_uri_search};

use crate::ir::{
    ResourceColumnParam, render_date_column_clauses_as_or, resolve_composite_component_specs,
    resolve_resource_column_param,
};
use crate::parameters::{ElementTypeHint, SearchModifier, SearchParameter, SearchParameterType};
use crate::parser::ParsedParam;
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::{
    SqlBuilder, SqlBuilderError, build_jsonb_accessor, fhirpath_to_jsonb_path,
};
use std::sync::Arc;

/// Dispatch a search parameter to the appropriate type handler.
///
/// This function determines the correct search type handler based on the
/// parameter definition and generates the appropriate SQL conditions.
pub fn dispatch_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    definition: &Arc<SearchParameter>,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    dispatch_search_inner(builder, param, definition, resource_type, None)
}

/// Dispatch a search parameter with access to the full SearchParameter registry.
///
/// Composite parameters need this to resolve component `definition` URLs to
/// their real SearchParameter types instead of guessing from expression text.
pub fn dispatch_search_with_registry(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    definition: &Arc<SearchParameter>,
    resource_type: &str,
    registry: &SearchParameterRegistry,
) -> Result<(), SqlBuilderError> {
    dispatch_search_inner(builder, param, definition, resource_type, Some(registry))
}

fn dispatch_search_inner(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    definition: &Arc<SearchParameter>,
    resource_type: &str,
    registry: Option<&SearchParameterRegistry>,
) -> Result<(), SqlBuilderError> {
    // Get the FHIRPath expression and convert to JSONB path
    let expression = definition.expression.as_deref().ok_or_else(|| {
        SqlBuilderError::InvalidPath(format!(
            "No expression for search parameter: {}",
            definition.code
        ))
    })?;

    let path_segments = fhirpath_to_jsonb_path(expression, resource_type);

    // Determine if we need text extraction (->>) or JSON traversal (->).
    // URI/canonical/url are stored as JSON strings — for `=`, `LIKE`, and
    // `LOWER(...)` to all work without explicit casts, extract the leaf as
    // text. Date/Number/String already follow this convention.
    let needs_text = matches!(
        definition.param_type,
        SearchParameterType::String
            | SearchParameterType::Number
            | SearchParameterType::Date
            | SearchParameterType::Uri
    );

    let jsonb_path = build_jsonb_accessor(builder.resource_column(), &path_segments, needs_text);

    // String/Date/Number/Quantity/Reference search as in-place predicates on the
    // resource JSONB via the shared jsonb dispatcher (no sidecar tables).
    if definition.user_defined
        || matches!(
            definition.param_type,
            SearchParameterType::String
                | SearchParameterType::Date
                | SearchParameterType::Number
                | SearchParameterType::Quantity
                | SearchParameterType::Reference
        )
    {
        if matches!(definition.param_type, SearchParameterType::Date) {
            // meta.lastUpdated is a row column, not a jsonb date path.
            if matches!(
                resolve_resource_column_param(definition),
                Some(ResourceColumnParam::LastUpdated)
            ) {
                return build_last_updated_search(builder, param);
            }
            return build_indexed_date_inplace(builder, param, resource_type, definition);
        }
        if matches!(definition.param_type, SearchParameterType::String) {
            return build_indexed_string_inplace(builder, param, resource_type, definition);
        }
        return dispatch_user_defined_jsonb_search(
            builder,
            param,
            definition,
            resource_type,
            registry,
            &path_segments,
            &jsonb_path,
        );
    }

    reject_unsupported_production_path(param, definition, resource_type)?;

    // String/Date/Number/Quantity/Reference are always handled in place by the
    // early branch above; only token/uri/composite/special reach here.
    match definition.param_type {
        SearchParameterType::Token => {
            if definition.element_type_hint.is_identifier()
                || (matches!(&definition.element_type_hint, ElementTypeHint::Unknown)
                    && is_identifier_param(&definition.code, expression))
            {
                // Identifier search uses full-resource containment for GIN-friendly forms.
                // Also detect identifier params when element_type_hint is Unknown
                // (e.g. schema resolver unavailable) by checking param code / expression.
                build_gin_identifier_search(builder, param, &path_segments)
            } else if matches!(&definition.element_type_hint, ElementTypeHint::SimpleCode) {
                // GIN-optimized simple code search (e.g., Patient.gender)
                build_gin_code_search(builder, param, &path_segments)
            } else if matches!(
                &definition.element_type_hint,
                ElementTypeHint::Array(t) if t == "CodeableConcept" || t == "Coding"
            ) {
                // Array-valued CodeableConcept/Coding (e.g. Observation.category 0..*):
                // scalar containment misses arrays — use array-aware render.
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_token_coding_array_search(builder, param, &array_path)
            } else {
                // GIN-optimized CodeableConcept/Coding/Token search
                build_gin_token_search(builder, param, &path_segments)
            }
        }

        SearchParameterType::Composite => {
            // Build composite search using component definitions
            if definition.component.is_empty() {
                return Err(SqlBuilderError::InvalidPath(format!(
                    "Composite search parameter {} has no components defined",
                    definition.code
                )));
            }

            let Some(registry) = registry else {
                return Err(SqlBuilderError::InvalidPath(
                    "Composite component type resolution requires SearchParameterRegistry"
                        .to_string(),
                ));
            };
            let components =
                resolve_composite_component_specs(registry, resource_type, definition)?;
            build_composite_search_with_specs(builder, param, resource_type, &components)
        }

        SearchParameterType::Uri => match &definition.element_type_hint {
            ElementTypeHint::Array(_) => {
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_uri_array_search(builder, param, &array_path)
            }
            _ => build_uri_search(builder, param, &jsonb_path),
        },

        SearchParameterType::Special => {
            // Special parameters are usually handled by name, not by expression
            // Detect special type and dispatch accordingly
            match detect_special_type(&param.name) {
                Some(SpecialParameterType::Near) => {
                    let json_path =
                        build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                    if let Some(value) = param.values.first() {
                        build_near_search(builder, &value.raw, &json_path)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_near requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::Text) => {
                    if let Some(value) = param.values.first() {
                        build_text_search(builder, &value.raw)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_text requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::Content) => {
                    if let Some(value) = param.values.first() {
                        build_content_search(builder, &value.raw)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_content requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::Filter) => {
                    if let Some(value) = param.values.first() {
                        build_filter_search(builder, &value.raw, resource_type)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_filter requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::List) => {
                    if let Some(value) = param.values.first() {
                        build_list_search(builder, &value.raw, resource_type)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_list requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::Query) => Err(SqlBuilderError::NotImplemented(
                    "_query search not yet implemented".to_string(),
                )),
                None => Err(SqlBuilderError::NotImplemented(format!(
                    "Unknown special parameter: {}",
                    param.name
                ))),
            }
        }

        SearchParameterType::String
        | SearchParameterType::Number
        | SearchParameterType::Date
        | SearchParameterType::Quantity
        | SearchParameterType::Reference => {
            unreachable!("handled in place by the early branch in dispatch_search_inner")
        }
    }
}

fn dispatch_user_defined_jsonb_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    definition: &SearchParameter,
    resource_type: &str,
    registry: Option<&SearchParameterRegistry>,
    path_segments: &[String],
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    let object_path = build_jsonb_accessor(builder.resource_column(), path_segments, false);
    let text_path = build_jsonb_accessor(builder.resource_column(), path_segments, true);

    match definition.param_type {
        SearchParameterType::String => {
            if definition.element_type_hint.is_human_name() && path_segments.len() == 1 {
                build_human_name_search(builder, param, &object_path)
            } else if matches!(definition.element_type_hint, ElementTypeHint::Array(_))
                && path_segments.len() > 1
            {
                let array_path = build_jsonb_accessor(
                    builder.resource_column(),
                    &path_segments[..path_segments.len() - 1],
                    false,
                );
                let field_name = path_segments.last().expect("checked non-empty");
                build_array_string_search(builder, param, &array_path, field_name)
            } else {
                build_string_search(builder, param, jsonb_path)
            }
        }
        SearchParameterType::Token => {
            if definition.element_type_hint.is_identifier()
                || (matches!(&definition.element_type_hint, ElementTypeHint::Unknown)
                    && definition
                        .expression
                        .as_deref()
                        .is_some_and(|expr| is_identifier_param(&definition.code, expr)))
            {
                build_identifier_search(builder, param, &object_path)
            } else if matches!(&definition.element_type_hint, ElementTypeHint::SimpleCode) {
                build_code_search(builder, param, &text_path)
            } else if matches!(
                &definition.element_type_hint,
                ElementTypeHint::Array(t) if t == "CodeableConcept" || t == "Coding"
            ) {
                // Array-valued CodeableConcept/Coding: scalar token render misses
                // the outer array — use array-aware render (see build_token_coding_array_search).
                build_token_coding_array_search(builder, param, &object_path)
            } else {
                build_token_search(builder, param, &object_path)
            }
        }
        SearchParameterType::Number => build_number_search(builder, param, jsonb_path),
        SearchParameterType::Date => build_date_search(builder, param, jsonb_path),
        SearchParameterType::Quantity => {
            build_gin_quantity_search(builder, param, &object_path, path_segments)
        }
        SearchParameterType::Reference => {
            build_reference_jsonb_fallback_search(builder, param, &object_path, &definition.target)
        }
        SearchParameterType::Composite => {
            if definition.component.is_empty() {
                return Err(SqlBuilderError::InvalidPath(format!(
                    "Composite search parameter {} has no components defined",
                    definition.code
                )));
            }

            let Some(registry) = registry else {
                return Err(SqlBuilderError::InvalidPath(
                    "Custom composite component type resolution requires SearchParameterRegistry"
                        .to_string(),
                ));
            };
            let components =
                resolve_composite_component_specs(registry, resource_type, definition)?;
            build_composite_search_with_specs_jsonb_fallback(
                builder,
                param,
                resource_type,
                &components,
            )
        }
        SearchParameterType::Uri => match &definition.element_type_hint {
            ElementTypeHint::Array(_) => build_uri_array_search(builder, param, &object_path),
            _ => build_uri_search(builder, param, &text_path),
        },
        SearchParameterType::Special => Err(SqlBuilderError::NotImplemented(format!(
            "User-defined special SearchParameter '{}' is not supported",
            definition.url
        ))),
    }
}

fn build_reference_jsonb_fallback_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    json_path: &str,
    target_types: &[String],
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    if matches!(param.modifier, Some(SearchModifier::Identifier)) {
        // `reference:identifier` matches the embedded `.identifier` element as a
        // plain token; the `:identifier` modifier has already been consumed by
        // routing here, so drop it before delegating to the token identifier path
        // (which only accepts token modifiers).
        let identifier_param = ParsedParam {
            name: param.name.clone(),
            modifier: None,
            values: param.values.clone(),
        };
        return build_identifier_search(
            builder,
            &identifier_param,
            &format!("{json_path}->'identifier'"),
        );
    }

    if matches!(param.modifier, Some(SearchModifier::Missing)) {
        let is_missing = param
            .values
            .first()
            .map(|v| v.raw.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let condition = if is_missing {
            format!(
                "({json_path} IS NULL OR {json_path} = 'null'::jsonb OR {json_path} = '[]'::jsonb)"
            )
        } else {
            format!(
                "({json_path} IS NOT NULL AND {json_path} != 'null'::jsonb AND {json_path} != '[]'::jsonb)"
            )
        };
        builder.add_raw_condition(condition);
        return Ok(());
    }

    let type_modifier = match &param.modifier {
        Some(SearchModifier::Type(resource_type)) => Some(resource_type.as_str()),
        None => None,
        Some(other) => {
            return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
        }
    };

    let mut or_conditions: Vec<crate::ir::sql::SqlExpr> = Vec::new();
    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let references = reference_fallback_candidates(&value.raw, type_modifier, target_types);
        let mut value_conditions: Vec<crate::ir::sql::SqlExpr> = Vec::new();
        for reference in references {
            let p = builder.add_text_param(reference);
            value_conditions.push(crate::ir::sql::SqlExpr::Raw(format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements({}) AS ref WHERE ref->>'reference' = ${p})",
                jsonb_array_or_singleton(json_path)
            )));
        }

        match value_conditions.len() {
            0 => {}
            1 => or_conditions.push(value_conditions.pop().unwrap()),
            _ => or_conditions.push(crate::ir::sql::SqlExpr::Or(value_conditions)),
        }
    }

    match or_conditions.len() {
        0 => {}
        1 => builder.add_condition(or_conditions.pop().unwrap()),
        _ => builder.add_condition(crate::ir::sql::SqlExpr::Or(or_conditions)),
    }
    Ok(())
}

fn reference_fallback_candidates(
    raw: &str,
    type_modifier: Option<&str>,
    target_types: &[String],
) -> Vec<String> {
    if raw.contains('/') || raw.starts_with("http://") || raw.starts_with("https://") {
        return vec![raw.to_string()];
    }

    if let Some(resource_type) = type_modifier {
        return vec![format!("{resource_type}/{raw}")];
    }

    if target_types.len() == 1 {
        return vec![format!("{}/{raw}", target_types[0]), raw.to_string()];
    }

    vec![raw.to_string()]
}

fn jsonb_array_or_singleton(path: &str) -> String {
    format!(
        "CASE WHEN jsonb_typeof({path}) = 'array' THEN {path} WHEN {path} IS NULL THEN '[]'::jsonb ELSE jsonb_build_array({path}) END"
    )
}

pub(crate) fn reject_unsupported_production_path(
    parsed: &ParsedParam,
    param_def: &SearchParameter,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    match param_def.param_type {
        SearchParameterType::String if matches!(parsed.modifier, Some(SearchModifier::Text)) => {
            Err(production_path_disabled(
                resource_type,
                &parsed.name,
                "string :text narrative JSONB traversal",
            ))
        }
        SearchParameterType::Token if matches!(parsed.modifier, Some(SearchModifier::Text)) => Err(
            production_path_disabled(resource_type, &parsed.name, "token :text display traversal"),
        ),
        _ => Ok(()),
    }
}

fn production_path_disabled(resource_type: &str, param_name: &str, path: &str) -> SqlBuilderError {
    SqlBuilderError::NotImplemented(format!(
        "{resource_type}.{param_name} requires {path}, but production fallback search paths are disabled"
    ))
}

/// Build a date search against the `updated_at` system column (used for the
/// `_lastUpdated` search parameter). The column is a timestamptz, so we can
/// compare it directly against a timestamptz bind parameter.
fn build_last_updated_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
) -> Result<(), SqlBuilderError> {
    use crate::types::date_ast::DateClause;

    if param.values.is_empty() {
        return Ok(());
    }

    // Determine the column accessor — match the alias used by the FhirQueryBuilder.
    let column = {
        let resource_col = builder.resource_column();
        if let Some(dot_idx) = resource_col.find('.') {
            format!("{}.updated_at", &resource_col[..dot_idx])
        } else {
            "updated_at".to_string()
        }
    };

    // Handle :missing modifier — updated_at is always set, so :missing=true
    // matches nothing and :missing=false matches everything.
    if let Some(SearchModifier::Missing) = &param.modifier {
        let is_missing = param
            .values
            .first()
            .map(|v| v.raw.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let condition = if is_missing {
            format!("({column} IS NULL)")
        } else {
            format!("({column} IS NOT NULL)")
        };
        builder.add_raw_condition(condition);
        return Ok(());
    }

    let clauses = DateClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_date_column_clauses_as_or(builder, &clauses, &column) {
        builder.add_condition(sql);
    }

    Ok(())
}

/// Build GIN-optimized `:exact` string search using `resource @> '{...}'::jsonb`.
///
/// Uses the `@>` containment operator to leverage the existing GIN index
/// (`jsonb_path_ops`) on the resource column. Handles three cases:
///
/// - **HumanName** (path=`["name"]`): OR of containments for family/text/given
/// - **Array string** (e.g. `["name","family"]`): containment wrapping in array
/// - **Simple string** (e.g. `["gender"]`): direct containment
#[cfg(test)]
fn build_gin_exact_string_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    path_segments: &[String],
    element_type_hint: &ElementTypeHint,
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    let resource_col = builder.resource_column().to_string();
    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let condition = if element_type_hint.is_human_name() && path_segments.len() == 1 {
            // HumanName root path (e.g., `name` → Patient.name). Builds the
            // family/text/given OR. For sub-field paths (e.g., `family` →
            // Patient.name.family) the regular array branch produces correct
            // containment `{name:[{family:"…"}]}`; HumanName branch would
            // incorrectly wrap the sub-field into both levels.
            build_gin_human_name_exact(builder, &resource_col, path_segments, &value.raw)
        } else if matches!(element_type_hint, ElementTypeHint::Array(_)) || path_segments.len() > 1
        {
            // Array string field (e.g., HumanName.family inside Patient.name[]).
            // Split into array path and field, then OR two containments:
            //   {"name":[{"given":"Alex"}]}      — for scalar leaf fields
            //   {"name":[{"given":["Alex"]}]}    — for array-of-strings leaves
            //
            // Both shapes occur in FHIR (family is scalar, given is array).
            // OR-ing keeps the GIN @> index usable while staying correct without
            // per-field schema lookup. Postgres' "array contains scalar"
            // exception (jsonb docs) makes the scalar form work for arrays in
            // most cases, but nested containment evaluation is finicky so the
            // array form is included explicitly.
            let (array_segments, field) = split_array_path(path_segments);
            if field.is_empty() {
                let containment = build_string_nested_containment(
                    &array_segments,
                    serde_json::json!([&value.raw]),
                );
                let p = builder.add_json_param(containment.to_string());
                format!("{resource_col} @> ${p}::jsonb")
            } else {
                let scalar_obj = serde_json::json!([{field.as_str(): &value.raw}]);
                let scalar_containment =
                    build_string_nested_containment(&array_segments, scalar_obj);
                let p_scalar = builder.add_json_param(scalar_containment.to_string());

                let array_obj = serde_json::json!([{field.as_str(): [&value.raw]}]);
                let array_containment = build_string_nested_containment(&array_segments, array_obj);
                let p_array = builder.add_json_param(array_containment.to_string());

                format!(
                    "({resource_col} @> ${p_scalar}::jsonb OR {resource_col} @> ${p_array}::jsonb)"
                )
            }
        } else {
            // Simple string field: {"gender": "female"}
            let containment =
                build_string_nested_containment(path_segments, serde_json::json!(&value.raw));
            let json_str = containment.to_string();
            let p = builder.add_json_param(&json_str);
            format!("{resource_col} @> ${p}::jsonb")
        };

        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_raw_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

/// Build GIN containment conditions for HumanName :exact search.
///
/// Produces an OR of three containment checks:
/// - `resource @> '{"name": [{"family": "Smith"}]}'::jsonb`
/// - `resource @> '{"name": [{"text": "Smith"}]}'::jsonb`
/// - `resource @> '{"name": [{"given": ["Smith"]}]}'::jsonb`
#[cfg(test)]
fn build_gin_human_name_exact(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    value: &str,
) -> String {
    let family_obj = serde_json::json!([{"family": value}]);
    let family_containment = build_string_nested_containment(path_segments, family_obj);
    let p1 = builder.add_json_param(family_containment.to_string());

    let text_obj = serde_json::json!([{"text": value}]);
    let text_containment = build_string_nested_containment(path_segments, text_obj);
    let p2 = builder.add_json_param(text_containment.to_string());

    let given_obj = serde_json::json!([{"given": [value]}]);
    let given_containment = build_string_nested_containment(path_segments, given_obj);
    let p3 = builder.add_json_param(given_containment.to_string());

    format!(
        "({resource_col} @> ${p1}::jsonb OR {resource_col} @> ${p2}::jsonb OR {resource_col} @> ${p3}::jsonb)"
    )
}

/// Build a nested JSON object from path segments wrapping a leaf value (for string search).
#[cfg(test)]
fn build_string_nested_containment(
    path_segments: &[String],
    leaf_value: serde_json::Value,
) -> serde_json::Value {
    let mut result = leaf_value;
    for segment in path_segments.iter().rev() {
        result = serde_json::json!({ segment.as_str(): result });
    }
    result
}

/// Split a path into array path and field name.
#[cfg(test)]
fn split_array_path(path: &[String]) -> (Vec<String>, String) {
    if path.len() > 1 {
        let array_path = path[..path.len() - 1].to_vec();
        let field = path.last().unwrap().clone();
        (array_path, field)
    } else {
        (path.to_vec(), String::new())
    }
}

/// Detect identifier params when element_type_hint is Unknown.
///
/// Heuristic: if the param code is "identifier" or the FHIRPath expression
/// ends with ".identifier", this is likely an Identifier-type field.
fn is_identifier_param(code: &str, expression: &str) -> bool {
    code == "identifier" || expression.ends_with(".identifier")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::SearchPrefix;
    use crate::parser::ParsedValue;

    #[test]
    fn test_dispatch_string_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "family".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "family",
                "http://hl7.org/fhir/SearchParameter/Patient-family",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name.family"),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
    }

    #[test]
    fn test_dispatch_token_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "gender".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "female".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "gender",
                "http://hl7.org/fhir/SearchParameter/Patient-gender",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.gender")
            .with_element_type_hint(ElementTypeHint::SimpleCode),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        let clause_str = clause.unwrap();
        // GIN-optimized: should use @> containment operator
        assert!(
            clause_str.contains("@>"),
            "Expected GIN containment (@>), got: {clause_str}"
        );
    }

    #[test]
    fn test_dispatch_date_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "birthdate".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "2000-01-01".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "birthdate",
                "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
                SearchParameterType::Date,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.birthDate"),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
    }

    #[test]
    fn test_dispatch_last_updated_ne_uses_column_ir_without_not() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let param = ParsedParam {
            name: "_lastUpdated".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: Some(SearchPrefix::Ne),
                raw: "2024-06-15".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "_lastUpdated",
                "http://hl7.org/fhir/SearchParameter/Resource-lastUpdated",
                SearchParameterType::Date,
                vec!["Patient".to_string()],
            )
            .with_expression("Resource.meta.lastUpdated"),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert_eq!(
            clause,
            "(r.updated_at < $1::timestamptz OR r.updated_at >= $2::timestamptz)"
        );
        assert!(!clause.contains("NOT"));
    }

    #[test]
    fn test_dispatch_no_expression_returns_error() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "test".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "value".to_string(),
            }],
        };
        let def = Arc::new(SearchParameter::new(
            "test",
            "http://example.org/test",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        ));

        let result = dispatch_search(&mut builder, &param, &def, "Patient");
        assert!(result.is_err());
    }

    #[test]
    fn test_dispatch_reference_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "subject".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "Patient/123".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "subject",
                "http://hl7.org/fhir/SearchParameter/Observation-subject",
                SearchParameterType::Reference,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.subject")
            .with_targets(vec!["Patient".to_string(), "Group".to_string()]),
        );

        dispatch_search(&mut builder, &param, &def, "Observation").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        let clause_str = clause.unwrap();
        assert!(clause_str.contains("ref->>'reference'"));
        assert!(clause_str.contains("jsonb_array_elements"));
    }

    #[test]
    fn test_dispatch_uri_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "url".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "http://example.org/fhir/StructureDefinition/patient".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "url",
                "http://hl7.org/fhir/SearchParameter/StructureDefinition-url",
                SearchParameterType::Uri,
                vec!["StructureDefinition".to_string()],
            )
            .with_expression("StructureDefinition.url"),
        );

        dispatch_search(&mut builder, &param, &def, "StructureDefinition").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
    }

    #[test]
    fn test_dispatch_composite_search() {
        use crate::parameters::SearchParameterComponent;

        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "code",
                "http://hl7.org/fhir/SearchParameter/Observation-code",
                SearchParameterType::Token,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.code"),
        );
        registry.register(
            SearchParameter::new(
                "value-quantity",
                "http://hl7.org/fhir/SearchParameter/Observation-value-quantity",
                SearchParameterType::Quantity,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.valueQuantity"),
        );

        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "code-value-quantity".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "http://loinc.org|8480-6$gt100".to_string(),
            }],
        };

        let def = Arc::new(
            SearchParameter::new(
                "code-value-quantity",
                "http://hl7.org/fhir/SearchParameter/Observation-code-value-quantity",
                SearchParameterType::Composite,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation")
            .with_components(vec![
                SearchParameterComponent {
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-code".to_string(),
                    expression: "code".to_string(),
                },
                SearchParameterComponent {
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-value-quantity"
                        .to_string(),
                    expression: "valueQuantity".to_string(),
                },
            ]),
        );

        dispatch_search_with_registry(&mut builder, &param, &def, "Observation", &registry)
            .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("(resource->'valueQuantity'->>'value')::numeric > "));
        assert!(clause.contains("resource->>'code'"));
        assert!(!clause.contains("jsonb_array_elements"));
    }

    #[test]
    fn test_dispatch_composite_no_components_fails() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "test-composite".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "value1$value2".to_string(),
            }],
        };

        let def = Arc::new(
            SearchParameter::new(
                "test-composite",
                "http://hl7.org/fhir/SearchParameter/Patient-test-composite",
                SearchParameterType::Composite,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient"),
        );

        let result = dispatch_search(&mut builder, &param, &def, "Patient");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no components defined")
        );
    }

    #[test]
    fn test_dispatch_composite_requires_registry_for_component_types() {
        use crate::parameters::SearchParameterComponent;

        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "code-value-quantity".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "http://loinc.org|8480-6$gt100".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "code-value-quantity",
                "http://hl7.org/fhir/SearchParameter/Observation-code-value-quantity",
                SearchParameterType::Composite,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation")
            .with_components(vec![SearchParameterComponent {
                definition: "http://hl7.org/fhir/SearchParameter/Observation-code".to_string(),
                expression: "code".to_string(),
            }]),
        );

        let err = dispatch_search(&mut builder, &param, &def, "Observation").unwrap_err();
        assert!(
            err.to_string().contains("requires SearchParameterRegistry"),
            "composite dispatch without registry must fail explicitly, got: {err}"
        );
    }

    #[test]
    fn test_dispatch_user_defined_search_parameter_uses_jsonb_fallback() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "custom-name".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "custom-name",
                "http://example.org/fhir/SearchParameter/Patient-custom-name",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name.family")
            .with_user_defined(true),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("fhir_text_blob(fhir_extract_text(resource, '[[\"name\",\"family\"]]'::jsonb)) LIKE"));
        assert!(!clause.contains("search_idx_string"));
    }

    // ========================================================================
    // GIN-optimized search tests
    // ========================================================================

    #[test]
    fn test_gin_exact_string_simple() {
        // Simple string field: Patient.gender with :exact
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "address-city".to_string(),
            modifier: Some(SearchModifier::Exact),
            values: vec![ParsedValue {
                prefix: None,
                raw: "Boston".to_string(),
            }],
        };

        build_gin_exact_string_search(
            &mut builder,
            &param,
            &["address".to_string(), "city".to_string()],
            &ElementTypeHint::Unknown,
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected @> containment, got: {clause}"
        );
        assert!(clause.contains("::jsonb"));
    }

    #[test]
    fn test_gin_exact_string_human_name() {
        // HumanName :exact search should generate OR of family/text/given containments
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "name".to_string(),
            modifier: Some(SearchModifier::Exact),
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };

        build_gin_exact_string_search(
            &mut builder,
            &param,
            &["name".to_string()],
            &ElementTypeHint::HumanName,
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        // Should have 3 @> checks (family, text, given)
        let containment_count = clause.matches("@>").count();
        assert_eq!(
            containment_count, 3,
            "Expected 3 containment checks for HumanName, got {containment_count}: {clause}"
        );
        assert!(clause.contains("OR"));
    }

    #[test]
    fn test_gin_exact_string_array() {
        // Array string field: Patient.name.family with :exact
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "family".to_string(),
            modifier: Some(SearchModifier::Exact),
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };

        build_gin_exact_string_search(
            &mut builder,
            &param,
            &["name".to_string(), "family".to_string()],
            &ElementTypeHint::Array("string".to_string()),
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected @> containment, got: {clause}"
        );
        assert!(clause.contains("::jsonb"));
    }

    #[test]
    fn test_dispatch_string_exact_uses_inplace_raw_value_array() {
        // :exact string search matches the raw extracted value array in-place.
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "family".to_string(),
            modifier: Some(SearchModifier::Exact),
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "family",
                "http://hl7.org/fhir/SearchParameter/Patient-family",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name.family")
            .with_element_type_hint(ElementTypeHint::Array("string".to_string())),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("= ANY(fhir_extract_text(resource"),
            "Expected in-place :exact over extracted value array, got: {clause}"
        );
    }

    #[test]
    fn test_dispatch_token_code_uses_gin() {
        // SimpleCode token dispatch should use GIN containment
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "gender".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "female".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "gender",
                "http://hl7.org/fhir/SearchParameter/Patient-gender",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.gender")
            .with_element_type_hint(ElementTypeHint::SimpleCode),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected GIN containment for SimpleCode, got: {clause}"
        );
        // JSON params contain the containment object {"gender": "female"}
        let params = builder.params();
        let json_str = params[0].as_str();
        assert!(
            json_str.contains("gender") && json_str.contains("female"),
            "Expected JSON param with gender/female, got: {json_str}"
        );
    }

    #[test]
    fn test_dispatch_token_codeable_concept_uses_gin() {
        // CodeableConcept token dispatch should use GIN containment
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "code".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "http://loinc.org|8480-6".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "code",
                "http://hl7.org/fhir/SearchParameter/Observation-code",
                SearchParameterType::Token,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.code"),
        );

        dispatch_search(&mut builder, &param, &def, "Observation").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected GIN containment for CodeableConcept, got: {clause}"
        );
        // JSON params contain the containment object with coding array
        let params = builder.params();
        let json_str = params[0].as_str();
        assert!(
            json_str.contains("coding"),
            "Expected JSON param with coding, got: {json_str}"
        );
    }
}
