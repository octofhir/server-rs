//! Convert SearchParams to FhirQueryBuilder for search execution.
//!
//! This module bridges the modern `SearchParams` type (from octofhir-storage)
//! to the SQL query builder, enabling search via the FhirStorage trait.

use crate::chaining::{build_chained_search, is_chained_parameter, parse_chained_parameter};
use crate::include::{is_include_parameter, is_revinclude_parameter};
use crate::ir::{
    CompositeClause, IdClause, NumberClause, QuantityClause, ReferenceClause, ResourceColumnParam,
    SearchDebugPlan, StringClause, TokenClause, TokenIndexShape, build_composite_debug_plan,
    build_date_debug_plan, build_number_debug_plan, build_quantity_debug_plan,
    build_reference_debug_plan, build_string_debug_plan, build_string_text_debug_predicate,
    build_token_debug_plan, render_date_clauses_as_or, render_id_clauses_as_or,
    resolve_composite_component_specs, resolve_resource_column_param, rewrite_date_clauses,
};
use crate::parameters::{
    ElementTypeHint, SearchModifier, SearchParameter, SearchParameterType, SearchPrefix,
};
use crate::parser::{ParsedParam, ParsedValue};
use crate::registry::SearchParameterRegistry;
use crate::reverse_chaining::{
    build_reverse_chain_search, is_reverse_chain_parameter, parse_reverse_chain,
};
use crate::sql_builder::{
    FhirQueryBuilder, IncludeSpec, JsonbPath, RevIncludeSpec, SearchCondition, SortOrder, SortSpec,
    SqlBuilder, SqlBuilderError, SqlValue, fhirpath_to_jsonb_path,
};
use crate::types::date_ast::{DateClause, DatePredicate};
use crate::types::dispatch_search_with_registry;
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
    /// Collect safe internal debug plan data. Off by default and not exposed by routes.
    pub collect_debug_plan: bool,
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
    /// Optional safe debug plan, collected only when requested by internal config.
    pub debug_plan: Option<SearchDebugPlan>,
}

/// Convert SearchParams through the native-IR search path.
///
/// This function:
/// 1. Converts SearchParams.parameters to ParsedParam format
/// 2. Uses dispatch_search() to build SQL conditions
/// 3. Handles _sort, _count, _offset
/// 4. Extracts _include/_revinclude specifications
/// 5. Handles chained parameters
pub fn build_native_ir_query_from_params(
    resource_type: &str,
    params: &SearchParams,
    registry: &SearchParameterRegistry,
    schema: &str,
) -> Result<ConvertedQuery, SqlBuilderError> {
    build_native_ir_query_from_params_with_config(
        resource_type,
        params,
        registry,
        schema,
        &SearchConfig::default(),
    )
}

/// Convert SearchParams through the native-IR search path with configurable unknown parameter
/// handling.
///
/// When `config.unknown_param_handling` is `Strict`, returns an error for unknown parameters.
/// When `Lenient`, unknown parameters are skipped and returned in the result for warning.
pub fn build_native_ir_query_from_params_with_config(
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
    let mut debug_plan = config
        .collect_debug_plan
        .then(|| SearchDebugPlan::new(resource_type));

    // Convert SearchParams.parameters to ParsedParam and process.
    //
    // FHIR R4 §3.1.1.5 (search.html#combining):
    //   - Repeated parameter occurrences (a=v1&a=v2) → AND (each iteration must
    //     match independently). Emit one ParsedParam per value entry.
    //   - Comma-separated values within one occurrence (a=v1,v2) → OR. Emit one
    //     ParsedParam with multiple ParsedValue.
    //
    // `params.parameters: HashMap<String, Vec<String>>` stores the Vec from
    // `&`-repetition (one entry per occurrence). Each String can still contain
    // a comma list. Iterate per entry so repeated occurrences AND naturally
    // through SqlBuilder.add_condition (which AND's top-level conditions).
    for (key, value_entries) in &params.parameters {
        // Skip control parameters
        if is_control_param(key) {
            continue;
        }

        // Reverse chaining (_has) — handled before chain detection because
        // `_has:` keys can contain `.` in the inner search-parameter spec.
        if is_reverse_chain_parameter(key) {
            handle_has_param(
                &mut sql_builder,
                key,
                value_entries,
                registry,
                resource_type,
            )?;
            continue;
        }

        // Handle chained parameters
        if is_chained_parameter(key) {
            handle_chained_param(
                &mut builder,
                &mut sql_builder,
                key,
                value_entries,
                registry,
                schema,
                resource_type,
            )?;
            continue;
        }

        // Fold repeated date-param occurrences with `{ge,gt}` and `{le,lt}`
        // bounds into one combined `&&` predicate over `search_idx_date`.
        if try_fold_repeated_date_window(
            &mut sql_builder,
            debug_plan.as_mut(),
            key,
            value_entries,
            registry,
            resource_type,
        )? {
            continue;
        }

        for value_entry in value_entries {
            // Convert one `&`-occurrence to ParsedParam (single-value entry,
            // possibly comma-split into multiple ParsedValue for OR).
            let mut parsed = convert_to_parsed_param(key, value_entry);

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
            if let Some(ref modifier) = parsed.modifier
                && !modifier.applicable_to(&param_def.param_type)
            {
                return Err(SqlBuilderError::InvalidSearchValue(format!(
                    "Modifier ':{:?}' is not valid for {} parameter '{}' (type: {:?})",
                    modifier, resource_type, parsed.name, param_def.param_type
                )));
            }

            // Validate prefix compatibility with parameter type.
            // If a prefix is not applicable (e.g., UUID starting with "eb" parsed as "ends before"
            // for a reference param), revert it — treat the prefix chars as part of the value.
            for value in &mut parsed.values {
                if let Some(ref prefix) = value.prefix
                    && !prefix.applicable_to(&param_def.param_type)
                {
                    // Restore the prefix as part of the raw value
                    value.raw = format!("{}{}", prefix, value.raw);
                    value.prefix = None;
                }
            }

            // Resource.id maps to the database row id column, not JSONB.
            if matches!(
                resolve_resource_column_param(&param_def),
                Some(crate::ir::ResourceColumnParam::Id)
            ) {
                let clauses = IdClause::from_parsed_param(&parsed, resource_type)?;
                if let Some(sql) = render_id_clauses_as_or(&mut sql_builder, &clauses, "r.id") {
                    sql_builder.add_condition(sql);
                }
                continue;
            }

            reject_non_production_fallback(&parsed, &param_def, resource_type)?;

            if config.collect_debug_plan {
                collect_date_debug_plan(debug_plan.as_mut(), &parsed, &param_def, resource_type)?;
                collect_string_debug_plan(
                    debug_plan.as_mut(),
                    &parsed,
                    param_def.param_type,
                    resource_type,
                )?;
                collect_number_debug_plan(
                    debug_plan.as_mut(),
                    &parsed,
                    param_def.param_type,
                    resource_type,
                )?;
                collect_quantity_debug_plan(
                    debug_plan.as_mut(),
                    &parsed,
                    param_def.param_type,
                    resource_type,
                )?;
                collect_composite_debug_plan(
                    debug_plan.as_mut(),
                    &parsed,
                    &param_def,
                    registry,
                    resource_type,
                )?;
                collect_token_debug_plan(debug_plan.as_mut(), &parsed, &param_def, resource_type)?;
                collect_reference_debug_plan(
                    debug_plan.as_mut(),
                    &parsed,
                    &param_def,
                    resource_type,
                )?;
            }

            // Use dispatch_search to build the condition
            dispatch_search_with_registry(
                &mut sql_builder,
                &parsed,
                &param_def,
                resource_type,
                registry,
            )?;
        }
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
        // Default FHIR ordering is latest updates first. Use the row column so
        // PostgreSQL can use the per-resource updated_at B-tree index.
        if let Ok(sort) = SortSpec::column("updated_at", SortOrder::Desc) {
            builder = builder.sort_by(sort);
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
        debug_plan,
    })
}

