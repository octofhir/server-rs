//! Number search parameter implementation.
//!
//! Number search supports the following prefixes:
//! - eq: equal (default) - uses implicit precision range
//! - ne: not equal
//! - gt: greater than
//! - lt: less than
//! - ge: greater or equal
//! - le: less or equal
//! - sa: starts after (same as gt for numbers)
//! - eb: ends before (same as lt for numbers)
//! - ap: approximately (10% range)

use crate::parameters::{SearchModifier, SearchPrefix};
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};

/// Build SQL conditions for number search.
///
/// Number parameters use prefixes to specify comparison operators.
/// The default (eq) considers implicit precision of the value.
pub fn build_number_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    // Check for :missing modifier first
    if let Some(SearchModifier::Missing) = &param.modifier {
        return build_missing_condition(builder, param, jsonb_path);
    }

    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let prefix = value.prefix.unwrap_or(SearchPrefix::Eq);
        let num_str = &value.raw;

        let num: f64 = num_str.parse().map_err(|_| {
            SqlBuilderError::InvalidSearchValue(format!("Invalid number: {num_str}"))
        })?;

        let condition = match prefix {
            SearchPrefix::Eq => {
                // Per FHIR spec, eq includes implicit precision range
                let precision = calculate_precision(num_str);
                let lower = num - precision;
                let upper = num + precision;
                let p1 = builder.add_float_param(lower);
                let p2 = builder.add_float_param(upper);
                format!("({jsonb_path})::numeric BETWEEN ${p1} AND ${p2}")
            }

            SearchPrefix::Ne => {
                // Not equal - outside the precision range
                let precision = calculate_precision(num_str);
                let lower = num - precision;
                let upper = num + precision;
                let p1 = builder.add_float_param(lower);
                let p2 = builder.add_float_param(upper);
                format!("({jsonb_path})::numeric NOT BETWEEN ${p1} AND ${p2}")
            }

            SearchPrefix::Gt | SearchPrefix::Sa => {
                // Greater than
                let p = builder.add_float_param(num);
                format!("({jsonb_path})::numeric > ${p}")
            }

            SearchPrefix::Lt | SearchPrefix::Eb => {
                // Less than
                let p = builder.add_float_param(num);
                format!("({jsonb_path})::numeric < ${p}")
            }

            SearchPrefix::Ge => {
                // Greater or equal
                let p = builder.add_float_param(num);
                format!("({jsonb_path})::numeric >= ${p}")
            }

            SearchPrefix::Le => {
                // Less or equal
                let p = builder.add_float_param(num);
                format!("({jsonb_path})::numeric <= ${p}")
            }

            SearchPrefix::Ap => {
                // Approximate: 10% range
                let range = num.abs() * 0.1;
                let lower = num - range;
                let upper = num + range;
                let p1 = builder.add_float_param(lower);
                let p2 = builder.add_float_param(upper);
                format!("({jsonb_path})::numeric BETWEEN ${p1} AND ${p2}")
            }
        };

        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

/// Build missing condition for number fields.
fn build_missing_condition(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    if let Some(value) = param.values.first() {
        let is_missing = value.raw.eq_ignore_ascii_case("true");
        let condition = if is_missing {
            format!("({jsonb_path} IS NULL OR {jsonb_path} = 'null')")
        } else {
            format!("({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null')")
        };
        builder.add_condition(condition);
    }
    Ok(())
}

/// Calculate the implicit precision based on significant figures.
///
/// Per FHIR spec, a number like "5.5" has implicit range [5.45, 5.55)
/// and "100" has range [99.5, 100.5).
fn calculate_precision(num_str: &str) -> f64 {
    // Remove leading/trailing whitespace and sign
    let cleaned = num_str.trim().trim_start_matches(['+', '-']);

    if let Some(dot_pos) = cleaned.find('.') {
        // Has decimal point - precision based on decimal places
        let decimals = cleaned.len() - dot_pos - 1;
        0.5 * 10f64.powi(-(decimals as i32))
    } else {
        // Integer - precision is 0.5
        0.5
    }
}

/// Build search for Quantity types.
///
/// Quantity has value, unit, system, and code fields.
/// Search format: [prefix]number|system|code or [prefix]number||code or just [prefix]number
pub fn build_quantity_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    // Check for :missing modifier first
    if let Some(SearchModifier::Missing) = &param.modifier {
        return build_missing_condition(builder, param, jsonb_path);
    }

    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let prefix = value.prefix.unwrap_or(SearchPrefix::Eq);

        // Parse quantity format: number|system|code
        let (num_str, system, code) = parse_quantity_value(&value.raw);

        let num: f64 = num_str.parse().map_err(|_| {
            SqlBuilderError::InvalidSearchValue(format!("Invalid number in quantity: {num_str}"))
        })?;

        // Build the numeric condition
        let num_condition = build_numeric_condition(
            builder,
            &format!("{jsonb_path}->>'value'"),
            prefix,
            num,
            num_str,
        );

        // Add system/code constraints if present
        let condition = if system.is_some() || code.is_some() {
            let mut constraints = vec![num_condition];

            if let Some(sys) = system
                && !sys.is_empty()
            {
                let p = builder.add_text_param(sys);
                constraints.push(format!("{jsonb_path}->>'system' = ${p}"));
            }

            if let Some(c) = code
                && !c.is_empty()
            {
                let p = builder.add_text_param(c);
                constraints.push(format!(
                    "({jsonb_path}->>'code' = ${p} OR {jsonb_path}->>'unit' = ${p})"
                ));
            }

            format!("({})", constraints.join(" AND "))
        } else {
            num_condition
        };

        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

