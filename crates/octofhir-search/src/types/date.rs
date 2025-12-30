//! Date search parameter implementation.
//!
//! Date search supports the following prefixes:
//! - eq: equal (default) - within the precision range
//! - ne: not equal - outside the precision range
//! - gt: greater than the upper bound
//! - lt: less than the lower bound
//! - ge: greater or equal to lower bound
//! - le: less than or equal to upper bound
//! - sa: starts after the upper bound
//! - eb: ends before the lower bound
//! - ap: approximately (10% range around the period)
//!
//! Date precision is determined by the input format:
//! - Year: 2023 -> [2023-01-01, 2024-01-01)
//! - Month: 2023-01 -> [2023-01-01, 2023-02-01)
//! - Day: 2023-01-15 -> [2023-01-15, 2023-01-16)
//! - DateTime: full instant

use crate::parameters::{SearchModifier, SearchPrefix};
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time};

/// Represents a date range based on precision.
#[derive(Debug, Clone)]
pub struct DateRange {
    pub start: OffsetDateTime,
    pub end: OffsetDateTime,
}

/// Build SQL conditions for date search.
///
/// Date parameters use prefixes to specify comparison operators.
/// The precision of the input affects the range matching.
pub fn build_date_search(
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
        let date_range = parse_date_range(&value.raw)?;

        let condition = build_date_condition(builder, jsonb_path, prefix, &date_range)?;
        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

/// Build missing condition for date fields.
fn build_missing_condition(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    if let Some(value) = param.values.first() {
        let is_missing = value.raw.eq_ignore_ascii_case("true");
        let condition = if is_missing {
            format!("({jsonb_path} IS NULL OR {jsonb_path} = 'null' OR {jsonb_path} = '\"\"')")
        } else {
            format!(
                "({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null' AND {jsonb_path} != '\"\"')"
            )
        };
        builder.add_condition(condition);
    }
    Ok(())
}

/// Build a SQL condition for date comparison.
///
/// Note: Parameters are bound as text strings and must be explicitly cast to
/// timestamptz in the SQL to ensure proper type comparison.
fn build_date_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    prefix: SearchPrefix,
    range: &DateRange,
) -> Result<String, SqlBuilderError> {
    let start_str = format_datetime(&range.start);
    let end_str = format_datetime(&range.end);

    let condition = match prefix {
        SearchPrefix::Eq => {
            // Resource value overlaps with parameter range
            // resource.start < param.end AND resource.end >= param.start
            let p1 = builder.add_timestamp_param(&start_str);
            let p2 = builder.add_timestamp_param(&end_str);
            format!(
                "(({jsonb_path})::timestamptz >= ${p1}::timestamptz AND ({jsonb_path})::timestamptz < ${p2}::timestamptz)"
            )
        }

        SearchPrefix::Ne => {
            // Resource value does NOT overlap with parameter range
            let p1 = builder.add_timestamp_param(&start_str);
            let p2 = builder.add_timestamp_param(&end_str);
            format!(
                "NOT (({jsonb_path})::timestamptz >= ${p1}::timestamptz AND ({jsonb_path})::timestamptz < ${p2}::timestamptz)"
            )
        }

        SearchPrefix::Gt => {
            // Resource value is after parameter range
            let p = builder.add_timestamp_param(&end_str);
            format!("({jsonb_path})::timestamptz >= ${p}::timestamptz")
        }

        SearchPrefix::Lt => {
            // Resource value is before parameter range
            let p = builder.add_timestamp_param(&start_str);
            format!("({jsonb_path})::timestamptz < ${p}::timestamptz")
        }

        SearchPrefix::Ge => {
            // Resource value >= parameter start
            let p = builder.add_timestamp_param(&start_str);
            format!("({jsonb_path})::timestamptz >= ${p}::timestamptz")
        }

        SearchPrefix::Le => {
            // Resource value < parameter end
            let p = builder.add_timestamp_param(&end_str);
            format!("({jsonb_path})::timestamptz < ${p}::timestamptz")
        }

        SearchPrefix::Sa => {
            // Starts after - same as gt for point-in-time
            let p = builder.add_timestamp_param(&end_str);
            format!("({jsonb_path})::timestamptz >= ${p}::timestamptz")
        }

        SearchPrefix::Eb => {
            // Ends before - same as lt for point-in-time
            let p = builder.add_timestamp_param(&start_str);
            format!("({jsonb_path})::timestamptz < ${p}::timestamptz")
        }

        SearchPrefix::Ap => {
            // Approximate: expand the range by 10% on each side
            let duration = range.end - range.start;
            let expansion = duration / 10;
            let approx_start = range.start - expansion;
            let approx_end = range.end + expansion;

            let p1 = builder.add_timestamp_param(format_datetime(&approx_start));
            let p2 = builder.add_timestamp_param(format_datetime(&approx_end));
            format!(
                "(({jsonb_path})::timestamptz >= ${p1}::timestamptz AND ({jsonb_path})::timestamptz < ${p2}::timestamptz)"
            )
        }
    };

    Ok(condition)
}