/// Check if a parameter is a control parameter.
fn is_control_param(name: &str) -> bool {
    CONTROL_PARAMS.contains(&name) || is_include_parameter(name) || is_revinclude_parameter(name)
}

/// Fold repeated date-param occurrences for the same key into one combined
/// `sid.rng && tstzrange(lo, hi, bounds)` EXISTS clause.
///
/// Triggers only for plain date-typed params (no `_lastUpdated`, no modifier,
/// no comma-OR lists, only `{ge, gt, le, lt}` prefixes). Returns `Ok(true)`
/// when the merged clause is emitted; caller must then skip the per-entry
/// dispatch loop. Returns `Ok(false)` to fall through.
fn try_fold_repeated_date_window(
    sql_builder: &mut SqlBuilder,
    debug_plan: Option<&mut SearchDebugPlan>,
    key: &str,
    value_entries: &[String],
    registry: &SearchParameterRegistry,
    resource_type: &str,
) -> Result<bool, SqlBuilderError> {
    if value_entries.len() < 2 {
        return Ok(false);
    }
    if key.contains(':') {
        return Ok(false); // any modifier blocks fold
    }
    let Some(param_def) = registry.get(resource_type, key) else {
        return Ok(false);
    };
    if param_def.param_type != SearchParameterType::Date {
        return Ok(false);
    }
    if matches!(
        resolve_resource_column_param(&param_def),
        Some(ResourceColumnParam::LastUpdated)
    ) {
        return Ok(false);
    }

    // Quick syntactic gate before any allocation: every occurrence must be
    // a single prefixed value with no comma-OR and no modifier baggage.
    for entry in value_entries {
        if entry.contains(',') || entry.is_empty() {
            return Ok(false);
        }
        let prefix_chars = entry.chars().take(2).collect::<String>();
        if !matches!(prefix_chars.as_str(), "ge" | "gt" | "le" | "lt") {
            return Ok(false);
        }
    }

    // Parse to AST. Every entry must produce one Overlap clause.
    let mut clauses: Vec<DateClause> = Vec::with_capacity(value_entries.len());
    for entry in value_entries {
        let parsed = convert_to_parsed_param(key, entry);
        if parsed.modifier.is_some() || parsed.values.len() != 1 {
            return Ok(false);
        }
        let mut produced = DateClause::from_parsed_param(&parsed, resource_type)?;
        if produced.len() != 1 || !matches!(produced[0].predicate, DatePredicate::Overlap { .. }) {
            return Ok(false);
        }
        clauses.push(produced.remove(0));
    }

    // Tree rewrite — collapse all Overlap clauses on the same key into one.
    let merged = rewrite_date_clauses(clauses);
    if merged.len() != 1 {
        return Ok(false);
    }
    let DatePredicate::Overlap { lo, hi } = &merged[0].predicate else {
        return Ok(false);
    };
    // Fold only when *both* bounds are present. A single-side window is
    // already handled efficiently by the per-occurrence path (the rewrite
    // is structurally identical to one Overlap clause).
    if lo.is_none() || hi.is_none() {
        return Ok(false);
    }

    if let Some(sql) = render_date_clauses_as_or(sql_builder, &merged) {
        sql_builder.add_condition(sql);
    }
    append_date_debug_plan(debug_plan, resource_type, &merged);
    Ok(true)
}

fn reject_non_production_fallback(
    parsed: &ParsedParam,
    param_def: &SearchParameter,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    match param_def.param_type {
        SearchParameterType::String if matches!(parsed.modifier, Some(SearchModifier::Text)) => {
            Err(fallback_disabled(
                resource_type,
                &parsed.name,
                "string :text narrative JSONB traversal",
            ))
        }
        SearchParameterType::Composite => Err(fallback_disabled(
            resource_type,
            &parsed.name,
            "composite JSONB component traversal",
        )),
        SearchParameterType::Reference
            if matches!(parsed.modifier, Some(SearchModifier::Missing)) =>
        {
            Err(fallback_disabled(
                resource_type,
                &parsed.name,
                "reference :missing JSONB presence check",
            ))
        }
        SearchParameterType::Token if matches!(parsed.modifier, Some(SearchModifier::Text)) => Err(
            fallback_disabled(resource_type, &parsed.name, "token :text display traversal"),
        ),
        _ => Ok(()),
    }
}

fn fallback_disabled(resource_type: &str, param_name: &str, fallback: &str) -> SqlBuilderError {
    SqlBuilderError::NotImplemented(format!(
        "{resource_type}.{param_name} requires {fallback}, but production fallback search paths are disabled"
    ))
}

fn collect_date_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    parsed: &ParsedParam,
    param_def: &SearchParameter,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    if parsed.modifier.is_some()
        || param_def.param_type != SearchParameterType::Date
        || matches!(
            resolve_resource_column_param(param_def),
            Some(ResourceColumnParam::LastUpdated)
        )
    {
        return Ok(());
    }

    let clauses = DateClause::from_parsed_param(parsed, resource_type)?;
    append_date_debug_plan(debug_plan, resource_type, &clauses);
    Ok(())
}

fn append_date_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    resource_type: &str,
    clauses: &[DateClause],
) {
    if let Some(plan) = debug_plan {
        plan.predicates
            .extend(build_date_debug_plan(resource_type, clauses).predicates);
    }
}

fn collect_string_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    parsed: &ParsedParam,
    param_type: SearchParameterType,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    if param_type != SearchParameterType::String {
        return Ok(());
    }

    if matches!(
        parsed.modifier,
        Some(crate::parameters::SearchModifier::Text)
    ) {
        append_string_text_debug_plan(debug_plan, &parsed.name);
        return Ok(());
    }

    let clauses = StringClause::from_parsed_param(parsed, resource_type)?;
    append_string_debug_plan(debug_plan, resource_type, &clauses);
    Ok(())
}

fn append_string_text_debug_plan(debug_plan: Option<&mut SearchDebugPlan>, param_code: &str) {
    if let Some(plan) = debug_plan {
        plan.predicates
            .push(build_string_text_debug_predicate(param_code));
    }
}

fn append_string_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    resource_type: &str,
    clauses: &[StringClause],
) {
    if let Some(plan) = debug_plan {
        plan.predicates
            .extend(build_string_debug_plan(resource_type, clauses).predicates);
    }
}

fn collect_number_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    parsed: &ParsedParam,
    param_type: SearchParameterType,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    if param_type != SearchParameterType::Number {
        return Ok(());
    }

    let clauses = NumberClause::from_parsed_param(parsed, resource_type)?;
    append_number_debug_plan(debug_plan, resource_type, &clauses);
    Ok(())
}

fn append_number_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    resource_type: &str,
    clauses: &[NumberClause],
) {
    if let Some(plan) = debug_plan {
        plan.predicates
            .extend(build_number_debug_plan(resource_type, clauses).predicates);
    }
}

fn collect_quantity_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    parsed: &ParsedParam,
    param_type: SearchParameterType,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    if param_type != SearchParameterType::Quantity {
        return Ok(());
    }

    let clauses = QuantityClause::from_parsed_param(parsed, resource_type)?;
    append_quantity_debug_plan(debug_plan, resource_type, &clauses);
    Ok(())
}

fn append_quantity_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    resource_type: &str,
    clauses: &[QuantityClause],
) {
    if let Some(plan) = debug_plan {
        plan.predicates
            .extend(build_quantity_debug_plan(resource_type, clauses).predicates);
    }
}

