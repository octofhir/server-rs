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

use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::{
    ir::NumberClause, ir::QuantityClause, ir::render_number_clauses_as_or,
    ir::render_number_index_clauses_as_or, ir::render_quantity_clauses_as_or,
    ir::render_quantity_containment_clauses_as_or, ir::render_quantity_index_clauses_as_or,
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
struct DecimalParts {
    mantissa: i128,
    scale: u32,
}

#[cfg(test)]
impl DecimalParts {
    fn parse(input: &str) -> Result<Self, SqlBuilderError> {
        let raw = input.trim();
        if raw.is_empty() {
            return Err(invalid_number(input));
        }

        let (negative, unsigned) = match raw.as_bytes()[0] {
            b'+' => (false, &raw[1..]),
            b'-' => (true, &raw[1..]),
            _ => (false, raw),
        };
        if unsigned.is_empty() {
            return Err(invalid_number(input));
        }

        let mut digits = String::new();
        let mut scale = 0_u32;
        let mut seen_dot = false;
        let mut seen_digit = false;

        for ch in unsigned.chars() {
            match ch {
                '0'..='9' => {
                    seen_digit = true;
                    digits.push(ch);
                    if seen_dot {
                        scale += 1;
                    }
                }
                '.' if !seen_dot => {
                    seen_dot = true;
                }
                _ => return Err(invalid_number(input)),
            }
        }

        if !seen_digit {
            return Err(invalid_number(input));
        }

        let mut mantissa = digits.parse::<i128>().map_err(|_| invalid_number(input))?;
        if negative {
            mantissa = -mantissa;
        }

        Ok(Self { mantissa, scale })
    }

    fn implicit_eq_bounds(&self) -> (String, String) {
        let scale = self.scale + 1;
        let centered = self.mantissa * 10;
        (
            format_decimal(centered - 5, scale),
            format_decimal(centered + 5, scale),
        )
    }
}

#[cfg(test)]
fn invalid_number(value: &str) -> SqlBuilderError {
    SqlBuilderError::InvalidSearchValue(format!("Invalid number: {value}"))
}

#[cfg(test)]
fn format_decimal(mantissa: i128, scale: u32) -> String {
    let negative = mantissa < 0;
    let digits = mantissa.abs().to_string();

    if scale == 0 {
        return if negative {
            format!("-{digits}")
        } else {
            digits
        };
    }

    let scale = scale as usize;
    let value = if digits.len() > scale {
        let split = digits.len() - scale;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("0.{}{}", "0".repeat(scale - digits.len()), digits)
    };
    let trimmed = value.trim_end_matches('0').trim_end_matches('.');

    if negative && trimmed != "0" {
        format!("-{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// Build SQL conditions for number search.
///
/// Number parameters use prefixes to specify comparison operators.
/// The default (eq) considers implicit precision of the value.
pub fn build_number_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = NumberClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_number_clauses_as_or(builder, &clauses, jsonb_path)? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build number search against `search_idx_number`.
pub fn build_index_number_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = NumberClause::from_parsed_param(param, resource_type)?;
    if let Some(sql) = render_number_index_clauses_as_or(builder, &clauses)? {
        builder.add_condition(sql);
    }
    Ok(())
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
    let clauses = QuantityClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_quantity_clauses_as_or(builder, &clauses, jsonb_path)? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build search for Quantity types with a full-resource containment prefilter
/// for system/code constraints.
pub fn build_gin_quantity_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
    path_segments: &[String],
) -> Result<(), SqlBuilderError> {
    let clauses = QuantityClause::from_parsed_param(param, "")?;
    if let Some(sql) =
        render_quantity_containment_clauses_as_or(builder, &clauses, jsonb_path, path_segments)?
    {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build quantity search against `search_idx_quantity`.
pub fn build_index_quantity_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = QuantityClause::from_parsed_param(param, resource_type)?;
    if let Some(sql) = render_quantity_index_clauses_as_or(builder, &clauses)? {
        builder.add_condition(sql);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::{SearchModifier, SearchPrefix};
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
    fn test_decimal_implicit_precision_bounds_are_exact_half_open() {
        let cases = [
            ("100", "99.5", "100.5"),
            ("5.5", "5.45", "5.55"),
            ("5.50", "5.495", "5.505"),
            ("5.500", "5.4995", "5.5005"),
            ("-5.5", "-5.55", "-5.45"),
        ];

        for (value, expected_lower, expected_upper) in cases {
            let number = DecimalParts::parse(value).unwrap();
            assert_eq!(
                number.implicit_eq_bounds(),
                (expected_lower.to_string(), expected_upper.to_string())
            );
        }
    }

    #[test]
    fn test_number_eq_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "5.5", Some(SearchPrefix::Eq));

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains(">= $1::numeric"));
        assert!(clause.contains("< $2::numeric"));
        assert!(!clause.contains("BETWEEN"));
        assert_eq!(builder.param_count(), 2);
        assert_eq!(builder.params()[0].as_str(), "5.45");
        assert_eq!(builder.params()[1].as_str(), "5.55");
    }

    #[test]
    fn test_number_gt_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "100", Some(SearchPrefix::Gt));

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("> $1::numeric"));
        assert_eq!(builder.params()[0].as_str(), "100");
    }

    #[test]
    fn test_number_default_is_eq() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "100", None);

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains(">= $1::numeric"));
        assert!(clause.contains("< $2::numeric"));
    }

    #[test]
    fn test_number_ap_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "100", Some(SearchPrefix::Ap));

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains(">= $1::numeric"));
        assert!(clause.contains("< $2::numeric"));
        assert!(!clause.contains("BETWEEN"));

        // Check approximate range (10% of 100 = 10)
        let params = builder.params();
        assert_eq!(params[0].as_str(), "90");
        assert_eq!(params[1].as_str(), "110");
    }

    #[test]
    fn test_number_ne_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("value", "5", Some(SearchPrefix::Ne));

        build_number_search(&mut builder, &param, "resource->>'value'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("< $1::numeric"));
        assert!(clause.contains(">= $2::numeric"));
        assert!(!clause.contains("NOT BETWEEN"));
        assert_eq!(builder.params()[0].as_str(), "4.5");
        assert_eq!(builder.params()[1].as_str(), "5.5");
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
    fn test_gin_quantity_with_unit() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "value-quantity".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: Some(SearchPrefix::Ge),
                raw: "100|http://unitsofmeasure.org|mm[Hg]".to_string(),
            }],
        };

        build_gin_quantity_search(
            &mut builder,
            &param,
            "resource->'valueQuantity'",
            &["valueQuantity".to_string()],
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource @>"));
        assert!(clause.contains("::numeric >= $1::numeric"));
        assert!(!clause.contains("resource->'valueQuantity'->>'system'"));
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