/// Parse a date string into a range based on precision.
///
/// Supported formats:
/// - Year: 2023
/// - Year-Month: 2023-01
/// - Date: 2023-01-15
/// - DateTime: 2023-01-15T10:30:00
/// - DateTime with TZ: 2023-01-15T10:30:00Z or 2023-01-15T10:30:00+05:00
pub fn parse_date_range(date_str: &str) -> Result<DateRange, SqlBuilderError> {
    let trimmed = date_str.trim();

    // Year only: 2023
    if trimmed.len() == 4 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        let year: i32 = trimmed
            .parse()
            .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid year: {trimmed}")))?;

        let start = Date::from_calendar_date(year, Month::January, 1)
            .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid date: {e}")))?
            .with_time(Time::MIDNIGHT)
            .assume_utc();

        let end = Date::from_calendar_date(year + 1, Month::January, 1)
            .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid date: {e}")))?
            .with_time(Time::MIDNIGHT)
            .assume_utc();

        return Ok(DateRange { start, end });
    }

    // Year-Month: 2023-01
    if trimmed.len() == 7 && trimmed.chars().nth(4) == Some('-') {
        let parts: Vec<&str> = trimmed.split('-').collect();
        if parts.len() == 2 {
            let year: i32 = parts[0].parse().map_err(|_| {
                SqlBuilderError::InvalidSearchValue(format!("Invalid year: {}", parts[0]))
            })?;
            let month_num: u8 = parts[1].parse().map_err(|_| {
                SqlBuilderError::InvalidSearchValue(format!("Invalid month: {}", parts[1]))
            })?;

            let month = Month::try_from(month_num).map_err(|_| {
                SqlBuilderError::InvalidSearchValue(format!("Invalid month number: {month_num}"))
            })?;

            let start = Date::from_calendar_date(year, month, 1)
                .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid date: {e}")))?
                .with_time(Time::MIDNIGHT)
                .assume_utc();

            let end = if month_num == 12 {
                Date::from_calendar_date(year + 1, Month::January, 1)
            } else {
                Date::from_calendar_date(year, Month::try_from(month_num + 1).unwrap(), 1)
            }
            .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid date: {e}")))?
            .with_time(Time::MIDNIGHT)
            .assume_utc();

            return Ok(DateRange { start, end });
        }
    }

    // Full date: 2023-01-15
    if trimmed.len() == 10 && !trimmed.contains('T') {
        let date = Date::parse(
            trimmed,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid date: {e}")))?;

        let start = date.with_time(Time::MIDNIGHT).assume_utc();
        let end = start + Duration::days(1);

        return Ok(DateRange { start, end });
    }

    // DateTime formats
    parse_datetime_range(trimmed)
}

