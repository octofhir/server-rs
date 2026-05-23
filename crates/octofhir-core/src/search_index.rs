//! Search index extraction engine.
//!
//! Extracts reference, date, and string values from FHIR resource JSON
//! for indexing in denormalized search tables.

use crate::fhir_reference::{NormalizedRef, normalize_reference_for_index};
use serde_json::Value;

/// An extracted reference ready for indexing.
#[derive(Debug, Clone)]
pub struct ExtractedReference {
    pub param_code: String,
    pub normalized: NormalizedRef,
    pub raw_reference: Option<String>,
}

/// An extracted date range ready for indexing.
#[derive(Debug, Clone)]
pub struct ExtractedDate {
    pub param_code: String,
    pub range_start: String, // ISO8601 timestamp
    pub range_end: String,   // ISO8601 timestamp
}

/// An extracted string value ready for indexing.
#[derive(Debug, Clone)]
pub struct ExtractedString {
    pub param_code: String,
    pub value_normalized: String,
    pub value_exact: String,
}

// ============================================================================
// Reference Extraction
// ============================================================================

/// Extract references from a FHIR resource using a search parameter's FHIRPath expression.
///
/// Navigates the resource JSON to find reference values, normalizes them, and
/// returns a list of extracted references for indexing.
pub fn extract_references(
    resource: &Value,
    resource_type: &str,
    param_code: &str,
    expression: &str,
    base_url: Option<&str>,
) -> Vec<ExtractedReference> {
    let path_segments = fhirpath_to_segments(expression, resource_type);
    if path_segments.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut values = Vec::new();
    navigate_json(resource, &path_segments, 0, &mut values);

    for ref_value in values {
        let raw_ref = ref_value
            .get("reference")
            .and_then(|r| r.as_str())
            .map(String::from);

        let normalized_refs = normalize_reference_for_index(&ref_value, base_url);
        for normalized in normalized_refs {
            results.push(ExtractedReference {
                param_code: param_code.to_string(),
                normalized,
                raw_reference: raw_ref.clone(),
            });
        }
    }

    results
}

// ============================================================================
// Date Extraction
// ============================================================================

/// Maximum number of `Timing.event[]` entries indexed per resource for one
/// search parameter. Larger schedules are truncated and a warning is logged.
/// Bounds write fanout for multi-year dose schedules where indexing every
/// event yields no extra query value (FHIR date search matches outer limits,
/// not enumerated occurrences).
pub const MAX_TIMING_EVENTS: usize = 64;

/// Extract date values from a FHIR resource for indexing.
///
/// Every value is collapsed to a half-open `[lower, upper)` range in UTC and
/// stored as one row in `search_idx_date`. Cardinality > 1 elements
/// (`Timing.event[]`, repeating Periods, multi-value extensions) emit one row
/// per element — the index is never collapsed to a min/max envelope.
///
/// `Timing.repeat.boundsPeriod` is also indexed when present, as a single
/// Period row (open ends become `±infinity`). `boundsDuration`, `boundsRange`,
/// and recurrence-only schedules without finite bounds are not indexed —
/// they have no concrete calendar window.
pub fn extract_dates(
    resource: &Value,
    resource_type: &str,
    param_code: &str,
    expression: &str,
) -> Vec<ExtractedDate> {
    let path_segments = fhirpath_to_segments(expression, resource_type);
    if path_segments.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut values = Vec::new();
    navigate_json(resource, &path_segments, 0, &mut values);

    for date_value in values {
        emit_date_rows(&date_value, param_code, &mut results);
    }

    results
}

