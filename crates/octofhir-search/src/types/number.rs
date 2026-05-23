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

#[derive(Debug, Clone, PartialEq, Eq)]
struct DecimalParts {
    mantissa: i128,
    scale: u32,
}

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

    fn format(&self) -> String {
        format_decimal(self.mantissa, self.scale)
    }

    fn implicit_eq_bounds(&self) -> (String, String) {
        let scale = self.scale + 1;
        let centered = self.mantissa * 10;
        (
            format_decimal(centered - 5, scale),
            format_decimal(centered + 5, scale),
        )
    }

    fn approximate_bounds(&self) -> (String, String) {
        let scale = self.scale + 1;
        let centered = self.mantissa * 10;
        let delta = self.mantissa.abs();
        (
            format_decimal(centered - delta, scale),
            format_decimal(centered + delta, scale),
        )
    }
}

fn invalid_number(value: &str) -> SqlBuilderError {
    SqlBuilderError::InvalidSearchValue(format!("Invalid number: {value}"))
}

fn invalid_quantity_number(value: &str) -> SqlBuilderError {
    SqlBuilderError::InvalidSearchValue(format!("Invalid number in quantity: {value}"))
}

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

fn bind_numeric(builder: &mut SqlBuilder, value: impl Into<String>) -> usize {
    builder.add_text_param(value.into())
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
        let number = DecimalParts::parse(num_str)?;

        let condition = match prefix {
            SearchPrefix::Eq => {
                let (lower, upper) = number.implicit_eq_bounds();
                let p1 = bind_numeric(builder, lower);
                let p2 = bind_numeric(builder, upper);
                format!(
                    "(({jsonb_path})::numeric >= ${p1}::numeric AND ({jsonb_path})::numeric < ${p2}::numeric)"
                )
            }

            SearchPrefix::Ne => {
                let (lower, upper) = number.implicit_eq_bounds();
                let p1 = bind_numeric(builder, lower);
                let p2 = bind_numeric(builder, upper);
                format!(
                    "(({jsonb_path})::numeric < ${p1}::numeric OR ({jsonb_path})::numeric >= ${p2}::numeric)"
                )
            }

            SearchPrefix::Gt | SearchPrefix::Sa => {
                let p = bind_numeric(builder, number.format());
                format!("({jsonb_path})::numeric > ${p}::numeric")
            }

            SearchPrefix::Lt | SearchPrefix::Eb => {
                let p = bind_numeric(builder, number.format());
                format!("({jsonb_path})::numeric < ${p}::numeric")
            }

            SearchPrefix::Ge => {
                let p = bind_numeric(builder, number.format());
                format!("({jsonb_path})::numeric >= ${p}::numeric")
            }

            SearchPrefix::Le => {
                let p = bind_numeric(builder, number.format());
                format!("({jsonb_path})::numeric <= ${p}::numeric")
            }

            SearchPrefix::Ap => {
                let (lower, upper) = number.approximate_bounds();
                let p1 = bind_numeric(builder, lower);
                let p2 = bind_numeric(builder, upper);
                format!(
                    "(({jsonb_path})::numeric >= ${p1}::numeric AND ({jsonb_path})::numeric < ${p2}::numeric)"
                )
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

        let number = DecimalParts::parse(num_str).map_err(|_| invalid_quantity_number(num_str))?;

        // Build the numeric condition
        let num_condition =
            build_numeric_condition(builder, &format!("{jsonb_path}->>'value'"), prefix, &number);

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

/// Parse a quantity search value into (number, system, code) parts per
/// FHIR R4 §3.1.1.5.7 (search.html#quantity).
///
/// Format: `value[|system[|code]]`. The system and code components are
/// positional — the first `|`-delimited field after the value is the system,
/// the second is the code. Empty fields collapse to `None` so callers can
/// distinguish "any value" from a literal match.
///
/// Examples:
///   "5.4"             → ("5.4", None,        None)
///   "5.4|"            → ("5.4", None,        None)         // trailing pipe = any system
///   "5.4|http://x"    → ("5.4", Some("…"),   None)
///   "5.4|http://x|kg" → ("5.4", Some("…"),   Some("kg"))
///   "5.4||kg"         → ("5.4", None,        Some("kg"))   // any system, fixed code
fn parse_quantity_value(value: &str) -> (&str, Option<&str>, Option<&str>) {
    let parts: Vec<&str> = value.splitn(3, '|').collect();
    let num = parts[0];
    let system = parts.get(1).copied().filter(|s| !s.is_empty());
    let code = parts.get(2).copied().filter(|s| !s.is_empty());
    (num, system, code)
}

/// Build a numeric comparison condition with the given prefix.
fn build_numeric_condition(
    builder: &mut SqlBuilder,
    path: &str,
    prefix: SearchPrefix,
    number: &DecimalParts,
) -> String {
    match prefix {
        SearchPrefix::Eq => {
            let (lower, upper) = number.implicit_eq_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            format!("(({path})::numeric >= ${p1}::numeric AND ({path})::numeric < ${p2}::numeric)")
        }

        SearchPrefix::Ne => {
            let (lower, upper) = number.implicit_eq_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            format!("(({path})::numeric < ${p1}::numeric OR ({path})::numeric >= ${p2}::numeric)")
        }

        SearchPrefix::Gt | SearchPrefix::Sa => {
            let p = bind_numeric(builder, number.format());
            format!("({path})::numeric > ${p}::numeric")
        }

        SearchPrefix::Lt | SearchPrefix::Eb => {
            let p = bind_numeric(builder, number.format());
            format!("({path})::numeric < ${p}::numeric")
        }

        SearchPrefix::Ge => {
            let p = bind_numeric(builder, number.format());
            format!("({path})::numeric >= ${p}::numeric")
        }

        SearchPrefix::Le => {
            let p = bind_numeric(builder, number.format());
            format!("({path})::numeric <= ${p}::numeric")
        }

        SearchPrefix::Ap => {
            let (lower, upper) = number.approximate_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            format!("(({path})::numeric >= ${p1}::numeric AND ({path})::numeric < ${p2}::numeric)")
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