/// Parse a datetime string into a range.
fn parse_datetime_range(dt_str: &str) -> Result<DateRange, SqlBuilderError> {
    // Try parsing as RFC3339 / ISO8601 with timezone
    if dt_str.contains('T') {
        // Try parsing with timezone (Z or +/-offset)
        if dt_str.ends_with('Z')
            || dt_str.contains('+')
            || dt_str.rfind('-').is_some_and(|i| i > 10)
        {
            let dt = OffsetDateTime::parse(dt_str, &time::format_description::well_known::Rfc3339)
                .map_err(|e| {
                    SqlBuilderError::InvalidSearchValue(format!("Invalid datetime: {e}"))
                })?;

            let end = dt + Duration::seconds(1);
            return Ok(DateRange { start: dt, end });
        }

        // DateTime without timezone - assume UTC
        // Format: 2023-01-15T10:30:00 or 2023-01-15T10:30
        let format_with_seconds =
            time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
        let format_no_seconds =
            time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]");

        let pdt = PrimitiveDateTime::parse(dt_str, &format_with_seconds)
            .or_else(|_| PrimitiveDateTime::parse(dt_str, &format_no_seconds))
            .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid datetime: {e}")))?;

        let dt = pdt.assume_utc();
        let end = dt + Duration::seconds(1);
        return Ok(DateRange { start: dt, end });
    }

    Err(SqlBuilderError::InvalidSearchValue(format!(
        "Unrecognized date format: {dt_str}"
    )))
}

