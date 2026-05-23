//! Date search parameter implementation.
//!
//! See `docs/architecture/date-search.md` for the design.
//!
//! Every FHIR date-shaped value is canonicalised to a half-open `[lower, upper)`
//! UTC interval. Queries are issued against `search_idx_date` using `tstzrange`
//! operators so the GiST index covers every FHIR prefix:
//!
//! - `eq` (`<@`) — resource value entirely inside query range
//! - `ne` — NOT eq (`NOT EXISTS … <@`)
//! - `gt` (`&& [upper(q), +∞)`) — range above q intersects r (`upper(r) > upper(q)`)
//! - `lt` (`&& (-∞, lower(q))`) — range below q intersects r (`lower(r) < lower(q)`)
//! - `ge` (`&& [lower(q), +∞)`) — r intersects from q.lo onward (`upper(r) > lower(q)`)
//! - `le` (`&& (-∞, upper(q))`) — r intersects strictly before q.hi (`lower(r) < upper(q)`)
//! - `sa` (`>>`) — target strictly starts after q (`lower(r) >= upper(q)`)
//! - `eb` (`<<`) — target strictly ends before q (`upper(r) <= lower(q)`)
//! - `ap` (`&&`) — overlaps a ±10 % expansion of the query range
//!
//! Every operator is GiST-indexable via the standard `range_ops` opclass — no
//! `NOT (…)` wrappers, so the planner always has the option of a Bitmap Index
//! Scan on `search_idx_date_*_param_code_rng_idx`.
//!
//! `&>` / `&<` are NOT equivalent to `NOT(<<)` / `NOT(>>)` on wide Periods —
//! they compare against the *opposite* bound of `q`. Do not substitute them.

use crate::parameters::{SearchModifier, SearchPrefix};
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::types::date_ast::DateClause;
use crate::{ir::render_date_clauses_as_or, ir::rewrite_date_clauses};
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time};

/// Half-open `[start, end)` UTC range built from a FHIR date / dateTime /
/// instant search parameter value.
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