fn collect_composite_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    parsed: &ParsedParam,
    param_def: &SearchParameter,
    registry: &SearchParameterRegistry,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    if param_def.param_type != SearchParameterType::Composite {
        return Ok(());
    }

    let components = resolve_composite_component_specs(registry, resource_type, param_def)?;
    let clauses = CompositeClause::from_parsed_param(parsed, resource_type, &components)?;
    append_composite_debug_plan(debug_plan, resource_type, &clauses);
    Ok(())
}

fn append_composite_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    resource_type: &str,
    clauses: &[CompositeClause],
) {
    if let Some(plan) = debug_plan {
        plan.predicates
            .extend(build_composite_debug_plan(resource_type, clauses).predicates);
    }
}

fn collect_token_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    parsed: &ParsedParam,
    param_def: &SearchParameter,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    if param_def.param_type != SearchParameterType::Token {
        return Ok(());
    }

    let index_shape = token_index_shape(param_def);
    let clauses = TokenClause::from_parsed_param(parsed, resource_type, index_shape)?;
    append_token_debug_plan(debug_plan, resource_type, &clauses);
    Ok(())
}

fn collect_reference_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    parsed: &ParsedParam,
    param_def: &SearchParameter,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    if param_def.param_type != SearchParameterType::Reference {
        return Ok(());
    }

    let clauses = ReferenceClause::from_parsed_param(parsed, resource_type, &param_def.target)?;
    append_reference_debug_plan(debug_plan, resource_type, &clauses);
    Ok(())
}

fn token_index_shape(param_def: &SearchParameter) -> TokenIndexShape {
    if param_def.element_type_hint.is_identifier()
        || (matches!(&param_def.element_type_hint, ElementTypeHint::Unknown)
            && is_identifier_param(&param_def.code, param_def.expression.as_deref()))
    {
        TokenIndexShape::Identifier
    } else if matches!(&param_def.element_type_hint, ElementTypeHint::SimpleCode) {
        TokenIndexShape::SimpleCode
    } else {
        TokenIndexShape::Coding
    }
}

fn is_identifier_param(code: &str, expression: Option<&str>) -> bool {
    code == "identifier" || expression.is_some_and(|expr| expr.ends_with(".identifier"))
}

fn append_token_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    resource_type: &str,
    clauses: &[TokenClause],
) {
    if let Some(plan) = debug_plan {
        plan.predicates
            .extend(build_token_debug_plan(resource_type, clauses).predicates);
    }
}

fn append_reference_debug_plan(
    debug_plan: Option<&mut SearchDebugPlan>,
    resource_type: &str,
    clauses: &[ReferenceClause],
) {
    if let Some(plan) = debug_plan {
        plan.predicates
            .extend(build_reference_debug_plan(resource_type, clauses).predicates);
    }
}