/// Format a datetime as ISO8601/RFC3339 string.
fn format_datetime(dt: &OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

/// Build search for Period types which have start and end fields.
///
/// A Period overlaps with the search range if:
/// - period.start < search.end AND (period.end IS NULL OR period.end >= search.start)
pub fn build_period_search(
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
        let date_range = parse_date_range(&value.raw)?;

        let condition = build_period_condition(builder, jsonb_path, prefix, &date_range)?;
        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

/// Build a SQL condition for Period comparison.
///
/// Note: Parameters are bound as text strings and must be explicitly cast to
/// timestamptz in the SQL to ensure proper type comparison.
fn build_period_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    prefix: SearchPrefix,
    range: &DateRange,
) -> Result<String, SqlBuilderError> {
    let start_str = format_datetime(&range.start);
    let end_str = format_datetime(&range.end);

    let start_path = format!("{jsonb_path}->>'start'");
    let end_path = format!("{jsonb_path}->>'end'");

    let condition = match prefix {
        SearchPrefix::Eq => {
            // Period overlaps with search range
            let p1 = builder.add_timestamp_param(&start_str);
            let p2 = builder.add_timestamp_param(&end_str);
            format!(
                "(({start_path} IS NULL OR ({start_path})::timestamptz < ${p2}::timestamptz) AND \
                 ({end_path} IS NULL OR ({end_path})::timestamptz >= ${p1}::timestamptz))"
            )
        }

        SearchPrefix::Ne => {
            let p1 = builder.add_timestamp_param(&start_str);
            let p2 = builder.add_timestamp_param(&end_str);
            format!(
                "NOT (({start_path} IS NULL OR ({start_path})::timestamptz < ${p2}::timestamptz) AND \
                 ({end_path} IS NULL OR ({end_path})::timestamptz >= ${p1}::timestamptz))"
            )
        }

        SearchPrefix::Gt | SearchPrefix::Sa => {
            // Period starts after search range
            let p = builder.add_timestamp_param(&end_str);
            format!("({start_path})::timestamptz >= ${p}::timestamptz")
        }

        SearchPrefix::Lt | SearchPrefix::Eb => {
            // Period ends before search range
            let p = builder.add_timestamp_param(&start_str);
            format!("({end_path} IS NOT NULL AND ({end_path})::timestamptz < ${p}::timestamptz)")
        }

        SearchPrefix::Ge => {
            let p = builder.add_timestamp_param(&start_str);
            format!(
                "(({start_path})::timestamptz >= ${p}::timestamptz OR \
                 ({end_path} IS NOT NULL AND ({end_path})::timestamptz >= ${p}::timestamptz))"
            )
        }

        SearchPrefix::Le => {
            let p = builder.add_timestamp_param(&end_str);
            format!(
                "(({start_path} IS NULL OR ({start_path})::timestamptz < ${p}::timestamptz) AND \
                 ({end_path} IS NULL OR ({end_path})::timestamptz < ${p}::timestamptz))"
            )
        }

        SearchPrefix::Ap => {
            let duration = range.end - range.start;
            let expansion = duration / 10;
            let approx_start = range.start - expansion;
            let approx_end = range.end + expansion;

            let p1 = builder.add_timestamp_param(format_datetime(&approx_start));
            let p2 = builder.add_timestamp_param(format_datetime(&approx_end));
            format!(
                "(({start_path} IS NULL OR ({start_path})::timestamptz < ${p2}::timestamptz) AND \
                 ({end_path} IS NULL OR ({end_path})::timestamptz >= ${p1}::timestamptz))"
            )
        }
    };

    Ok(condition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedValue;
    use time::UtcOffset;

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
    fn test_parse_year() {
        let range = parse_date_range("2023").unwrap();
        assert_eq!(range.start.year(), 2023);
        assert_eq!(range.start.month(), Month::January);
        assert_eq!(range.start.day(), 1);
        assert_eq!(range.end.year(), 2024);
    }

    #[test]
    fn test_parse_year_month() {
        let range = parse_date_range("2023-06").unwrap();
        assert_eq!(range.start.year(), 2023);
        assert_eq!(range.start.month(), Month::June);
        assert_eq!(range.end.month(), Month::July);
    }

    #[test]
    fn test_parse_date() {
        let range = parse_date_range("2023-06-15").unwrap();
        assert_eq!(range.start.day(), 15);
        assert_eq!(range.end.day(), 16);
    }

    #[test]
    fn test_parse_datetime_with_z() {
        let range = parse_date_range("2023-06-15T10:30:00Z").unwrap();
        assert_eq!(range.start.hour(), 10);
        assert_eq!(range.start.minute(), 30);
        assert_eq!(range.start.offset(), UtcOffset::UTC);
    }

    #[test]
    fn test_date_eq_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("date", "2023-06-15", Some(SearchPrefix::Eq));

        build_date_search(&mut builder, &param, "resource->>'date'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains(">="));
        assert!(clause.contains("<"));
    }

    #[test]
    fn test_date_gt_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("date", "2023-06-15", Some(SearchPrefix::Gt));

        build_date_search(&mut builder, &param, "resource->>'date'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains(">="));
    }

    #[test]
    fn test_date_lt_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("date", "2023-06-15", Some(SearchPrefix::Lt));

        build_date_search(&mut builder, &param, "resource->>'date'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("<"));
    }

    #[test]
    fn test_date_missing_modifier() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "date".to_string(),
            modifier: Some(SearchModifier::Missing),
            values: vec![ParsedValue {
                prefix: None,
                raw: "true".to_string(),
            }],
        };

        build_date_search(&mut builder, &param, "resource->>'date'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("IS NULL"));
    }

    #[test]
    fn test_period_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("period", "2023-06", Some(SearchPrefix::Eq));

        build_period_search(&mut builder, &param, "resource->'period'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("start"));
        assert!(clause.contains("end"));
    }

    #[test]
    fn test_invalid_date_returns_error() {
        let result = parse_date_range("not-a-date");
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_date_values() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "date".to_string(),
            modifier: None,
            values: vec![
                ParsedValue {
                    prefix: Some(SearchPrefix::Ge),
                    raw: "2023-01-01".to_string(),
                },
                ParsedValue {
                    prefix: Some(SearchPrefix::Le),
                    raw: "2023-12-31".to_string(),
                },
            ],
        };

        build_date_search(&mut builder, &param, "resource->>'date'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains(" OR "));
    }
}