/// Parse a FHIR date / dateTime / instant string into a half-open `[start, end)`
/// UTC `DateRange` whose width matches the value's stated precision.
///
/// Accepts every FHIR R4 surface form:
/// - `2028`
/// - `2028-05`
/// - `2028-05-15`
/// - `2028-05-15T14:30`
/// - `2028-05-15T14:30:45`
/// - `2028-05-15T14:30:45.123` (and any sub-second precision)
/// - any of the above with `Z`, `+HH:MM`, `-HH:MM`, `+HHMM`
pub fn parse_date_range(date_str: &str) -> Result<DateRange, SqlBuilderError> {
    let trimmed = date_str.trim();

    // Year only: 2028
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

    // Year-Month: 2028-05
    if trimmed.len() == 7 && trimmed.chars().nth(4) == Some('-') {
        let year: i32 = trimmed[..4]
            .parse()
            .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid year: {trimmed}")))?;
        let month_num: u8 = trimmed[5..7].parse().map_err(|_| {
            SqlBuilderError::InvalidSearchValue(format!("Invalid month: {trimmed}"))
        })?;
        let month = Month::try_from(month_num).map_err(|_| {
            SqlBuilderError::InvalidSearchValue(format!("Invalid month: {trimmed}"))
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

    // Full date: 2028-05-15
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

    // DateTime — any precision, any timezone (or none).
    parse_datetime_range(trimmed)
}

/// Parse a FHIR dateTime / instant string into a half-open UTC range whose
/// width is the value's stated precision (minute / second / sub-second).
fn parse_datetime_range(dt_str: &str) -> Result<DateRange, SqlBuilderError> {
    if !dt_str.contains('T') {
        return Err(SqlBuilderError::InvalidSearchValue(format!(
            "Unrecognized date format: {dt_str}"
        )));
    }

    // Split TZ suffix. TZ marker: trailing 'Z', or '+'/'-' after position 10.
    let bytes = dt_str.as_bytes();
    let mut tz_pos: Option<usize> = None;
    if bytes.last() == Some(&b'Z') {
        tz_pos = Some(dt_str.len() - 1);
    } else {
        for (i, b) in bytes.iter().enumerate().skip(11) {
            if *b == b'+' || *b == b'-' {
                tz_pos = Some(i);
                break;
            }
        }
    }
    let (body, tz) = match tz_pos {
        Some(p) => (&dt_str[..p], &dt_str[p..]),
        None => (dt_str, ""),
    };

    if body.len() < 16 || body.as_bytes().get(10) != Some(&b'T') {
        return Err(SqlBuilderError::InvalidSearchValue(format!(
            "Unrecognized date format: {dt_str}"
        )));
    }

    let year: i32 = body[..4]
        .parse()
        .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid year: {dt_str}")))?;
    let month: u8 = body[5..7]
        .parse()
        .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid month: {dt_str}")))?;
    let day: u8 = body[8..10]
        .parse()
        .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid day: {dt_str}")))?;
    let hour: u8 = body[11..13]
        .parse()
        .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid hour: {dt_str}")))?;
    let minute: u8 = body[14..16]
        .parse()
        .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid minute: {dt_str}")))?;

    let (sec, frac_str, precision): (u8, &str, Precision) = if body.len() == 16 {
        (0, "", Precision::Minute)
    } else if body.len() >= 19 && body.as_bytes().get(16) == Some(&b':') {
        let s: u8 = body[17..19].parse().map_err(|_| {
            SqlBuilderError::InvalidSearchValue(format!("Invalid second: {dt_str}"))
        })?;
        if body.len() == 19 {
            (s, "", Precision::Second)
        } else if body.as_bytes().get(19) == Some(&b'.') {
            let frac = &body[20..];
            if frac.is_empty() || !frac.chars().all(|c| c.is_ascii_digit()) {
                return Err(SqlBuilderError::InvalidSearchValue(format!(
                    "Invalid fractional second: {dt_str}"
                )));
            }
            let p = match frac.len() {
                1..=3 => Precision::Milli,
                4..=6 => Precision::Micro,
                _ => Precision::Micro, // 7+ digits clipped to µs upper
            };
            (s, frac, p)
        } else {
            return Err(SqlBuilderError::InvalidSearchValue(format!(
                "Unrecognized date format: {dt_str}"
            )));
        }
    } else {
        return Err(SqlBuilderError::InvalidSearchValue(format!(
            "Unrecognized date format: {dt_str}"
        )));
    };

    let month_e = Month::try_from(month)
        .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid month: {dt_str}")))?;
    let date = Date::from_calendar_date(year, month_e, day)
        .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid date: {e}")))?;
    let time = Time::from_hms(hour, minute, sec)
        .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid time: {e}")))?;
    let pdt = PrimitiveDateTime::new(date, time);

    let off_minutes = parse_offset_minutes(tz).ok_or_else(|| {
        SqlBuilderError::InvalidSearchValue(format!("Invalid timezone offset in {dt_str}"))
    })?;
    let offset = time::UtcOffset::from_whole_seconds(off_minutes * 60)
        .map_err(|e| SqlBuilderError::InvalidSearchValue(format!("Invalid offset: {e}")))?;
    let mut start = pdt.assume_offset(offset).to_offset(time::UtcOffset::UTC);

    if !frac_str.is_empty() {
        let mut buf = String::from(frac_str);
        if buf.len() > 9 {
            buf.truncate(9);
        } else {
            while buf.len() < 9 {
                buf.push('0');
            }
        }
        let ns: i64 = buf.parse().unwrap_or(0);
        start += Duration::nanoseconds(ns);
    }

    let end = match precision {
        Precision::Minute => start + Duration::minutes(1),
        Precision::Second => start + Duration::seconds(1),
        Precision::Milli => start + Duration::milliseconds(1),
        Precision::Micro => start + Duration::microseconds(1),
    };
    Ok(DateRange { start, end })
}

#[derive(Copy, Clone)]
enum Precision {
    Minute,
    Second,
    Milli,
    Micro,
}

fn parse_offset_minutes(tz: &str) -> Option<i32> {
    if tz.is_empty() || tz == "Z" {
        return Some(0);
    }
    let bytes = tz.as_bytes();
    let sign: i32 = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &tz[1..];
    let (hh, mm) = if let Some(colon) = rest.find(':') {
        let h: i32 = rest[..colon].parse().ok()?;
        let m: i32 = rest[colon + 1..].parse().ok()?;
        (h, m)
    } else if rest.len() == 4 {
        let h: i32 = rest[..2].parse().ok()?;
        let m: i32 = rest[2..].parse().ok()?;
        (h, m)
    } else if rest.len() == 2 {
        let h: i32 = rest.parse().ok()?;
        (h, 0)
    } else {
        return None;
    };
    Some(sign * (hh * 60 + mm))
}

/// Format a datetime as ISO8601/RFC3339 string.
fn format_datetime(dt: &OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

/// Build a date search against the `search_idx_date` table using `tstzrange`
/// range operators. One EXISTS subquery per value; values are OR-combined.
///
/// FHIR prefix → range-operator mapping (see architecture doc §4):
///
/// | Prefix | Truth condition | Operator |
/// |---|---|---|
/// | `eq` (default) | `r ⊆ q`       | `r.rng <@ q` |
/// | `ne` | `NOT (r ⊆ q)`         | `NOT EXISTS … <@ q` |
/// | `gt` | `lower(r) >= upper(q)` | `r.rng >> q` |
/// | `lt` | `upper(r) <= lower(q)` | `r.rng << q` |
/// | `ge` | `upper(r) > lower(q)`  | `NOT (r.rng << q)` |
/// | `le` | `lower(r) < upper(q)`  | `NOT (r.rng >> q)` |
/// | `sa` | `lower(r) >= upper(q)` | `r.rng >> q` |
/// | `eb` | `upper(r) <= lower(q)` | `r.rng << q` |
/// | `ap` | overlaps ±10 % window | `r.rng && q_apx` |
pub fn build_index_date_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    // :missing modifier — match resources that have / don't have any indexed
    // value for this search parameter.
    if let Some(SearchModifier::Missing) = &param.modifier {
        let is_missing = param
            .values
            .first()
            .map(|v| v.raw.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let rt_param = builder.add_text_param(resource_type);
        let pc_param = builder.add_text_param(&param.name);
        let id_col = builder.id_column();

        let condition = if is_missing {
            format!(
                "NOT EXISTS (SELECT 1 FROM search_idx_date sid \
                 WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                 AND sid.param_code = ${pc_param})"
            )
        } else {
            format!(
                "EXISTS (SELECT 1 FROM search_idx_date sid \
                 WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                 AND sid.param_code = ${pc_param})"
            )
        };
        builder.add_condition(condition);
        return Ok(());
    }

    let clauses = rewrite_date_clauses(DateClause::from_parsed_param(param, resource_type)?);
    if let Some(sql) = render_date_clauses_as_or(builder, &clauses) {
        builder.add_condition(sql);
    }

    Ok(())
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

    // Per-prefix SQL shape for the sidecar path. Guards against
    // `Gt|Sa` / `Lt|Eb` collapse and `NOT (rng <{<,>}> q)` regressions.

    fn index_clause(prefix: SearchPrefix, raw: &str) -> String {
        let mut builder = SqlBuilder::new();
        let param = make_param("date", raw, Some(prefix));
        build_index_date_search(&mut builder, &param, "Patient").unwrap();
        builder.build_where_clause().unwrap()
    }

    #[test]
    fn index_eq_uses_contains() {
        let clause = index_clause(SearchPrefix::Eq, "2024-06-15");
        assert!(
            clause.contains("sid.rng <@"),
            "eq must emit `sid.rng <@ q`, got: {clause}"
        );
    }

    #[test]
    fn index_ne_uses_not_exists_contains() {
        let mut builder = SqlBuilder::new();
        let param = make_param("date", "2024-06-15", Some(SearchPrefix::Ne));
        build_index_date_search(&mut builder, &param, "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("NOT EXISTS") && clause.contains("sid.rng <@"),
            "ne must emit `NOT EXISTS (… sid.rng <@ q)` anti-join, got: {clause}"
        );
    }

    #[test]
    fn index_gt_uses_overlap_above_q() {
        let clause = index_clause(SearchPrefix::Gt, "2024-06-15");
        assert!(
            clause.contains("sid.rng &&") && clause.contains("NULL, '[)')"),
            "gt must emit `sid.rng && tstzrange(upper(q), NULL, '[)')`, got: {clause}"
        );
        // gt must NOT regress to `>>` (which is `sa` semantics).
        assert!(
            !clause.contains("sid.rng >>"),
            "gt regressed to `>>` (collapse with `sa`): {clause}"
        );
    }

    #[test]
    fn index_lt_uses_overlap_below_q() {
        let clause = index_clause(SearchPrefix::Lt, "2024-06-15");
        assert!(
            clause.contains("sid.rng &&") && clause.contains("tstzrange(NULL,"),
            "lt must emit `sid.rng && tstzrange(NULL, lower(q), '()')`, got: {clause}"
        );
        assert!(
            !clause.contains("sid.rng <<"),
            "lt regressed to `<<` (collapse with `eb`): {clause}"
        );
    }

    #[test]
    fn index_ge_uses_overlap_from_q_lo() {
        let clause = index_clause(SearchPrefix::Ge, "2024-06-15");
        assert!(
            clause.contains("sid.rng &&") && clause.contains("NULL, '[)')"),
            "ge must emit `sid.rng && tstzrange(lower(q), NULL, '[)')`, got: {clause}"
        );
        // ge must NOT regress to the GiST-unfriendly `NOT (sid.rng << q)` shape.
        assert!(
            !clause.contains("NOT (sid.rng <<"),
            "ge regressed to NOT(<<) which defeats GiST: {clause}"
        );
        // ge must NOT be silently rewritten as `&>` (incorrect on wide Periods).
        assert!(
            !clause.contains("sid.rng &>"),
            "ge MUST NOT use `&>` — semantically wrong on wide Periods; got: {clause}"
        );
    }

    #[test]
    fn index_le_uses_overlap_to_q_hi() {
        let clause = index_clause(SearchPrefix::Le, "2024-06-15");
        assert!(
            clause.contains("sid.rng &&") && clause.contains("tstzrange(NULL,"),
            "le must emit `sid.rng && tstzrange(NULL, upper(q), '()')`, got: {clause}"
        );
        assert!(
            !clause.contains("NOT (sid.rng >>"),
            "le regressed to NOT(>>) which defeats GiST: {clause}"
        );
        assert!(
            !clause.contains("sid.rng &<"),
            "le MUST NOT use `&<` — semantically wrong on wide Periods; got: {clause}"
        );
    }

    #[test]
    fn index_sa_uses_strictly_after() {
        let clause = index_clause(SearchPrefix::Sa, "2024-06-15");
        assert!(
            clause.contains("sid.rng >>"),
            "sa must emit `sid.rng >> q`, got: {clause}"
        );
        // sa must NOT collapse with gt's overlap form.
        assert!(
            !clause.contains("sid.rng && tstzrange"),
            "sa must not use overlap-with-half-infinite (that's gt): {clause}"
        );
    }

    #[test]
    fn index_eb_uses_strictly_before() {
        let clause = index_clause(SearchPrefix::Eb, "2024-06-15");
        assert!(
            clause.contains("sid.rng <<"),
            "eb must emit `sid.rng << q`, got: {clause}"
        );
        assert!(
            !clause.contains("sid.rng && tstzrange"),
            "eb must not use overlap-with-half-infinite (that's lt): {clause}"
        );
    }

    #[test]
    fn index_ap_uses_overlap() {
        let clause = index_clause(SearchPrefix::Ap, "2024-06-15");
        assert!(
            clause.contains("sid.rng &&"),
            "ap must emit `sid.rng && expanded_q`, got: {clause}"
        );
    }

    #[test]
    fn index_no_bare_not_in_where_clause() {
        // Guard against the Goal-3 NOT antipattern: `NOT (range_op)` in WHERE.
        // `NOT EXISTS (subquery)` is allowed (anti-join); the inline form is not.
        for prefix in [
            SearchPrefix::Eq,
            SearchPrefix::Gt,
            SearchPrefix::Lt,
            SearchPrefix::Ge,
            SearchPrefix::Le,
            SearchPrefix::Sa,
            SearchPrefix::Eb,
            SearchPrefix::Ap,
        ] {
            let clause = index_clause(prefix, "2024-06-15");
            assert!(
                !clause.contains("NOT (sid.rng"),
                "prefix {prefix:?} emitted `NOT (sid.rng …)` antipattern: {clause}"
            );
        }
    }
}