/// Push one or more `ExtractedDate` rows from a FHIR date-shaped value.
///
/// Dispatches on the JSON shape:
/// - Object with `event` → Timing (one row per event)
/// - Object with `start`/`end` → Period (one row, half-open, ±infinity for open ends)
/// - String → date / dateTime / instant (one row)
/// - Array → recurse (handles repeating Periods / Timings)
fn emit_date_rows(value: &Value, param_code: &str, results: &mut Vec<ExtractedDate>) {
    match value {
        Value::Array(arr) => {
            for v in arr {
                emit_date_rows(v, param_code, results);
            }
        }
        Value::Object(_) => {
            // Timing: index every concrete `event[]` plus the outer
            // `repeat.boundsPeriod` window if present.
            let mut matched_timing = false;

            if let Some(events) = value.get("event").and_then(|e| e.as_array()) {
                matched_timing = true;
                let take = events.len().min(MAX_TIMING_EVENTS);
                if events.len() > MAX_TIMING_EVENTS {
                    tracing::warn!(
                        param_code = param_code,
                        events = events.len(),
                        cap = MAX_TIMING_EVENTS,
                        "Timing.event[] truncated to cap; later events not indexed"
                    );
                }
                for ev in events.iter().take(take) {
                    if let Some(s) = ev.as_str()
                        && let Some(range) = parse_date_to_range(s)
                    {
                        results.push(ExtractedDate {
                            param_code: param_code.to_string(),
                            range_start: range.0,
                            range_end: range.1,
                        });
                    }
                }
            }

            // `boundsDuration` / `boundsRange` have no anchor and are skipped.
            if let Some(bp) = value.get("repeat").and_then(|r| r.get("boundsPeriod"))
                && let Some(range) = parse_period_to_range(bp)
            {
                matched_timing = true;
                results.push(ExtractedDate {
                    param_code: param_code.to_string(),
                    range_start: range.0,
                    range_end: range.1,
                });
            }

            if matched_timing {
                return;
            }

            // Period (closed, open-start, open-end).
            if (value.get("start").is_some() || value.get("end").is_some())
                && let Some(range) = parse_period_to_range(value)
            {
                results.push(ExtractedDate {
                    param_code: param_code.to_string(),
                    range_start: range.0,
                    range_end: range.1,
                });
            }
        }
        Value::String(s) => {
            if let Some(range) = parse_date_to_range(s) {
                results.push(ExtractedDate {
                    param_code: param_code.to_string(),
                    range_start: range.0,
                    range_end: range.1,
                });
            }
        }
        _ => {}
    }
}

/// Parse a FHIR date-shaped string into a half-open `[lower, upper)` range
/// expressed as RFC3339 UTC strings. Returns `None` if the string is not a
/// recognised FHIR date / dateTime / instant form.
///
/// All FHIR partial precisions and sub-second precisions are supported.
pub fn parse_date_to_range(date_str: &str) -> Option<(String, String)> {
    let trimmed = date_str.trim();
    let len = trimmed.len();

    // Year only: "2024" → [2024-01-01, 2025-01-01)
    if len == 4 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        let year: i32 = trimmed.parse().ok()?;
        return Some((
            format!("{year:04}-01-01T00:00:00Z"),
            format!("{:04}-01-01T00:00:00Z", year + 1),
        ));
    }

    // Year-Month: "2024-03" → [2024-03-01, 2024-04-01)
    if len == 7 && trimmed.chars().nth(4) == Some('-') {
        let year: i32 = trimmed[..4].parse().ok()?;
        let month: u32 = trimmed[5..7].parse().ok()?;
        if !(1..=12).contains(&month) {
            return None;
        }
        let (ny, nm) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        return Some((
            format!("{year:04}-{month:02}-01T00:00:00Z"),
            format!("{ny:04}-{nm:02}-01T00:00:00Z"),
        ));
    }

    // Full date: "2024-03-15" → [2024-03-15, 2024-03-16)
    if len == 10 && !trimmed.contains('T') {
        let year: i32 = trimmed[..4].parse().ok()?;
        let month: u32 = trimmed[5..7].parse().ok()?;
        let day: u32 = trimmed[8..10].parse().ok()?;
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        let (ny, nm, nd) = next_day(year, month, day);
        return Some((
            format!("{year:04}-{month:02}-{day:02}T00:00:00Z"),
            format!("{ny:04}-{nm:02}-{nd:02}T00:00:00Z"),
        ));
    }

    // DateTime — parse into UTC instant + precision, compute half-open upper.
    if trimmed.contains('T') {
        return parse_datetime_to_range(trimmed);
    }

    None
}