/// Parse a quantity search value into number, system, and code parts.
fn parse_quantity_value(value: &str) -> (&str, Option<&str>, Option<&str>) {
    let parts: Vec<&str> = value.splitn(3, '|').collect();

    match parts.len() {
        1 => (parts[0], None, None),
        2 => (parts[0], None, Some(parts[1])),
        _ => (parts[0], Some(parts[1]), Some(parts[2])),
    }
}

/// Build a numeric comparison condition with the given prefix.
fn build_numeric_condition(
    builder: &mut SqlBuilder,
    path: &str,
    prefix: SearchPrefix,
    num: f64,
    num_str: &str,
) -> String {
    match prefix {
        SearchPrefix::Eq => {
            let precision = calculate_precision(num_str);
            let lower = num - precision;
            let upper = num + precision;
            let p1 = builder.add_float_param(lower);
            let p2 = builder.add_float_param(upper);
            format!("({path})::numeric BETWEEN ${p1} AND ${p2}")
        }

        SearchPrefix::Ne => {
            let precision = calculate_precision(num_str);
            let lower = num - precision;
            let upper = num + precision;
            let p1 = builder.add_float_param(lower);
            let p2 = builder.add_float_param(upper);
            format!("({path})::numeric NOT BETWEEN ${p1} AND ${p2}")
        }

        SearchPrefix::Gt | SearchPrefix::Sa => {
            let p = builder.add_float_param(num);
            format!("({path})::numeric > ${p}")
        }

        SearchPrefix::Lt | SearchPrefix::Eb => {
            let p = builder.add_float_param(num);
            format!("({path})::numeric < ${p}")
        }

        SearchPrefix::Ge => {
            let p = builder.add_float_param(num);
            format!("({path})::numeric >= ${p}")
        }

        SearchPrefix::Le => {
            let p = builder.add_float_param(num);
            format!("({path})::numeric <= ${p}")
        }

        SearchPrefix::Ap => {
            let range = num.abs() * 0.1;
            let lower = num - range;
            let upper = num + range;
            let p1 = builder.add_float_param(lower);
            let p2 = builder.add_float_param(upper);
            format!("({path})::numeric BETWEEN ${p1} AND ${p2}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedValue;

    fn make_param(name: &str, value: &str, prefix: Option<SearchPrefix>) -> ParsedParam {
        ParsedParam {
            name: name.to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix,
                raw: value.to_string(),
            }],
        }
    }

    #[test]
    fn test_calculate_precision() {
        assert!((calculate_precision("100") - 0.5).abs() < 0.001);
        assert!((calculate_precision("5.5") - 0.05).abs() < 0.001);
        assert!((calculate_precision("5.50") - 0.005).abs() < 0.001);
        assert!((calculate_precision("5.500") - 0.0005).abs() < 0.0001);
    }

    #[test]
    fn test_number_eq_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "5.5", Some(SearchPrefix::Eq));

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("BETWEEN"));
        assert_eq!(builder.param_count(), 2);
    }

    #[test]
    fn test_number_gt_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "100", Some(SearchPrefix::Gt));

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("> $1"));
    }

    #[test]
    fn test_number_default_is_eq() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "100", None);

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("BETWEEN"));
    }

    #[test]
    fn test_number_ap_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "100", Some(SearchPrefix::Ap));

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("BETWEEN"));

        // Check approximate range (10% of 100 = 10)
        let params = builder.params();
        let lower: f64 = params[0].as_str().parse().unwrap();
        let upper: f64 = params[1].as_str().parse().unwrap();
        assert!((lower - 90.0).abs() < 0.001);
        assert!((upper - 110.0).abs() < 0.001);
    }

    #[test]
    fn test_number_ne_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "5", Some(SearchPrefix::Ne));

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("NOT BETWEEN"));
    }

    #[test]
    fn test_quantity_with_unit() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "value-quantity".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: Some(SearchPrefix::Gt),
                raw: "5.5|http://unitsofmeasure.org|mg".to_string(),
            }],
        };

        build_quantity_search(&mut builder, &param, "resource->'valueQuantity'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("system"));
        assert!(clause.contains("code"));
    }

    #[test]
    fn test_invalid_number_returns_error() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "abc", None);

        let result = build_number_search(&mut builder, &param, "resource->>'value'");
        assert!(result.is_err());
    }

    #[test]
    fn test_number_missing_modifier() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "value".to_string(),
            modifier: Some(SearchModifier::Missing),
            values: vec![ParsedValue {
                prefix: None,
                raw: "true".to_string(),
            }],
        };

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("IS NULL"));
    }
}
