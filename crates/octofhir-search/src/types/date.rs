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

use crate::ir::render_date_clauses_as_or;
use crate::ir::render_date_text_path_clauses_as_or;
#[cfg(test)]
use crate::ir::render_period_path_clauses_as_or;
use crate::parameters::SearchModifier;
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::types::date_ast::DateClause;
#[cfg(test)]
use crate::types::date_ast::PeriodClause;
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

    let clauses = DateClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_date_text_path_clauses_as_or(builder, &clauses, jsonb_path) {
        builder.add_condition(sql);
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

    let clauses = DateClause::from_parsed_param(param, resource_type)?;
    if let Some(sql) = render_date_clauses_as_or(builder, &clauses) {
        builder.add_condition(sql);
    }

    Ok(())
}

/// Build search for Period types which have start and end fields.
///
/// A Period overlaps with the search range if:
/// - period.start < search.end AND (period.end IS NULL OR period.end >= search.start)
#[cfg(test)]
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

    let clauses = PeriodClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_period_path_clauses_as_or(builder, &clauses, jsonb_path) {
        builder.add_condition(sql);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::{SearchModifier, SearchPrefix};
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

    fn make_missing_param(name: &str, value: &str) -> ParsedParam {
        ParsedParam {
            name: name.to_string(),
            modifier: Some(SearchModifier::Missing),
            values: vec![ParsedValue {
                prefix: None,
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
    fn test_date_ne_search_uses_positive_range_split() {
        let mut builder = SqlBuilder::new();
        let param = make_param("date", "2023-06-15", Some(SearchPrefix::Ne));

        build_date_search(&mut builder, &param, "resource->>'date'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert_eq!(
            clause,
            "((resource->>'date')::timestamptz < $1::timestamptz OR (resource->>'date')::timestamptz >= $2::timestamptz)"
        );
        assert!(!clause.contains("NOT"));
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
    fn test_period_ne_search_uses_positive_split() {
        let mut builder = SqlBuilder::new();
        let param = make_param("period", "2023-06", Some(SearchPrefix::Ne));

        build_period_search(&mut builder, &param, "resource->'period'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource->'period'->>'start' IS NOT NULL"));
        assert!(clause.contains("resource->'period'->>'end' IS NOT NULL"));
        assert!(clause.contains(" OR "));
        assert!(!clause.contains("NOT ("));
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
    fn index_ge_uses_overlap_plus_containment() {
        // FHIR R4 §3.1.1.5.1: `ge` = (range above search overlaps target)
        // OR (search contains target). Emitted as an OR of `&&` and `<@`.
        let clause = index_clause(SearchPrefix::Ge, "2024-06-15");
        assert!(
            clause.contains("sid.rng &&") && clause.contains("sid.rng <@"),
            "ge must emit `(sid.rng && …) OR (sid.rng <@ …)`, got: {clause}"
        );
        assert!(
            !clause.contains("NOT (sid.rng <<"),
            "ge regressed to NOT(<<) which defeats GiST: {clause}"
        );
        assert!(
            !clause.contains("sid.rng &>"),
            "ge MUST NOT use `&>` — semantically wrong on wide Periods; got: {clause}"
        );
    }

    #[test]
    fn index_le_uses_overlap_plus_containment() {
        // FHIR R4 §3.1.1.5.1: `le` = (range below search overlaps target)
        // OR (search contains target). Emitted as an OR of `&&` and `<@`.
        let clause = index_clause(SearchPrefix::Le, "2024-06-15");
        assert!(
            clause.contains("sid.rng &&") && clause.contains("sid.rng <@"),
            "le must emit `(sid.rng && …) OR (sid.rng <@ …)`, got: {clause}"
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
    fn index_sa_uses_strict_after_comparison() {
        // sa requires a strict gap (`lower(t) > upper(s)`) so a boundary
        // touch does not qualify.
        let clause = index_clause(SearchPrefix::Sa, "2024-06-15");
        assert!(
            clause.contains("lower(sid.rng) >"),
            "sa must emit `lower(sid.rng) > upper(q)`, got: {clause}"
        );
        assert!(
            !clause.contains("sid.rng && tstzrange"),
            "sa must not use overlap-with-half-infinite (that's gt): {clause}"
        );
    }

    #[test]
    fn index_eb_uses_strict_before_comparison() {
        // eb mirrors sa: `upper(t) < lower(s)` so boundary touches don't
        // qualify.
        let clause = index_clause(SearchPrefix::Eb, "2024-06-15");
        assert!(
            clause.contains("upper(sid.rng) <"),
            "eb must emit `upper(sid.rng) < lower(q)`, got: {clause}"
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
    fn index_missing_true_uses_date_sidecar_anti_join() {
        let mut builder = SqlBuilder::new();
        let param = make_missing_param("birthdate", "true");
        build_index_date_search(&mut builder, &param, "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("NOT EXISTS"));
        assert!(clause.contains("search_idx_date"));
        assert!(clause.contains("sid.param_code ="));
        assert!(!clause.contains("sid.rng"));
    }

    #[test]
    fn index_missing_false_uses_date_sidecar_exists() {
        let mut builder = SqlBuilder::new();
        let param = make_missing_param("birthdate", "false");
        build_index_date_search(&mut builder, &param, "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("EXISTS"));
        assert!(!clause.contains("NOT EXISTS"));
        assert!(clause.contains("search_idx_date"));
        assert!(!clause.contains("sid.rng"));
    }

    #[test]
    fn index_comma_or_keeps_prefix_per_value() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "birthdate".to_string(),
            modifier: None,
            values: vec![
                ParsedValue {
                    prefix: Some(SearchPrefix::Gt),
                    raw: "2005-01-01".to_string(),
                },
                ParsedValue {
                    prefix: Some(SearchPrefix::Lt),
                    raw: "1975-01-01".to_string(),
                },
            ],
        };
        build_index_date_search(&mut builder, &param, "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains(" OR "));
        assert!(clause.contains("tstzrange($3::timestamptz, NULL, '[)')"));
        assert!(clause.contains("tstzrange(NULL, $6::timestamptz, '[)'"));
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