/// Parse a FHIR dateTime / instant string (must contain 'T') into half-open
/// `[lower, upper)` UTC. Supports any timezone offset (Z, ±HH:MM, ±HHMM, none)
/// and any sub-second precision up to nanoseconds (clipped to µs for the
/// upper bound — Postgres `timestamptz` is µs-precise).
fn parse_datetime_to_range(s: &str) -> Option<(String, String)> {
    // Split TZ suffix from the body. TZ marker is 'Z' or '+' or a '-' that
    // appears after the date+time prefix (date "-" is at positions 4 and 7).
    let bytes = s.as_bytes();
    let mut tz_pos: Option<usize> = None;
    if let Some(last) = bytes.last()
        && *last == b'Z'
    {
        tz_pos = Some(s.len() - 1);
    } else {
        // Look for '+' or '-' after position 10 (past date)
        for (i, b) in bytes.iter().enumerate().skip(11) {
            if *b == b'+' || *b == b'-' {
                tz_pos = Some(i);
                break;
            }
        }
    }

    let (body, tz) = match tz_pos {
        Some(p) => (&s[..p], &s[p..]),
        None => (s, ""),
    };

    // body = YYYY-MM-DDThh:mm[:ss[.fff…]]
    if body.len() < 16 || body.as_bytes().get(10) != Some(&b'T') {
        return None;
    }
    let year: i32 = body[..4].parse().ok()?;
    let month: u32 = body[5..7].parse().ok()?;
    let day: u32 = body[8..10].parse().ok()?;
    let hour: u32 = body[11..13].parse().ok()?;
    let minute: u32 = body[14..16].parse().ok()?;

    let (sec_str, frac_str, precision): (&str, &str, Precision) = if body.len() == 16 {
        ("00", "", Precision::Minute)
    } else if body.len() >= 19 && body.as_bytes().get(16) == Some(&b':') {
        let s_str = &body[17..19];
        if body.len() == 19 {
            (s_str, "", Precision::Second)
        } else if body.as_bytes().get(19) == Some(&b'.') {
            let frac = &body[20..];
            let p = match frac.len() {
                1..=3 => Precision::Milli,
                4..=6 => Precision::Micro,
                _ => Precision::Nano, // 7+ digits — clip upper to µs
            };
            (s_str, frac, p)
        } else {
            return None;
        }
    } else {
        return None;
    };

    let second: u32 = sec_str.parse().ok()?;
    if month == 0
        || month > 12
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    // Convert (year, month, day, hour, minute, second + frac, tz) → UTC offset.
    // Use the `time` crate for offset arithmetic.
    let off_minutes = parse_offset_minutes(tz)?;

    use time::{Date, Duration, Month, PrimitiveDateTime, Time, UtcOffset};
    let m = Month::try_from(month as u8).ok()?;
    let date = Date::from_calendar_date(year, m, day as u8).ok()?;
    // Build a Time at second precision (frac handled separately).
    let t = Time::from_hms(hour as u8, minute as u8, second as u8).ok()?;
    let pdt = PrimitiveDateTime::new(date, t);
    let offset = UtcOffset::from_whole_seconds(off_minutes * 60).ok()?;
    let mut start = pdt.assume_offset(offset).to_offset(UtcOffset::UTC);

    // Apply fractional seconds (truncated to nanoseconds; clipped to µs for printing).
    let mut frac_ns: i64 = 0;
    if !frac_str.is_empty() {
        // Pad/truncate to 9 digits.
        let mut buf = String::from(frac_str);
        if buf.len() > 9 {
            buf.truncate(9);
        } else {
            while buf.len() < 9 {
                buf.push('0');
            }
        }
        frac_ns = buf.parse::<i64>().ok()?;
        start += Duration::nanoseconds(frac_ns);
    }

    let lower = format_utc(start);
    let upper_inst = match precision {
        Precision::Minute => start + Duration::minutes(1),
        Precision::Second => start + Duration::seconds(1),
        Precision::Milli => start + Duration::milliseconds(1),
        Precision::Micro => start + Duration::microseconds(1),
        Precision::Nano => {
            // Stored timestamps are µs-precise. Clip upper to next µs boundary.
            let _ = frac_ns; // already applied
            start + Duration::microseconds(1)
        }
    };
    let upper = format_utc(upper_inst);
    Some((lower, upper))
}

#[derive(Copy, Clone)]
enum Precision {
    Minute,
    Second,
    Milli,
    Micro,
    Nano,
}