/// Convert a single key=value occurrence to ParsedParam format.
///
/// Per FHIR R4 §3.1.1.5 search.html#combining:
/// - Comma-separated values within `value_entry` map to multiple ParsedValue
///   (OR semantics inside one occurrence).
/// - Repeated `&`-occurrences are AND'd by the caller emitting one ParsedParam
///   per occurrence.
///
/// :of-type matches the spec spelling (Identifier modifier). The legacy
/// camelCase `ofType` is also accepted.
fn convert_to_parsed_param(key: &str, value_entry: &str) -> ParsedParam {
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
            "of-type" | "ofType" => Some(crate::parameters::SearchModifier::OfType),
            "code-text" => Some(crate::parameters::SearchModifier::CodeText),
            "text-advanced" => Some(crate::parameters::SearchModifier::TextAdvanced),
            other if !other.is_empty() => {
                Some(crate::parameters::SearchModifier::Type(other.to_string()))
            }
            _ => None,
        };
        (n.to_string(), modifier)
    } else {
        (key.to_string(), None)
    };

    // Parse comma-separated values (OR semantics within this occurrence).
    let parsed_values: Vec<ParsedValue> = value_entry
        .split(',')
        .map(|part| {
            let part = part.trim();
            let (prefix, raw) = extract_prefix(part);
            ParsedValue {
                prefix,
                raw: raw.to_string(),
            }
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
/// Build reverse-chain (`_has`) conditions for one parameter key.
///
/// FHIR R4 §3.1.1.5.4 (search.html#has). Each `&`-occurrence emits one
/// EXISTS clause and contributes an AND at the top level; comma-separated
/// values within an occurrence OR via the inner `dispatch_search` invocation.
fn handle_has_param(
    sql_builder: &mut SqlBuilder,
    key: &str,
    values: &[String],
    registry: &SearchParameterRegistry,
    base_type: &str,
) -> Result<(), SqlBuilderError> {
    for value in values {
        match parse_reverse_chain(key, value, registry, base_type) {
            Ok(rc) => {
                tracing::debug!(
                    source = %rc.source_type,
                    ref_param = %rc.reference_param,
                    value = %value,
                    "Processing _has occurrence"
                );
                build_reverse_chain_search(sql_builder, &rc, base_type, registry)
                    .map_err(|e| SqlBuilderError::InvalidSearchValue(e.to_string()))?;
            }
            Err(e) => {
                tracing::warn!(
                    key = %key,
                    value = %value,
                    error = %e,
                    "Failed to parse _has parameter"
                );
                // Skip invalid occurrence (lenient handling of unknown params).
            }
        }
    }
    Ok(())
}

/// Build chained-search conditions for one parameter key.
///
/// Per FHIR R4 §3.1.1.5 (search.html#combining), repeated `&`-occurrences of
/// the same chained parameter (e.g. `subject:Patient.name=A&subject:Patient.name=B`)
/// AND independently — each link in the chain must match the corresponding value.
/// Comma-separated values inside a single occurrence still OR via the inner
/// parameter handler (build_chained_search routes through dispatch_search which
/// already handles comma-OR).
///
/// Each `&`-occurrence emits one top-level condition that SqlBuilder AND's.
fn handle_chained_param(
    _builder: &mut FhirQueryBuilder,
    sql_builder: &mut SqlBuilder,
    key: &str,
    values: &[String],
    registry: &SearchParameterRegistry,
    _schema: &str,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    for value in values {
        match parse_chained_parameter(key, value, registry, resource_type) {
            Ok(chained) => {
                tracing::debug!(
                    chain = ?chained.chain,
                    final_param = %chained.final_param,
                    value = %value,
                    "Processing chained parameter occurrence"
                );
                build_chained_search(sql_builder, &chained, resource_type, registry)
                    .map_err(|e| SqlBuilderError::InvalidSearchValue(e.to_string()))?;
            }
            Err(e) => {
                tracing::warn!(
                    key = %key,
                    value = %value,
                    error = %e,
                    "Failed to parse chained parameter"
                );
                // Skip invalid chained parameter occurrence (do not fail the
                // whole search — matches lenient handling of unknown params).
            }
        }
    }
    Ok(())
}

/// Build a SortSpec from a sort parameter.
fn build_sort_spec(
    field: &str,
    descending: bool,
    registry: &SearchParameterRegistry,
    resource_type: &str,
) -> Option<SortSpec> {
    let param_def = registry.get(resource_type, field)?;
    let order = if descending {
        SortOrder::Desc
    } else {
        SortOrder::Asc
    };

    if let Some(column_param) = resolve_resource_column_param(&param_def) {
        return SortSpec::column(column_param.column_name(), order).ok();
    }

    let expr = param_def.expression.as_deref()?;
    let path_segments = fhirpath_to_jsonb_path(expr, resource_type);
    let path = JsonbPath::new(path_segments).ok()?;

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

    fn assert_fallback_disabled<T>(
        result: Result<T, SqlBuilderError>,
        expected_param: &str,
        expected_fallback: &str,
    ) {
        let err = match result {
            Ok(_) => panic!("fallback-backed production search must be rejected"),
            Err(err) => err,
        };
        match err {
            SqlBuilderError::NotImplemented(message) => {
                assert!(
                    message.contains(expected_param),
                    "error should identify param {expected_param:?}, got: {message}"
                );
                assert!(
                    message.contains(expected_fallback),
                    "error should identify fallback {expected_fallback:?}, got: {message}"
                );
                assert!(
                    message.contains("fallback search paths are disabled"),
                    "error should explain disabled fallbacks, got: {message}"
                );
            }
            other => panic!("expected NotImplemented fallback rejection, got: {other:?}"),
        }
    }

    #[test]
    fn test_convert_to_parsed_param_simple() {
        let parsed = convert_to_parsed_param("name", "John");
        assert_eq!(parsed.name, "name");
        assert!(parsed.modifier.is_none());
        assert_eq!(parsed.values.len(), 1);
        assert_eq!(parsed.values[0].raw, "John");
    }

    #[test]
    fn test_convert_to_parsed_param_with_modifier() {
        let parsed = convert_to_parsed_param("name:exact", "John");
        assert_eq!(parsed.name, "name");
        assert!(matches!(
            parsed.modifier,
            Some(crate::parameters::SearchModifier::Exact)
        ));
    }

    #[test]
    fn test_convert_to_parsed_param_with_prefix() {
        let parsed = convert_to_parsed_param("birthdate", "ge2000-01-01");
        assert_eq!(parsed.name, "birthdate");
        assert_eq!(parsed.values.len(), 1);
        assert_eq!(parsed.values[0].prefix, Some(SearchPrefix::Ge));
        assert_eq!(parsed.values[0].raw, "2000-01-01");
    }

    #[test]
    fn test_convert_to_parsed_param_comma_separated() {
        let parsed = convert_to_parsed_param("status", "active,completed");
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
            build_native_ir_query_from_params("Observation", &params, &registry, "public").unwrap();
        assert!(
            !converted.includes.is_empty(),
            "converted.includes should not be empty"
        );
        assert_eq!(converted.includes[0].source_type, "Observation");
        assert_eq!(converted.includes[0].param_name, "subject");
    }

    fn representative_registry() -> SearchParameterRegistry {
        let registry = SearchParameterRegistry::new();
        crate::common::register_common_parameters(&registry);

        registry.register(
            SearchParameter::new(
                "birthdate",
                "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
                SearchParameterType::Date,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.birthDate"),
        );
        registry.register(
            SearchParameter::new(
                "family",
                "http://hl7.org/fhir/SearchParameter/Patient-family",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name.family"),
        );
        registry.register(
            SearchParameter::new(
                "identifier",
                "http://hl7.org/fhir/SearchParameter/Patient-identifier",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.identifier")
            .with_element_type_hint(ElementTypeHint::Identifier),
        );
        registry.register(
            SearchParameter::new(
                "code",
                "http://hl7.org/fhir/SearchParameter/Observation-code",
                SearchParameterType::Token,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.code")
            .with_element_type_hint(ElementTypeHint::Token),
        );
        registry.register(
            SearchParameter::new(
                "subject",
                "http://hl7.org/fhir/SearchParameter/Observation-subject",
                SearchParameterType::Reference,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.subject")
            .with_targets(vec!["Patient".to_string()]),
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
        registry.register(
            SearchParameter::new(
                "code-value-quantity",
                "http://hl7.org/fhir/SearchParameter/Observation-code-value-quantity",
                SearchParameterType::Composite,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.component")
            .with_components(vec![
                crate::parameters::SearchParameterComponent {
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-code".to_string(),
                    expression: "Observation.component.code".to_string(),
                },
                crate::parameters::SearchParameterComponent {
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-value-quantity"
                        .to_string(),
                    expression: "Observation.component.valueQuantity".to_string(),
                },
            ]),
        );

        registry
    }

    #[test]
    fn representative_native_ir_queries_keep_expected_sql_shape() {
        let registry = representative_registry();
        let cases = [
            (
                "Patient",
                "_id=pat-1&_count=10",
                ["r.id = $1", "updated_at"],
            ),
            (
                "Patient",
                "_lastUpdated=ge2024-01-01&_lastUpdated=le2024-12-31&_count=10",
                ["r.updated_at", "ORDER BY"],
            ),
            (
                "Patient",
                "birthdate=ge1980-01-01&birthdate=le2000-12-31&_count=10",
                ["search_idx_date", "tstzrange"],
            ),
            (
                "Patient",
                "family=Smith&_count=10",
                ["search_idx_string", "value_norm LIKE"],
            ),
            (
                "Patient",
                "family:exact=Smith&_count=10",
                ["search_idx_string", "value_exact ="],
            ),
            (
                "Patient",
                "identifier=http://hospital.example/mrn|12345&_count=10",
                ["r.resource @>", "ORDER BY"],
            ),
            (
                "Observation",
                "code=http://loinc.org|8480-6&_count=10",
                ["r.resource @>", "\"observation\""],
            ),
            (
                "Observation",
                "subject=Patient/pat-1&_count=10",
                ["search_idx_reference", "target_id ="],
            ),
            (
                "Observation",
                "subject:Patient.family=Smith&_count=10",
                ["search_idx_reference", "search_idx_string"],
            ),
            (
                "Patient",
                "_has:Observation:subject:code=http://loinc.org|8480-6&_count=10",
                ["search_idx_reference", "has0.resource @>"],
            ),
            (
                "Observation",
                "value-quantity=ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
                ["search_idx_quantity", "siq.value_num"],
            ),
        ];

        for (resource_type, query, expected_fragments) in cases {
            let params = parse_query_string(query, 10, 100);
            let converted =
                build_native_ir_query_from_params(resource_type, &params, &registry, "public")
                    .unwrap();
            let built = converted.builder.with_raw_resource(true).build().unwrap();

            for expected in expected_fragments {
                assert!(
                    built.sql.contains(expected),
                    "query `{resource_type}?{query}` missing `{expected}` in SQL: {}",
                    built.sql
                );
            }
        }

        let composite_params = parse_query_string(
            "code-value-quantity=http://loinc.org|8480-6$ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
            10,
            100,
        );
        assert_fallback_disabled(
            build_native_ir_query_from_params(
                "Observation",
                &composite_params,
                &registry,
                "public",
            ),
            "Observation.code-value-quantity",
            "composite JSONB component traversal",
        );
    }

    #[test]
    fn test_default_sort_uses_updated_at_column() {
        let registry = SearchParameterRegistry::new();
        let params = parse_query_string("_count=5", 10, 100);
        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        assert!(
            built.sql.contains("ORDER BY \"r\".\"updated_at\" DESC"),
            "expected updated_at column sort, got: {}",
            built.sql
        );
        assert!(
            !built.sql.contains("meta"),
            "default sort should not use JSONB meta path, got: {}",
            built.sql
        );
    }

    #[test]
    fn test_last_updated_and_id_sort_use_columns() {
        let registry = SearchParameterRegistry::new();
        crate::common::register_common_parameters(&registry);
        let params = parse_query_string("_sort=-_lastUpdated,_id&_count=5", 10, 100);
        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        assert!(
            built
                .sql
                .contains("ORDER BY \"r\".\"updated_at\" DESC NULLS LAST, \"r\".\"id\" ASC"),
            "expected _lastUpdated/_id column sort, got: {}",
            built.sql
        );
    }

    #[test]
    fn test_id_search_uses_column_bound_params() {
        let registry = SearchParameterRegistry::new();
        crate::common::register_common_parameters(&registry);
        let params = parse_query_string("_id=pat-1,pat-2&_count=5", 10, 100);
        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        assert!(
            built.sql.contains("(r.id = $1 OR r.id = $2)"),
            "expected _id OR over r.id column, got: {}",
            built.sql
        );
        assert!(!built.sql.contains("pat-1"));
        assert!(matches!(&built.params[0], SqlValue::Text(value) if value == "pat-1"));
        assert!(matches!(&built.params[1], SqlValue::Text(value) if value == "pat-2"));
    }

    #[test]
    fn test_id_not_search_uses_boolean_negation_without_not_wrapper() {
        let registry = SearchParameterRegistry::new();
        crate::common::register_common_parameters(&registry);
        let params = parse_query_string("_id:not=pat-1&_count=5", 10, 100);
        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        assert!(
            built.sql.contains("(r.id = $1) = false"),
            "expected _id:not boolean negation over r.id column, got: {}",
            built.sql
        );
        assert!(!built.sql.contains("NOT (r.id ="));
        assert!(matches!(&built.params[0], SqlValue::Text(value) if value == "pat-1"));
    }

    #[test]
    fn test_identifier_system_value_through_query_builder() {
        use crate::parameters::{ElementTypeHint, SearchParameter, SearchParameterType};
        // Set up registry with identifier search parameter
        let registry = SearchParameterRegistry::new();
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

        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
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
        let registry = SearchParameterRegistry::new();
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

        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
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

    // try_fold_repeated_date_window — `{ge,gt}+{le,lt}` → single `&&` EXISTS.

    fn date_registry() -> SearchParameterRegistry {
        use crate::parameters::SearchParameter;
        let registry = SearchParameterRegistry::new();
        registry.register(SearchParameter::new(
            "birthdate",
            "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
            SearchParameterType::Date,
            vec!["Patient".to_string()],
        ));
        registry.register(SearchParameter::new(
            "date",
            "http://hl7.org/fhir/SearchParameter/Encounter-date",
            SearchParameterType::Date,
            vec!["Encounter".to_string()],
        ));
        registry
    }

    fn date_registry_with_expression() -> SearchParameterRegistry {
        use crate::parameters::SearchParameter;
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "birthdate",
                "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
                SearchParameterType::Date,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.birthDate"),
        );
        registry
    }

    fn string_registry_with_expression() -> SearchParameterRegistry {
        use crate::parameters::SearchParameter;
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "family",
                "http://hl7.org/fhir/SearchParameter/Patient-family",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name.family"),
        );
        registry
    }

    fn token_registry_with_expression() -> SearchParameterRegistry {
        use crate::parameters::SearchParameter;
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "code",
                "http://hl7.org/fhir/SearchParameter/Observation-code",
                SearchParameterType::Token,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.code")
            .with_element_type_hint(ElementTypeHint::Token),
        );
        registry
    }

    fn simple_code_registry_with_expression() -> SearchParameterRegistry {
        use crate::parameters::SearchParameter;
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "gender",
                "http://hl7.org/fhir/SearchParameter/Patient-gender",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.gender")
            .with_element_type_hint(ElementTypeHint::SimpleCode),
        );
        registry
    }

    fn reference_registry_with_expression() -> SearchParameterRegistry {
        use crate::parameters::SearchParameter;
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "subject",
                "http://hl7.org/fhir/SearchParameter/Observation-subject",
                SearchParameterType::Reference,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.subject")
            .with_targets(vec!["Patient".to_string(), "Group".to_string()]),
        );
        registry
    }

    fn number_registry_with_expression() -> SearchParameterRegistry {
        use crate::parameters::SearchParameter;
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "value",
                "http://hl7.org/fhir/SearchParameter/Observation-value",
                SearchParameterType::Number,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.valueInteger"),
        );
        registry
    }

    fn quantity_registry_with_expression() -> SearchParameterRegistry {
        use crate::parameters::SearchParameter;
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "value-quantity",
                "http://hl7.org/fhir/SearchParameter/Observation-value-quantity",
                SearchParameterType::Quantity,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.valueQuantity"),
        );
        registry
    }

    fn composite_registry_with_expression() -> SearchParameterRegistry {
        use crate::parameters::{SearchParameter, SearchParameterComponent};
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "code",
                "http://hl7.org/fhir/SearchParameter/Observation-code",
                SearchParameterType::Token,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.component.code"),
        );
        registry.register(
            SearchParameter::new(
                "value-quantity",
                "http://hl7.org/fhir/SearchParameter/Observation-value-quantity",
                SearchParameterType::Quantity,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.component.valueQuantity"),
        );
        registry.register(
            SearchParameter::new(
                "code-value-quantity",
                "http://hl7.org/fhir/SearchParameter/Observation-code-value-quantity",
                SearchParameterType::Composite,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.component")
            .with_components(vec![
                SearchParameterComponent {
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-code".to_string(),
                    expression: "Observation.component.code".to_string(),
                },
                SearchParameterComponent {
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-value-quantity"
                        .to_string(),
                    expression: "Observation.component.valueQuantity".to_string(),
                },
            ]),
        );
        registry
    }

    #[test]
    fn date_comma_values_render_or_within_one_occurrence() {
        let registry = date_registry_with_expression();
        let params = parse_query_string("birthdate=2024-01-01,2024-02-01&_count=5", 10, 100);

        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        assert!(
            built.sql.contains(" OR "),
            "comma-separated date values must OR, got: {}",
            built.sql
        );
        assert_eq!(
            built
                .sql
                .matches("EXISTS (SELECT 1 FROM search_idx_date")
                .count(),
            2,
            "two comma values should produce two OR'd EXISTS clauses: {}",
            built.sql
        );
    }

    #[test]
    fn date_comma_values_keep_prefix_per_value() {
        let registry = date_registry_with_expression();
        let params = parse_query_string("birthdate=lt1975,ge2005&_count=5", 10, 100);

        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        assert!(
            built.sql.contains(" OR "),
            "comma-separated prefixed dates must OR, got: {}",
            built.sql
        );
        assert_eq!(
            built
                .sql
                .matches("EXISTS (SELECT 1 FROM search_idx_date")
                .count(),
            2,
            "two prefixed comma values should produce two OR'd EXISTS clauses: {}",
            built.sql
        );
        let rendered_params = built
            .params
            .iter()
            .map(SqlValue::as_display_str)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered_params.contains("1975-01-01T00:00:00Z")
                && rendered_params.contains("2005-01-01T00:00:00Z"),
            "date bounds should be bound independently per OR value, params: {rendered_params}"
        );
    }

    #[test]
    fn date_missing_modifier_uses_sidecar_existence() {
        let registry = date_registry_with_expression();
        let cases = [
            ("birthdate:missing=true&_count=5", "NOT EXISTS"),
            ("birthdate:missing=false&_count=5", "EXISTS"),
        ];

        for (query, expected_sql) in cases {
            let params = parse_query_string(query, 10, 100);
            let converted =
                build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
            let built = converted.builder.with_raw_resource(true).build().unwrap();

            assert!(
                built.sql.contains(expected_sql) && built.sql.contains("search_idx_date"),
                "date :missing should use sidecar existence SQL, got: {}",
                built.sql
            );
            assert!(
                !built
                    .params
                    .iter()
                    .map(SqlValue::as_display_str)
                    .any(|param| param.eq_ignore_ascii_case("true")
                        || param.eq_ignore_ascii_case("false")),
                ":missing booleans should not be parsed as date values"
            );
        }
    }

    #[test]
    fn repeated_date_params_render_and_between_occurrences() {
        let registry = date_registry_with_expression();
        let params = parse_query_string(
            "birthdate=2024-01-01&birthdate=2024-02-01&_count=5",
            10,
            100,
        );

        let converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();

        assert!(
            built
                .sql
                .contains(") AND EXISTS (SELECT 1 FROM search_idx_date"),
            "repeated date params must AND occurrences, got: {}",
            built.sql
        );
        assert_eq!(
            built
                .sql
                .matches("EXISTS (SELECT 1 FROM search_idx_date")
                .count(),
            2,
            "two repeated values should produce two AND'd EXISTS clauses: {}",
            built.sql
        );
    }

    #[test]
    fn date_debug_plan_is_collected_only_when_requested() {
        let registry = date_registry_with_expression();
        let params = parse_query_string(
            "birthdate=ge2000-01-01&birthdate=le2000-12-31&_count=5",
            10,
            100,
        );

        let default_converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        assert!(default_converted.debug_plan.is_none());

        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };
        let converted = build_native_ir_query_from_params_with_config(
            "Patient", &params, &registry, "public", &config,
        )
        .unwrap();
        let plan = converted.debug_plan.expect("debug plan collected");

        assert_eq!(plan.resource_type, "Patient");
        assert_eq!(
            plan.predicates.len(),
            1,
            "folded date window = one predicate"
        );
        assert_eq!(plan.predicates[0].param_code, "birthdate");
        assert!(plan.predicates[0].index_backed);
        assert!(plan.predicates[0].sql_shape.contains("sid.rng &&"));

        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("sidecar_date"));
        assert!(json.contains("search_idx_date_*_param_code_rng_idx"));
        assert!(
            !json.contains("2000-01-01") && !json.contains("2000-12-31"),
            "debug output must stay redacted: {json}"
        );
    }

    #[test]
    fn string_debug_plan_is_collected_only_when_requested() {
        let registry = string_registry_with_expression();
        let params = parse_query_string("family:contains=Smíth&_count=5", 10, 100);

        let default_converted =
            build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
        assert!(default_converted.debug_plan.is_none());

        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };
        let converted = build_native_ir_query_from_params_with_config(
            "Patient", &params, &registry, "public", &config,
        )
        .unwrap();
        let plan = converted.debug_plan.expect("debug plan collected");

        assert_eq!(plan.resource_type, "Patient");
        assert_eq!(plan.predicates.len(), 1);
        assert_eq!(plan.predicates[0].param_code, "family");
        assert_eq!(plan.predicates[0].search_type, SearchParameterType::String);
        assert!(plan.predicates[0].index_backed);
        assert_eq!(
            plan.predicates[0].strategy,
            crate::ir::IndexStrategy::SidecarString
        );
        assert!(plan.predicates[0].sql_shape.contains("sid.value_norm LIKE"));

        let built = converted.builder.with_raw_resource(true).build().unwrap();
        assert!(
            built.sql.contains("search_idx_string") && built.sql.contains("sid.value_norm LIKE"),
            "string runtime path must use sidecar SQL, got: {}",
            built.sql
        );

        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("sidecar_string"));
        assert!(json.contains("search_idx_string_*_param_code_value_norm_trgm_idx"));
        assert!(
            !json.contains("Smíth") && !json.contains("smith"),
            "debug output must stay redacted: {json}"
        );
    }

    #[test]
    fn string_query_builder_uses_sidecar_normalization_and_exact_raw_value() {
        let registry = string_registry_with_expression();
        let cases = [
            ("family=Müller", "sid.value_norm LIKE", "muller%"),
            ("family=Van+Pelt", "sid.value_norm LIKE", "van pelt%"),
            ("family:contains=Müller", "sid.value_norm LIKE", "%muller%"),
            ("family:exact=Müller", "sid.value_exact =", "Müller"),
            ("family:exact=Smith", "sid.value_exact =", "Smith"),
            ("family:exact=smith", "sid.value_exact =", "smith"),
        ];

        for (query, expected_sql, expected_param) in cases {
            let params = parse_query_string(query, 10, 100);
            let converted =
                build_native_ir_query_from_params("Patient", &params, &registry, "public").unwrap();
            let built = converted.builder.with_raw_resource(true).build().unwrap();

            assert!(
                built.sql.contains("search_idx_string") && built.sql.contains(expected_sql),
                "string query should use sidecar SQL, got: {}",
                built.sql
            );
            assert!(
                !built.sql.contains("Müller") && !built.sql.contains("muller"),
                "string values must be bound, not interpolated: {}",
                built.sql
            );
            let rendered_params = built
                .params
                .iter()
                .map(SqlValue::as_display_str)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                rendered_params.contains(expected_param),
                "expected bound param {expected_param:?}, params: {rendered_params}"
            );
        }
    }

    #[test]
    fn string_text_query_jsonb_fallback_is_rejected() {
        let registry = string_registry_with_expression();
        let params = parse_query_string("family:text=Müller&_count=5", 10, 100);
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        assert_fallback_disabled(
            build_native_ir_query_from_params_with_config(
                "Patient", &params, &registry, "public", &config,
            ),
            "Patient.family",
            "string :text narrative JSONB traversal",
        );
    }

    #[test]
    fn number_query_uses_sidecar_number_index() {
        let registry = number_registry_with_expression();
        let params = parse_query_string("value=ge123.45&_count=5", 10, 100);
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        let converted = build_native_ir_query_from_params_with_config(
            "Observation",
            &params,
            &registry,
            "public",
            &config,
        )
        .unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();
        assert!(
            built.sql.contains("search_idx_number") && built.sql.contains("sin.value_num"),
            "number runtime path should use sidecar number index, got: {}",
            built.sql
        );
        let plan = converted.debug_plan.expect("debug plan collected");
        assert_eq!(
            plan.predicates[0].strategy,
            crate::ir::IndexStrategy::SidecarNumber
        );
        assert!(plan.predicates[0].index_backed);
    }

    #[test]
    fn quantity_query_uses_sidecar_quantity_index() {
        let registry = quantity_registry_with_expression();
        let params = parse_query_string(
            "value-quantity=5.5|http://unitsofmeasure.org|mg&_count=5",
            10,
            100,
        );
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        let converted = build_native_ir_query_from_params_with_config(
            "Observation",
            &params,
            &registry,
            "public",
            &config,
        )
        .unwrap();
        let built = converted.builder.with_raw_resource(true).build().unwrap();
        assert!(
            built.sql.contains("search_idx_quantity")
                && built.sql.contains("siq.value_num")
                && built.sql.contains("siq.system")
                && built.sql.contains("siq.code")
                && built.sql.contains("siq.unit"),
            "quantity runtime path should use sidecar quantity index, got: {}",
            built.sql
        );
        let plan = converted.debug_plan.expect("debug plan collected");
        assert_eq!(
            plan.predicates[0].strategy,
            crate::ir::IndexStrategy::SidecarQuantity
        );
        assert!(plan.predicates[0].index_backed);
    }

    #[test]
    fn composite_jsonb_component_traversal_fallback_is_rejected() {
        let registry = composite_registry_with_expression();
        let params = parse_query_string(
            "code-value-quantity=http://loinc.org|8480-6$gt5.5|http://unitsofmeasure.org|mg&_count=5",
            10,
            100,
        );
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        assert_fallback_disabled(
            build_native_ir_query_from_params_with_config(
                "Observation",
                &params,
                &registry,
                "public",
                &config,
            ),
            "Observation.code-value-quantity",
            "composite JSONB component traversal",
        );
    }

    #[test]
    fn reference_missing_jsonb_presence_fallback_is_rejected() {
        let registry = reference_registry_with_expression();
        let params = parse_query_string("subject:missing=true&_count=5", 10, 100);
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        assert_fallback_disabled(
            build_native_ir_query_from_params_with_config(
                "Observation",
                &params,
                &registry,
                "public",
                &config,
            ),
            "Observation.subject",
            "reference :missing JSONB presence check",
        );
    }

    #[test]
    fn token_text_display_fallback_is_rejected() {
        let registry = token_registry_with_expression();
        let params = parse_query_string("code:text=systolic&_count=5", 10, 100);
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        assert_fallback_disabled(
            build_native_ir_query_from_params_with_config(
                "Observation",
                &params,
                &registry,
                "public",
                &config,
            ),
            "Observation.code",
            "token :text display traversal",
        );
    }

    #[test]
    fn token_debug_plan_is_collected_only_when_requested() {
        let registry = token_registry_with_expression();
        let params = parse_query_string("code=http://loinc.org|8480-6&_count=5", 10, 100);

        let default_converted =
            build_native_ir_query_from_params("Observation", &params, &registry, "public").unwrap();
        assert!(default_converted.debug_plan.is_none());

        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };
        let converted = build_native_ir_query_from_params_with_config(
            "Observation",
            &params,
            &registry,
            "public",
            &config,
        )
        .unwrap();
        let plan = converted.debug_plan.expect("debug plan collected");

        assert_eq!(plan.resource_type, "Observation");
        assert_eq!(plan.predicates.len(), 1);
        assert_eq!(plan.predicates[0].param_code, "code");
        assert_eq!(plan.predicates[0].search_type, SearchParameterType::Token);
        assert_eq!(
            plan.predicates[0].strategy,
            crate::ir::IndexStrategy::JsonbContainment
        );
        assert!(plan.predicates[0].index_backed);
        assert!(plan.predicates[0].sql_shape.contains("system: $system"));

        let built = converted.builder.with_raw_resource(true).build().unwrap();
        assert!(
            built.sql.contains("@>") && built.sql.contains("::jsonb"),
            "token runtime path must keep existing JSONB containment SQL, got: {}",
            built.sql
        );

        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("jsonb_containment"));
        assert!(json.contains("idx_observation_gin"));
        assert!(
            !json.contains("loinc") && !json.contains("8480-6"),
            "debug output must stay redacted: {json}"
        );
    }

    #[test]
    fn identifier_token_debug_plan_matches_gin_runtime_path() {
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "identifier",
                "http://hl7.org/fhir/SearchParameter/Patient-identifier",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.identifier")
            .with_element_type_hint(ElementTypeHint::Identifier),
        );
        let params = parse_query_string(
            "identifier=http://hospital.example/mrn|12345&_count=5",
            10,
            100,
        );
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        let converted = build_native_ir_query_from_params_with_config(
            "Patient", &params, &registry, "public", &config,
        )
        .unwrap();
        let plan = converted.debug_plan.expect("debug plan collected");

        assert_eq!(plan.resource_type, "Patient");
        assert_eq!(plan.predicates.len(), 1);
        assert_eq!(plan.predicates[0].param_code, "identifier");
        assert_eq!(
            plan.predicates[0].strategy,
            crate::ir::IndexStrategy::JsonbContainment
        );
        assert!(plan.predicates[0].index_backed);
        assert_eq!(
            plan.predicates[0].expected_index,
            Some("idx_patient_gin".to_string())
        );
        assert!(
            plan.predicates[0]
                .sql_shape
                .contains("resource @> {identifier")
        );

        let built = converted.builder.with_raw_resource(true).build().unwrap();
        assert!(
            built.sql.contains("r.resource @>") && built.sql.contains("::jsonb"),
            "identifier token runtime path should use resource containment, got: {}",
            built.sql
        );

        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("jsonb_containment"));
        assert!(json.contains("idx_patient_gin"));
        assert!(
            !json.contains("hospital.example") && !json.contains("12345"),
            "debug output must stay redacted: {json}"
        );
    }

    #[test]
    fn token_query_builder_preserves_fhir_token_forms() {
        let registry = token_registry_with_expression();

        let cases = [
            (
                "code=8480-6",
                "@>",
                "code-only token should use JSONB containment",
                vec!["8480-6"],
            ),
            (
                "code=|8480-6",
                "c->>'system' IS NULL",
                "|code token should require absent coding system",
                vec!["8480-6"],
            ),
            (
                "code=http://loinc.org|",
                "c->>'system' =",
                "system| token should match any code in that system",
                vec!["http://loinc.org"],
            ),
            (
                "code=http://loinc.org|8480-6",
                "@>",
                "system|code token should use JSONB coding containment",
                vec!["http://loinc.org", "8480-6"],
            ),
        ];

        for (query, expected_sql, message, redacted_values) in cases {
            let params = parse_query_string(query, 10, 100);
            let converted =
                build_native_ir_query_from_params("Observation", &params, &registry, "public")
                    .unwrap();
            let built = converted.builder.with_raw_resource(true).build().unwrap();

            assert!(
                built.sql.contains(expected_sql),
                "{message}, got: {}",
                built.sql
            );
            assert!(
                !built.sql.contains("loinc") && !built.sql.contains("8480-6"),
                "token values must not be interpolated into SQL text: {}",
                built.sql
            );

            let rendered_params = built
                .params
                .iter()
                .map(SqlValue::as_display_str)
                .collect::<Vec<_>>()
                .join("\n");
            for expected_value in redacted_values {
                assert!(
                    rendered_params.contains(expected_value),
                    "expected bound param {expected_value:?}, params: {rendered_params}"
                );
            }
        }
    }

    #[test]
    fn simple_code_system_any_token_matches_nothing() {
        let registry = simple_code_registry_with_expression();
        let params = parse_query_string("gender=http://example.org|&_count=5", 10, 100);
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        let converted = build_native_ir_query_from_params_with_config(
            "Patient", &params, &registry, "public", &config,
        )
        .unwrap();
        let plan = converted.debug_plan.expect("debug plan collected");

        assert_eq!(plan.predicates.len(), 1);
        assert_eq!(plan.predicates[0].param_code, "gender");
        assert_eq!(
            plan.predicates[0].strategy,
            crate::ir::IndexStrategy::JsonbTraversal
        );
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].sql_shape, "FALSE");

        let built = converted.builder.with_raw_resource(true).build().unwrap();
        assert!(
            built.sql.contains("WHERE FALSE"),
            "simple code system-only token should match nothing, got: {}",
            built.sql
        );
        assert!(
            !built.sql.contains("example.org"),
            "token values must not be interpolated into SQL text: {}",
            built.sql
        );
    }

    #[test]
    fn reference_debug_plan_marks_sidecar_path_and_runtime_uses_index_only() {
        let registry = reference_registry_with_expression();
        let params = parse_query_string("subject=Patient/pat-123&_count=5", 10, 100);
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        let converted = build_native_ir_query_from_params_with_config(
            "Observation",
            &params,
            &registry,
            "public",
            &config,
        )
        .unwrap();
        let plan = converted.debug_plan.expect("debug plan collected");

        assert_eq!(plan.resource_type, "Observation");
        assert_eq!(plan.predicates.len(), 1);
        assert_eq!(plan.predicates[0].param_code, "subject");
        assert_eq!(
            plan.predicates[0].search_type,
            SearchParameterType::Reference
        );
        assert_eq!(
            plan.predicates[0].strategy,
            crate::ir::IndexStrategy::SidecarReference
        );
        assert!(plan.predicates[0].index_backed);
        assert_eq!(
            plan.predicates[0].expected_index,
            Some("idx_ref_local".to_string())
        );
        assert!(plan.predicates[0].sql_shape.contains("target_type"));

        let built = converted.builder.with_raw_resource(true).build().unwrap();
        assert!(
            built.sql.contains("search_idx_reference") && !built.sql.contains(" OR "),
            "reference runtime path should use sidecar only, got: {}",
            built.sql
        );
        assert!(
            !built.sql.contains("pat-123") && !built.sql.contains("Patient/pat-123"),
            "reference values must be bound, not interpolated: {}",
            built.sql
        );

        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("sidecar_reference"));
        assert!(
            !json.contains("pat-123") && !json.contains("Patient/pat-123"),
            "debug output must stay redacted: {json}"
        );
    }

    #[test]
    fn reference_identifier_debug_plan_uses_identifier_index_shape() {
        let registry = reference_registry_with_expression();
        let params = parse_query_string(
            "subject:identifier=http://hospital.example|abc&_count=5",
            10,
            100,
        );
        let config = SearchConfig {
            unknown_param_handling: UnknownParamHandling::Lenient,
            collect_debug_plan: true,
        };

        let converted = build_native_ir_query_from_params_with_config(
            "Observation",
            &params,
            &registry,
            "public",
            &config,
        )
        .unwrap();
        let plan = converted.debug_plan.expect("debug plan collected");

        assert_eq!(
            plan.predicates[0].expected_index,
            Some("idx_ref_identifier".to_string())
        );
        assert!(plan.predicates[0].sql_shape.contains("identifier_system"));

        let built = converted.builder.with_raw_resource(true).build().unwrap();
        assert!(
            built.sql.contains("ref_kind = 4") && built.sql.contains("identifier_value"),
            "reference :identifier runtime path should use reference sidecar, got: {}",
            built.sql
        );

        let json = serde_json::to_string(&plan).unwrap();
        assert!(
            !json.contains("hospital.example") && !json.contains("abc"),
            "debug output must stay redacted: {json}"
        );
    }

    #[test]
    fn fold_ge_le_emits_single_overlap() {
        let registry = date_registry();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "birthdate",
            &["ge1980-01-01".to_string(), "le2000-01-01".to_string()],
            &registry,
            "Patient",
        )
        .unwrap();
        assert!(folded, "ge+le must fold");
        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("sid.rng && tstzrange(")
                && clause.contains("'[)')")
                && clause.matches("EXISTS").count() == 1,
            "folded clause must have one EXISTS with `&& tstzrange(.., '[)')`: {clause}"
        );
        assert!(
            !clause.contains(" AND EXISTS"),
            "no second EXISTS allowed in folded form: {clause}"
        );
    }

    #[test]
    fn fold_gt_lt_uses_upper_q_for_lo() {
        // gt q ↔ r && [upper(q), +∞)  — inclusive at upper(q)
        // lt q ↔ r && (-∞, lower(q))  — exclusive at lower(q)
        // → combined window `[upper(gt_val), lower(lt_val))` = bounds `[)`.
        let registry = date_registry();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "date",
            &["gt2024-01-01".to_string(), "lt2025-01-01".to_string()],
            &registry,
            "Encounter",
        )
        .unwrap();
        assert!(folded, "gt+lt must fold");
        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("'[)'"),
            "gt + lt → `'[)'` (inclusive lo, exclusive hi): {clause}"
        );
        // gt2024-01-01 → lo at upper(q) = 2024-01-02. Verify via bound param.
        let params = builder.params();
        assert!(
            params.iter().any(|p| matches!(
                p,
                crate::sql_builder::SqlParam::Timestamp(s) if s.starts_with("2024-01-02")
            )),
            "gt2024-01-01 must bind lo at upper(q)=2024-01-02, params: {params:?}"
        );
        // lt2025-01-01 → hi at lower(q) = 2025-01-01.
        assert!(
            params.iter().any(|p| matches!(
                p,
                crate::sql_builder::SqlParam::Timestamp(s) if s.starts_with("2025-01-01")
            )),
            "lt2025-01-01 must bind hi at lower(q)=2025-01-01, params: {params:?}"
        );
    }

    #[test]
    fn fold_takes_strictest_lo_when_ge_repeated() {
        let registry = date_registry();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "birthdate",
            &[
                "ge1980-01-01".to_string(),
                "ge1990-06-15".to_string(),
                "le2010-01-01".to_string(),
            ],
            &registry,
            "Patient",
        )
        .unwrap();
        assert!(folded, "ge+ge+le must fold");
        let clause = builder.build_where_clause().unwrap();
        let params = builder.params();
        // The lo param must be the *later* of the two ge values (strictest).
        let lo_param = params
            .iter()
            .find_map(|p| match p {
                crate::sql_builder::SqlParam::Timestamp(s) if s.starts_with("1990-06-15") => {
                    Some(s.clone())
                }
                _ => None,
            })
            .expect("strictest lo (1990-06-15) must be bound");
        assert!(
            !clause.is_empty() && !lo_param.is_empty(),
            "expected lo param 1990-06-15 to be present"
        );
        // The looser 1980-01-01 must NOT be a parameter.
        assert!(
            !params.iter().any(|p| matches!(
                p,
                crate::sql_builder::SqlParam::Timestamp(s) if s.starts_with("1980-01-01")
            )),
            "looser lo 1980-01-01 should not survive the fold"
        );
    }

    #[test]
    fn fold_refuses_single_value() {
        let registry = date_registry();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "birthdate",
            &["ge1980-01-01".to_string()],
            &registry,
            "Patient",
        )
        .unwrap();
        assert!(!folded, "single value must fall through to per-entry path");
    }

    #[test]
    fn fold_refuses_when_only_one_side_present() {
        let registry = date_registry();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "birthdate",
            &["ge1980-01-01".to_string(), "ge1990-01-01".to_string()],
            &registry,
            "Patient",
        )
        .unwrap();
        assert!(!folded, "two ge with no upper bound must not fold");
    }

    #[test]
    fn fold_refuses_eq_or_ne_mixed_in() {
        let registry = date_registry();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "birthdate",
            &["ge1980-01-01".to_string(), "ne1995".to_string()],
            &registry,
            "Patient",
        )
        .unwrap();
        assert!(!folded, "eq/ne not foldable with prefix bounds");
    }

    #[test]
    fn fold_refuses_modifier_in_key() {
        let registry = date_registry();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "birthdate:missing",
            &["true".to_string(), "false".to_string()],
            &registry,
            "Patient",
        )
        .unwrap();
        assert!(!folded, ":modifier blocks fold");
    }

    #[test]
    fn fold_refuses_comma_or_list() {
        let registry = date_registry();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "birthdate",
            &[
                "ge1980-01-01,ge1990-01-01".to_string(),
                "le2000-01-01".to_string(),
            ],
            &registry,
            "Patient",
        )
        .unwrap();
        assert!(!folded, "comma-OR list blocks fold");
    }

    #[test]
    fn fold_refuses_last_updated() {
        let registry = SearchParameterRegistry::new();
        crate::common::register_common_parameters(&registry);
        // `_lastUpdated` is a common param; it maps to `r.updated_at`, not
        // search_idx_date, so the fold MUST refuse it even if both bounds match.
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "_lastUpdated",
            &["ge2024-01-01".to_string(), "le2024-12-31".to_string()],
            &registry,
            "Patient",
        )
        .unwrap();
        assert!(!folded, "_lastUpdated must never fold to search_idx_date");
    }

    #[test]
    fn fold_refuses_non_date_param() {
        let registry = SearchParameterRegistry::new();
        // No date registration for `value-quantity` → fold refuses.
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let folded = try_fold_repeated_date_window(
            &mut builder,
            None,
            "value-quantity",
            &["ge1.0".to_string(), "le2.0".to_string()],
            &registry,
            "Observation",
        )
        .unwrap();
        assert!(!folded, "non-date params must not fold");
    }
}