fn parse_offset_minutes(tz: &str) -> Option<i32> {
    if tz.is_empty() || tz == "Z" {
        return Some(0);
    }
    // tz is like "+05:30", "-08:00", "+0530", "+05"
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

fn format_utc(dt: time::OffsetDateTime) -> String {
    use time::format_description::well_known::Rfc3339;
    dt.format(&Rfc3339).unwrap_or_else(|_| dt.to_string())
}

fn next_day(year: i32, month: u32, day: u32) -> (i32, u32, u32) {
    if day < days_in_month(year, month) {
        (year, month, day + 1)
    } else if month < 12 {
        (year, month + 1, 1)
    } else {
        (year + 1, 1, 1)
    }
}

/// Parse a FHIR Period into half-open `[lower, upper)` UTC strings.
///
/// Open-start → lower = `-infinity`. Open-end → upper = `infinity`. Both
/// open → `None` (such a Period matches every query and is treated as
/// "no usable bound", consistent with the architecture doc).
fn parse_period_to_range(period: &Value) -> Option<(String, String)> {
    let start = period
        .get("start")
        .and_then(|s| s.as_str())
        .and_then(|s| parse_date_to_range(s).map(|r| r.0));

    let end = period
        .get("end")
        .and_then(|e| e.as_str())
        .and_then(|e| parse_date_to_range(e).map(|r| r.1));

    match (start, end) {
        (Some(s), Some(e)) => Some((s, e)),
        (Some(s), None) => Some((s, "infinity".to_string())),
        (None, Some(e)) => Some(("-infinity".to_string(), e)),
        (None, None) => None,
    }
}

/// Get number of days in a month.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

// ============================================================================
// String Extraction
// ============================================================================

/// Extract string values from a FHIR resource for indexing.
pub fn extract_strings(
    resource: &Value,
    resource_type: &str,
    param_code: &str,
    expression: &str,
) -> Vec<ExtractedString> {
    let path_segments = fhirpath_to_segments(expression, resource_type);
    if path_segments.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut values = Vec::new();
    navigate_json(resource, &path_segments, 0, &mut values);

    for str_value in values {
        // Handle HumanName type
        if str_value.get("family").is_some() || str_value.get("given").is_some() {
            extract_human_name_strings(&str_value, param_code, &mut results);
            continue;
        }

        // Handle Address type
        if str_value.get("line").is_some() || str_value.get("city").is_some() {
            extract_address_strings(&str_value, param_code, &mut results);
            continue;
        }

        // Handle simple string
        if let Some(s) = str_value.as_str()
            && !s.is_empty()
        {
            results.push(ExtractedString {
                param_code: param_code.to_string(),
                value_normalized: normalize_string(s),
                value_exact: s.to_string(),
            });
        }
    }

    results
}

/// Extract string values from a HumanName object.
fn extract_human_name_strings(name: &Value, param_code: &str, results: &mut Vec<ExtractedString>) {
    if let Some(family) = name.get("family").and_then(|f| f.as_str())
        && !family.is_empty()
    {
        results.push(ExtractedString {
            param_code: param_code.to_string(),
            value_normalized: normalize_string(family),
            value_exact: family.to_string(),
        });
    }

    if let Some(given) = name.get("given").and_then(|g| g.as_array()) {
        for g in given {
            if let Some(s) = g.as_str()
                && !s.is_empty()
            {
                results.push(ExtractedString {
                    param_code: param_code.to_string(),
                    value_normalized: normalize_string(s),
                    value_exact: s.to_string(),
                });
            }
        }
    }

    if let Some(text) = name.get("text").and_then(|t| t.as_str())
        && !text.is_empty()
    {
        results.push(ExtractedString {
            param_code: param_code.to_string(),
            value_normalized: normalize_string(text),
            value_exact: text.to_string(),
        });
    }
}

/// Extract string values from an Address object.
fn extract_address_strings(addr: &Value, param_code: &str, results: &mut Vec<ExtractedString>) {
    for field in &["city", "state", "country", "postalCode", "district", "text"] {
        if let Some(val) = addr.get(field).and_then(|v| v.as_str())
            && !val.is_empty()
        {
            results.push(ExtractedString {
                param_code: param_code.to_string(),
                value_normalized: normalize_string(val),
                value_exact: val.to_string(),
            });
        }
    }

    if let Some(lines) = addr.get("line").and_then(|l| l.as_array()) {
        for line in lines {
            if let Some(s) = line.as_str()
                && !s.is_empty()
            {
                results.push(ExtractedString {
                    param_code: param_code.to_string(),
                    value_normalized: normalize_string(s),
                    value_exact: s.to_string(),
                });
            }
        }
    }
}

/// Normalize a string for FHIR R4 search semantics.
///
/// Per FHIR R4 §3.1.1.5.6 (search.html#string), string parameter matching is
/// "by default" case-insensitive and accent-insensitive. We implement this by:
///   1. Lowercasing (Unicode-aware via `char::to_lowercase`).
///   2. Decomposing to NFD so combining marks split from base characters.
///   3. Stripping Unicode combining marks (categories Mn, Mc, Me).
///
/// Examples:
///   "Müller"  → "muller"
///   "García"  → "garcia"
///   "Renée"   → "renee"
///
/// Both indexed values and query values must go through this function so the
/// stored form and the lookup form match.
pub fn normalize_string(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Returns true for Unicode combining marks (general categories Mn, Mc, Me).
///
/// Combining marks are the diacritic glyphs that NFD decomposition splits off
/// from base characters (e.g., "é" → "e" + U+0301 COMBINING ACUTE ACCENT).
/// Stripping them yields accent-insensitive matching.
fn is_combining_mark(c: char) -> bool {
    matches!(
        c as u32,
        // Combining Diacritical Marks
        0x0300..=0x036F
        // Combining Diacritical Marks Extended
        | 0x1AB0..=0x1AFF
        // Combining Diacritical Marks Supplement
        | 0x1DC0..=0x1DFF
        // Combining Diacritical Marks for Symbols
        | 0x20D0..=0x20FF
        // Combining Half Marks
        | 0xFE20..=0xFE2F
    )
}

// ============================================================================
// JSON Navigation Helpers
// ============================================================================

/// Convert a FHIRPath expression to JSON path segments.
///
/// Simplified conversion for common patterns like `Observation.subject`,
/// `Patient.name.family`, etc. Strips resource type prefix and FHIRPath functions.
fn fhirpath_to_segments(expression: &str, resource_type: &str) -> Vec<String> {
    // Handle union expressions — find the one matching our resource type
    let expr = if expression.contains('|') {
        expression
            .split('|')
            .map(|s| s.trim())
            .find(|s| s.starts_with(&format!("{resource_type}.")))
            .or_else(|| expression.split('|').next().map(|s| s.trim()))
            .unwrap_or(expression)
    } else {
        expression
    };

    // Strip `as Type` casting
    let expr = if let Some(idx) = expr.find(" as ") {
        expr[..idx]
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
    } else {
        expr
    };

    // Strip resource type prefix
    let expr = expr
        .strip_prefix(&format!("{resource_type}."))
        .or_else(|| expr.strip_prefix("Resource."))
        .or_else(|| expr.strip_prefix("DomainResource."))
        .or_else(|| {
            if let Some(idx) = expr.find('.') {
                let prefix = &expr[..idx];
                if prefix
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_uppercase())
                    .unwrap_or(false)
                {
                    return Some(&expr[idx + 1..]);
                }
            }
            None
        })
        .unwrap_or(expr);

    // Strip FHIRPath function calls
    let expr = strip_fhirpath_functions(expr);

    expr.split('.')
        .filter(|s| !s.is_empty())
        .map(|s| {
            // Remove array subscripts
            if let Some(base) = s.strip_suffix(']')
                && let Some((name, _)) = base.split_once('[')
            {
                return name.to_string();
            }
            s.to_string()
        })
        .collect()
}

/// Strip FHIRPath function calls, keeping property paths.
fn strip_fhirpath_functions(expr: &str) -> String {
    let mut result = String::with_capacity(expr.len());
    let mut i = 0;
    let bytes = expr.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'(' {
            let mut depth = 1;
            i += 1;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            // Remove function name
            if let Some(dot_pos) = result.rfind('.') {
                let func = &result[dot_pos + 1..];
                if is_fhirpath_function(func) {
                    result.truncate(dot_pos);
                }
            } else if is_fhirpath_function(&result) {
                result.clear();
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    while result.ends_with('.') {
        result.pop();
    }
    while result.starts_with('.') {
        result.remove(0);
    }

    result
}

fn is_fhirpath_function(name: &str) -> bool {
    matches!(
        name,
        "where"
            | "resolve"
            | "ofType"
            | "exists"
            | "empty"
            | "first"
            | "last"
            | "as"
            | "is"
            | "not"
            | "all"
            | "any"
            | "count"
            | "distinct"
            | "single"
            | "type"
    )
}

/// Navigate JSON following path segments, collecting leaf values.
///
/// Handles arrays transparently — if a segment points to an array,
/// recurse into each element. Also handles FHIR polymorphic fields
/// (e.g., `effective` matches `effectiveDateTime`, `effectivePeriod`).
fn navigate_json(value: &Value, segments: &[String], depth: usize, results: &mut Vec<Value>) {
    if depth >= segments.len() {
        // Reached the target — collect this value
        match value {
            Value::Array(arr) => {
                for item in arr {
                    results.push(item.clone());
                }
            }
            Value::Null => {}
            _ => results.push(value.clone()),
        }
        return;
    }

    let segment = &segments[depth];

    match value {
        Value::Object(obj) => {
            if let Some(child) = obj.get(segment.as_str()) {
                navigate_json(child, segments, depth + 1, results);
            } else {
                // Try FHIR polymorphic field names: segment + Type suffix
                // e.g., "effective" matches "effectiveDateTime", "effectivePeriod"
                for (key, child) in obj {
                    if key.len() > segment.len()
                        && key.starts_with(segment.as_str())
                        && key.as_bytes()[segment.len()].is_ascii_uppercase()
                    {
                        navigate_json(child, segments, depth + 1, results);
                    }
                }
            }
        }
        Value::Array(arr) => {
            // Transparently iterate arrays
            for item in arr {
                if let Some(child) = item.get(segment.as_str()) {
                    navigate_json(child, segments, depth + 1, results);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_references_simple() {
        let resource = json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "subject": {
                "reference": "Patient/123"
            }
        });

        let refs = extract_references(
            &resource,
            "Observation",
            "subject",
            "Observation.subject",
            None,
        );
        assert_eq!(refs.len(), 1);
        assert!(
            matches!(&refs[0].normalized, NormalizedRef::Local { target_type, target_id }
            if target_type == "Patient" && target_id == "123")
        );
        assert_eq!(refs[0].param_code, "subject");
    }

    #[test]
    fn test_extract_references_array() {
        let resource = json!({
            "resourceType": "Observation",
            "performer": [
                { "reference": "Practitioner/456" },
                { "reference": "Organization/789" }
            ]
        });

        let refs = extract_references(
            &resource,
            "Observation",
            "performer",
            "Observation.performer",
            None,
        );
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_extract_references_identifier() {
        let resource = json!({
            "resourceType": "Observation",
            "subject": {
                "identifier": {
                    "system": "http://hospital.org",
                    "value": "MRN123"
                }
            }
        });

        let refs = extract_references(
            &resource,
            "Observation",
            "subject",
            "Observation.subject",
            None,
        );
        assert_eq!(refs.len(), 1);
        assert!(
            matches!(&refs[0].normalized, NormalizedRef::Identifier { system, value }
            if system.as_deref() == Some("http://hospital.org") && value == "MRN123")
        );
    }

    #[test]
    fn test_extract_dates_simple() {
        let resource = json!({
            "resourceType": "Observation",
            "effectiveDateTime": "2024-03-15"
        });

        let dates = extract_dates(
            &resource,
            "Observation",
            "date",
            "Observation.effectiveDateTime",
        );
        assert_eq!(dates.len(), 1);
        assert!(dates[0].range_start.starts_with("2024-03-15T00:00:00"));
        // Half-open: upper bound = start of next day
        assert!(dates[0].range_end.starts_with("2024-03-16T00:00:00"));
    }

    #[test]
    fn test_extract_dates_year() {
        let resource = json!({
            "resourceType": "Patient",
            "birthDate": "1990"
        });

        let dates = extract_dates(&resource, "Patient", "birthdate", "Patient.birthDate");
        assert_eq!(dates.len(), 1);
        assert!(dates[0].range_start.starts_with("1990-01-01"));
        // Half-open: upper bound = start of next year
        assert!(dates[0].range_end.starts_with("1991-01-01"));
    }

    #[test]
    fn test_extract_dates_period() {
        let resource = json!({
            "resourceType": "Encounter",
            "period": {
                "start": "2024-01-01",
                "end": "2024-01-15"
            }
        });

        let dates = extract_dates(&resource, "Encounter", "date", "Encounter.period");
        assert_eq!(dates.len(), 1);
        assert!(dates[0].range_start.starts_with("2024-01-01T00:00:00"));
        // Half-open: upper bound = start of day after end
        assert!(dates[0].range_end.starts_with("2024-01-16T00:00:00"));
    }

    #[test]
    fn test_extract_dates_timing_event_per_element() {
        let resource = json!({
            "resourceType": "Observation",
            "effectiveTiming": { "event": ["2026-03-01", "2026-06-15", "2026-09-30"] }
        });
        let dates = extract_dates(&resource, "Observation", "date", "Observation.effective");
        assert_eq!(dates.len(), 3, "one row per event");
        let starts: Vec<_> = dates.iter().map(|d| d.range_start.as_str()).collect();
        assert!(starts.iter().any(|s| s.starts_with("2026-03-01")));
        assert!(starts.iter().any(|s| s.starts_with("2026-06-15")));
        assert!(starts.iter().any(|s| s.starts_with("2026-09-30")));
    }

    #[test]
    fn test_extract_dates_timing_event_capped() {
        // Build a Timing with 100 events — must be truncated to MAX_TIMING_EVENTS.
        let events: Vec<String> = (0..100)
            .map(|i| format!("2024-{:02}-{:02}", (i / 28) + 1, (i % 28) + 1))
            .collect();
        let resource = json!({
            "resourceType": "MedicationStatement",
            "effectiveTiming": { "event": events }
        });
        let dates = extract_dates(
            &resource,
            "MedicationStatement",
            "date",
            "MedicationStatement.effective",
        );
        assert_eq!(
            dates.len(),
            MAX_TIMING_EVENTS,
            "Timing.event[] beyond cap must be dropped"
        );
        // First indexed event must still be the first one — truncation is a tail-drop,
        // not a sample.
        assert!(dates[0].range_start.starts_with("2024-01-01"));
    }

    #[test]
    fn test_extract_dates_timing_bounds_period_indexed() {
        // Timing with only `repeat.boundsPeriod` — no concrete events. Must
        // emit one Period row.
        let resource = json!({
            "resourceType": "MedicationStatement",
            "effectiveTiming": {
                "repeat": {
                    "boundsPeriod": { "start": "2024-03-01", "end": "2024-09-30" },
                    "frequency": 1,
                    "period": 1,
                    "periodUnit": "d"
                }
            }
        });
        let dates = extract_dates(
            &resource,
            "MedicationStatement",
            "date",
            "MedicationStatement.effective",
        );
        assert_eq!(dates.len(), 1, "repeat.boundsPeriod must index as 1 Period");
        assert!(dates[0].range_start.starts_with("2024-03-01"));
        assert!(dates[0].range_end.starts_with("2024-10-01"));
    }

    #[test]
    fn test_extract_dates_timing_events_and_bounds_period_both_indexed() {
        // Both concrete events AND a boundsPeriod present — every event plus the
        // outer window must be indexed.
        let resource = json!({
            "resourceType": "MedicationStatement",
            "effectiveTiming": {
                "event": ["2024-04-01", "2024-05-01"],
                "repeat": {
                    "boundsPeriod": { "start": "2024-03-01", "end": "2024-09-30" }
                }
            }
        });
        let dates = extract_dates(
            &resource,
            "MedicationStatement",
            "date",
            "MedicationStatement.effective",
        );
        assert_eq!(
            dates.len(),
            3,
            "2 events + 1 boundsPeriod = 3 rows, got: {dates:?}"
        );
    }

    #[test]
    fn test_extract_dates_timing_repeat_without_bounds_not_indexed() {
        // Recurrence-only Timing (frequency + period, no event[], no bounds*)
        // has no finite searchable window — must not be indexed.
        let resource = json!({
            "resourceType": "MedicationStatement",
            "effectiveTiming": {
                "repeat": { "frequency": 1, "period": 1, "periodUnit": "d" }
            }
        });
        let dates = extract_dates(
            &resource,
            "MedicationStatement",
            "date",
            "MedicationStatement.effective",
        );
        assert!(
            dates.is_empty(),
            "recurrence-only Timing must not be indexed, got: {dates:?}"
        );
    }

    #[test]
    fn test_extract_dates_timing_bounds_duration_not_indexed() {
        // `boundsDuration` describes an unanchored span — no calendar window,
        // must not be indexed.
        let resource = json!({
            "resourceType": "MedicationStatement",
            "effectiveTiming": {
                "repeat": { "boundsDuration": { "value": 7, "unit": "d" } }
            }
        });
        let dates = extract_dates(
            &resource,
            "MedicationStatement",
            "date",
            "MedicationStatement.effective",
        );
        assert!(
            dates.is_empty(),
            "boundsDuration must not be indexed, got: {dates:?}"
        );
    }

    #[test]
    fn test_extract_dates_period_open_start() {
        let resource = json!({
            "resourceType": "Encounter",
            "period": { "end": "2024-01-15" }
        });
        let dates = extract_dates(&resource, "Encounter", "date", "Encounter.period");
        assert_eq!(dates.len(), 1);
        assert_eq!(dates[0].range_start, "-infinity");
        assert!(dates[0].range_end.starts_with("2024-01-16"));
    }

    #[test]
    fn test_extract_dates_period_open_end() {
        let resource = json!({
            "resourceType": "Encounter",
            "period": { "start": "2024-01-15" }
        });
        let dates = extract_dates(&resource, "Encounter", "date", "Encounter.period");
        assert_eq!(dates.len(), 1);
        assert!(dates[0].range_start.starts_with("2024-01-15"));
        assert_eq!(dates[0].range_end, "infinity");
    }

    #[test]
    fn test_parse_datetime_fractional_no_tz() {
        // FHIR allows fractional seconds without an explicit TZ — historically
        // this rejected; ensure it's accepted now.
        let r = parse_date_to_range("2028-05-15T14:30:45.123").unwrap();
        assert!(r.0.starts_with("2028-05-15T14:30:45.123"));
    }

    #[test]
    fn test_parse_datetime_minute_precision() {
        let r = parse_date_to_range("2028-05-15T14:30").unwrap();
        assert!(r.0.starts_with("2028-05-15T14:30:00"));
        assert!(r.1.starts_with("2028-05-15T14:31:00"));
    }

    #[test]
    fn test_extract_strings_simple() {
        let resource = json!({
            "resourceType": "Patient",
            "name": [{
                "family": "Smith",
                "given": ["John", "James"]
            }]
        });

        let strings = extract_strings(&resource, "Patient", "name", "Patient.name");
        // family + 2 given = 3
        assert_eq!(strings.len(), 3);
        assert!(strings.iter().any(|s| s.value_exact == "Smith"));
        assert!(strings.iter().any(|s| s.value_exact == "John"));
    }

    #[test]
    fn test_normalize_string() {
        assert_eq!(normalize_string("Smith"), "smith");
        assert_eq!(normalize_string("HELLO"), "hello");
    }

    #[test]
    fn test_fhirpath_to_segments() {
        let segs = fhirpath_to_segments("Observation.subject", "Observation");
        assert_eq!(segs, vec!["subject"]);

        let segs = fhirpath_to_segments("Patient.name.family", "Patient");
        assert_eq!(segs, vec!["name", "family"]);

        let segs = fhirpath_to_segments(
            "Observation.subject.where(resolve() is Patient)",
            "Observation",
        );
        assert_eq!(segs, vec!["subject"]);
    }

    #[test]
    fn test_parse_date_ranges() {
        let range = parse_date_to_range("2024").unwrap();
        assert!(range.0.starts_with("2024-01-01"));
        // Half-open upper = start of next year
        assert!(range.1.starts_with("2025-01-01"));

        let range = parse_date_to_range("2024-02").unwrap();
        assert!(range.0.starts_with("2024-02-01"));
        // Half-open upper = start of March
        assert!(range.1.starts_with("2024-03-01"));

        let range = parse_date_to_range("2023-02").unwrap();
        assert!(range.1.starts_with("2023-03-01"));
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 2), 29); // leap year
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
    }
}
