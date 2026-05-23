//! Date search predicate AST.
//!
//! Explicit data representation of date search clauses between parsed FHIR
//! parameters and emitted SQL. Enables cross-occurrence rewrites
//! (e.g. collapsing `?date=ge…&date=le…` to one overlap predicate) as
//! pure passes over `Vec<DateClause>` instead of ad-hoc dispatch helpers.
//! Prefix algebra lives in one place where the truth table can prove it.

use crate::parameters::SearchPrefix;
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::types::date::{DateRange, parse_date_range};
use time::OffsetDateTime;

/// One end of a half-open range with an inclusivity flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bound {
    pub at: OffsetDateTime,
    pub inclusive: bool,
}

impl Bound {
    fn inclusive(at: OffsetDateTime) -> Self {
        Self {
            at,
            inclusive: true,
        }
    }
    fn exclusive(at: OffsetDateTime) -> Self {
        Self {
            at,
            inclusive: false,
        }
    }
}

/// A single FHIR date prefix predicate.
/// Boundary inclusivity follows FHIR R4 §3.1.1.5.1 with the half-open
/// canonical form `[lower, upper)`.
#[derive(Debug, Clone)]
pub enum DatePredicate {
    /// `r.rng <@ tstzrange(q.lo, q.hi, '[)')` — eq.
    Contains { q: DateRange },
    /// `NOT EXISTS (… r.rng <@ q …)` — ne.
    NotContains { q: DateRange },
    /// `r.rng && tstzrange(lo, hi, …)` — gt, lt, ge, le, ap, and any window
    /// produced by the fold rewrite. `lo`/`hi` of `None` mean an infinite
    /// half-line in that direction.
    Overlap {
        lo: Option<Bound>,
        hi: Option<Bound>,
    },
    /// `r.rng >> tstzrange(q.lo, q.hi, '[)')` — sa.
    StrictlyAfter { q: DateRange },
    /// `r.rng << tstzrange(q.lo, q.hi, '[)')` — eb.
    StrictlyBefore { q: DateRange },
}

/// Date predicate plus its `search_idx_date` lookup key.
#[derive(Debug, Clone)]
pub struct DateClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: DatePredicate,
}

impl DateClause {
    /// Build clauses from one `ParsedParam`. One `ParsedParam` may carry
    /// multiple comma-OR'd values, each producing one clause.
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
    ) -> Result<Vec<DateClause>, SqlBuilderError> {
        let mut out = Vec::with_capacity(param.values.len());
        for v in &param.values {
            if v.raw.is_empty() {
                continue;
            }
            let prefix = v.prefix.unwrap_or(SearchPrefix::Eq);
            let q = parse_date_range(&v.raw)?;
            let predicate = match prefix {
                SearchPrefix::Eq => DatePredicate::Contains { q },
                SearchPrefix::Ne => DatePredicate::NotContains { q },
                // gt: r intersects [upper(q), +∞)
                SearchPrefix::Gt => DatePredicate::Overlap {
                    lo: Some(Bound::inclusive(q.end)),
                    hi: None,
                },
                // lt: r intersects (-∞, lower(q))
                SearchPrefix::Lt => DatePredicate::Overlap {
                    lo: None,
                    hi: Some(Bound::exclusive(q.start)),
                },
                // ge: r intersects [lower(q), +∞)
                SearchPrefix::Ge => DatePredicate::Overlap {
                    lo: Some(Bound::inclusive(q.start)),
                    hi: None,
                },
                // le: r intersects (-∞, upper(q))
                SearchPrefix::Le => DatePredicate::Overlap {
                    lo: None,
                    hi: Some(Bound::exclusive(q.end)),
                },
                SearchPrefix::Sa => DatePredicate::StrictlyAfter { q },
                SearchPrefix::Eb => DatePredicate::StrictlyBefore { q },
                // ap: ±10% expansion around q (matches date.rs::build_index_date_search).
                SearchPrefix::Ap => {
                    let duration = q.end - q.start;
                    let expansion = duration / 10;
                    DatePredicate::Overlap {
                        lo: Some(Bound::inclusive(q.start - expansion)),
                        hi: Some(Bound::exclusive(q.end + expansion)),
                    }
                }
            };
            out.push(DateClause {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate,
            });
        }
        Ok(out)
    }

    /// Render this clause to a SQL fragment, registering bound parameters
    /// on the supplied `SqlBuilder`. The returned string is meant to be
    /// inserted into the surrounding `WHERE` directly.
    pub fn render(&self, builder: &mut SqlBuilder) -> String {
        let rt_param = builder.add_text_param(&self.resource_type);
        let pc_param = builder.add_text_param(&self.param_code);
        let id_col = builder.id_column();
        match &self.predicate {
            DatePredicate::Contains { q } => {
                let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
                let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_date sid \
                     WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                     AND sid.param_code = ${pc_param} \
                     AND sid.rng <@ tstzrange(${p_lo}::timestamptz, ${p_hi}::timestamptz, '[)'))"
                )
            }
            DatePredicate::NotContains { q } => {
                let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
                let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
                format!(
                    "NOT EXISTS (SELECT 1 FROM search_idx_date sid \
                     WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                     AND sid.param_code = ${pc_param} \
                     AND sid.rng <@ tstzrange(${p_lo}::timestamptz, ${p_hi}::timestamptz, '[)'))"
                )
            }
            DatePredicate::Overlap { lo, hi } => {
                let (lo_expr, lo_inc) = match lo {
                    Some(b) => {
                        let p = builder.add_timestamp_param(format_rfc3339(&b.at));
                        (format!("${p}::timestamptz"), b.inclusive)
                    }
                    None => ("NULL".to_string(), true),
                };
                let (hi_expr, hi_inc) = match hi {
                    Some(b) => {
                        let p = builder.add_timestamp_param(format_rfc3339(&b.at));
                        (format!("${p}::timestamptz"), b.inclusive)
                    }
                    None => ("NULL".to_string(), false),
                };
                let bounds = bounds_token(lo_inc, hi_inc);
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_date sid \
                     WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                     AND sid.param_code = ${pc_param} \
                     AND sid.rng && tstzrange({lo_expr}, {hi_expr}, '{bounds}'))"
                )
            }
            DatePredicate::StrictlyAfter { q } => {
                let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
                let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_date sid \
                     WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                     AND sid.param_code = ${pc_param} \
                     AND sid.rng >> tstzrange(${p_lo}::timestamptz, ${p_hi}::timestamptz, '[)'))"
                )
            }
            DatePredicate::StrictlyBefore { q } => {
                let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
                let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_date sid \
                     WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                     AND sid.param_code = ${pc_param} \
                     AND sid.rng << tstzrange(${p_lo}::timestamptz, ${p_hi}::timestamptz, '[)'))"
                )
            }
        }
    }
}

/// Merge `Overlap` clauses on the same `(resource_type, param_code)` into
/// one half-open window. Other predicate kinds pass through unchanged.
///
/// Intersection rules:
/// - Lower bound: largest `at`; tie keeps inclusive only if both sides are.
/// - Upper bound: smallest `at`; same tie rule.
/// - `None` on a bound = infinite half-line, contributes nothing.
///
/// Degenerate windows (`lo >= hi`) pass through — PG returns zero rows
/// from an empty `tstzrange`, which is the correct FHIR result.
pub fn merge_overlap_windows(clauses: Vec<DateClause>) -> Vec<DateClause> {
    use std::collections::BTreeMap;

    // Bucket by (resource_type, param_code).
    let mut buckets: BTreeMap<(String, String), (Option<Bound>, Option<Bound>, Vec<DateClause>)> =
        BTreeMap::new();
    // Stable ordering of buckets in the output follows first-occurrence.
    let mut order: Vec<(String, String)> = Vec::new();

    for clause in clauses {
        let key = (clause.resource_type.clone(), clause.param_code.clone());
        if !buckets.contains_key(&key) {
            order.push(key.clone());
        }
        let entry = buckets
            .entry(key)
            .or_insert_with(|| (None, None, Vec::new()));
        match &clause.predicate {
            DatePredicate::Overlap { lo, hi } => {
                entry.0 = tighter_lo(entry.0, *lo);
                entry.1 = tighter_hi(entry.1, *hi);
            }
            _ => entry.2.push(clause),
        }
    }

    let mut out = Vec::new();
    for key in order {
        let (lo, hi, mut passthrough) = buckets.remove(&key).expect("bucket present");
        let (resource_type, param_code) = key;
        // Only emit a merged Overlap if at least one side was contributed.
        if lo.is_some() || hi.is_some() {
            out.push(DateClause {
                resource_type: resource_type.clone(),
                param_code: param_code.clone(),
                predicate: DatePredicate::Overlap { lo, hi },
            });
        }
        out.append(&mut passthrough);
    }
    out
}

fn tighter_lo(cur: Option<Bound>, cand: Option<Bound>) -> Option<Bound> {
    match (cur, cand) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) if b.at > a.at => Some(b),
        (Some(a), Some(b)) if b.at < a.at => Some(a),
        (Some(a), Some(b)) => Some(Bound {
            at: a.at,
            inclusive: a.inclusive && b.inclusive,
        }),
    }
}

fn tighter_hi(cur: Option<Bound>, cand: Option<Bound>) -> Option<Bound> {
    match (cur, cand) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) if b.at < a.at => Some(b),
        (Some(a), Some(b)) if b.at > a.at => Some(a),
        (Some(a), Some(b)) => Some(Bound {
            at: a.at,
            inclusive: a.inclusive && b.inclusive,
        }),
    }
}

fn bounds_token(lo_inc: bool, hi_inc: bool) -> &'static str {
    match (lo_inc, hi_inc) {
        (true, true) => "[]",
        (true, false) => "[)",
        (false, true) => "(]",
        (false, false) => "()",
    }
}

fn format_rfc3339(dt: &OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedValue;
    use time::{Date, Month, OffsetDateTime, Time};

    fn pp(name: &str, prefix: SearchPrefix, raw: &str) -> ParsedParam {
        ParsedParam {
            name: name.to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: Some(prefix),
                raw: raw.to_string(),
            }],
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestEndpoint {
        NegInf,
        Finite(OffsetDateTime),
        PosInf,
    }

    #[derive(Debug, Clone, Copy)]
    struct TestRange {
        lo: TestEndpoint,
        hi: TestEndpoint,
    }

    fn dt(year: i32, month: Month, day: u8) -> OffsetDateTime {
        Date::from_calendar_date(year, month, day)
            .unwrap()
            .with_time(Time::MIDNIGHT)
            .assume_utc()
    }

    fn range(lo: OffsetDateTime, hi: OffsetDateTime) -> TestRange {
        TestRange {
            lo: TestEndpoint::Finite(lo),
            hi: TestEndpoint::Finite(hi),
        }
    }

    fn start_only(lo: OffsetDateTime) -> TestRange {
        TestRange {
            lo: TestEndpoint::Finite(lo),
            hi: TestEndpoint::PosInf,
        }
    }

    fn end_only(hi: OffsetDateTime) -> TestRange {
        TestRange {
            lo: TestEndpoint::NegInf,
            hi: TestEndpoint::Finite(hi),
        }
    }

    fn endpoint_cmp(a: TestEndpoint, b: TestEndpoint) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (a, b) {
            (TestEndpoint::NegInf, TestEndpoint::NegInf)
            | (TestEndpoint::PosInf, TestEndpoint::PosInf) => Ordering::Equal,
            (TestEndpoint::NegInf, _) | (_, TestEndpoint::PosInf) => Ordering::Less,
            (TestEndpoint::PosInf, _) | (_, TestEndpoint::NegInf) => Ordering::Greater,
            (TestEndpoint::Finite(a), TestEndpoint::Finite(b)) => a.cmp(&b),
        }
    }

    fn le(a: TestEndpoint, b: TestEndpoint) -> bool {
        !matches!(endpoint_cmp(a, b), std::cmp::Ordering::Greater)
    }

    fn lt(a: TestEndpoint, b: TestEndpoint) -> bool {
        matches!(endpoint_cmp(a, b), std::cmp::Ordering::Less)
    }

    fn contains(q: &DateRange, target: TestRange) -> bool {
        let q_lo = TestEndpoint::Finite(q.start);
        let q_hi = TestEndpoint::Finite(q.end);
        le(q_lo, target.lo) && le(target.hi, q_hi)
    }

    fn overlap(lo: Option<Bound>, hi: Option<Bound>, target: TestRange) -> bool {
        let win_lo = lo
            .map(|b| TestEndpoint::Finite(b.at))
            .unwrap_or(TestEndpoint::NegInf);
        let win_hi = hi
            .map(|b| TestEndpoint::Finite(b.at))
            .unwrap_or(TestEndpoint::PosInf);
        lt(target.lo, win_hi) && lt(win_lo, target.hi)
    }

    fn matches_predicate(predicate: &DatePredicate, target: TestRange) -> bool {
        match predicate {
            DatePredicate::Contains { q } => contains(q, target),
            DatePredicate::NotContains { q } => !contains(q, target),
            DatePredicate::Overlap { lo, hi } => overlap(*lo, *hi, target),
            DatePredicate::StrictlyAfter { q } => le(TestEndpoint::Finite(q.end), target.lo),
            DatePredicate::StrictlyBefore { q } => le(target.hi, TestEndpoint::Finite(q.start)),
        }
    }

    fn predicate(prefix: SearchPrefix, raw: &str) -> DatePredicate {
        DateClause::from_parsed_param(&pp("date", prefix, raw), "Patient")
            .unwrap()
            .remove(0)
            .predicate
    }

    #[test]
    fn from_parsed_param_maps_each_prefix() {
        for (prefix, expect_overlap, expect_inclusive_lo) in [
            (SearchPrefix::Ge, true, Some(true)),
            (SearchPrefix::Gt, true, Some(true)),
            (SearchPrefix::Le, true, None),
            (SearchPrefix::Lt, true, None),
            (SearchPrefix::Ap, true, Some(true)),
            (SearchPrefix::Eq, false, None),
        ] {
            let p = pp("date", prefix, "2024-06-15");
            let clauses = DateClause::from_parsed_param(&p, "Patient").unwrap();
            assert_eq!(clauses.len(), 1);
            match &clauses[0].predicate {
                DatePredicate::Overlap { lo, .. } => {
                    assert!(expect_overlap, "expected non-overlap for {prefix:?}");
                    if let Some(inc) = expect_inclusive_lo {
                        assert_eq!(
                            lo.map(|b| b.inclusive),
                            Some(inc),
                            "lo inclusivity mismatch for {prefix:?}"
                        );
                    }
                }
                DatePredicate::Contains { .. } => assert!(!expect_overlap),
                _ => {}
            }
        }
    }

    #[test]
    fn date_prefix_truth_table_matches_fhir_r4_range_semantics() {
        let before = range(dt(2024, Month::June, 14), dt(2024, Month::June, 15));
        let exact = range(dt(2024, Month::June, 15), dt(2024, Month::June, 16));
        let after = range(dt(2024, Month::June, 16), dt(2024, Month::June, 17));
        let wide = range(dt(2024, Month::June, 14), dt(2024, Month::June, 17));

        let cases = [
            (SearchPrefix::Eq, [false, true, false, false]),
            (SearchPrefix::Ne, [true, false, true, true]),
            (SearchPrefix::Gt, [false, false, true, true]),
            (SearchPrefix::Lt, [true, false, false, true]),
            (SearchPrefix::Ge, [false, true, true, true]),
            (SearchPrefix::Le, [true, true, false, true]),
            (SearchPrefix::Sa, [false, false, true, false]),
            (SearchPrefix::Eb, [true, false, false, false]),
            (SearchPrefix::Ap, [true, true, true, true]),
        ];

        for (prefix, expected) in cases {
            let predicate = predicate(prefix, "2024-06-15");
            let actual =
                [before, exact, after, wide].map(|target| matches_predicate(&predicate, target));
            assert_eq!(actual, expected, "prefix {prefix:?}");
        }
    }

    #[test]
    fn open_period_start_only_and_end_only_follow_fhir_missing_bound_semantics() {
        let start_only_period = start_only(dt(2024, Month::June, 15));
        let end_only_period = end_only(dt(2024, Month::June, 15));

        let eq = predicate(SearchPrefix::Eq, "2024-06-15");
        assert!(!matches_predicate(&eq, start_only_period));
        assert!(!matches_predicate(&eq, end_only_period));

        let ge = predicate(SearchPrefix::Ge, "2024-06-15");
        assert!(matches_predicate(&ge, start_only_period));

        let le = predicate(SearchPrefix::Le, "2024-06-15");
        assert!(matches_predicate(&le, end_only_period));

        let sa = predicate(SearchPrefix::Sa, "2024-06-15");
        assert!(!matches_predicate(&sa, start_only_period));

        let eb = predicate(SearchPrefix::Eb, "2024-06-15");
        assert!(matches_predicate(&eb, end_only_period));
    }

    #[test]
    fn half_open_boundaries_do_not_match_adjacent_eq_ranges() {
        let exact = predicate(SearchPrefix::Eq, "2024-06-15");
        let previous_day = range(dt(2024, Month::June, 14), dt(2024, Month::June, 15));
        let next_day = range(dt(2024, Month::June, 16), dt(2024, Month::June, 17));

        assert!(!matches_predicate(&exact, previous_day));
        assert!(!matches_predicate(&exact, next_day));
    }

    #[test]
    fn merge_collapses_ge_le_into_one_window() {
        let ge = DateClause::from_parsed_param(&pp("date", SearchPrefix::Ge, "2024-01-01"), "Pt")
            .unwrap();
        let le = DateClause::from_parsed_param(&pp("date", SearchPrefix::Le, "2024-12-31"), "Pt")
            .unwrap();
        let mut all = ge;
        all.extend(le);

        let merged = merge_overlap_windows(all);
        assert_eq!(merged.len(), 1, "ge+le must collapse into one clause");
        let DatePredicate::Overlap { lo, hi } = &merged[0].predicate else {
            panic!("expected Overlap, got {:?}", merged[0].predicate);
        };
        assert!(lo.unwrap().inclusive, "ge → inclusive lower");
        assert!(!hi.unwrap().inclusive, "le → exclusive upper");
        // Lower must be 2024-01-01, upper must be 2025-01-01 (le2024-12-31 expanded).
        let lo_str = format_rfc3339(&lo.unwrap().at);
        let hi_str = format_rfc3339(&hi.unwrap().at);
        assert!(
            lo_str.starts_with("2024-01-01"),
            "lo should be 2024-01-01, got {lo_str}"
        );
        assert!(
            hi_str.starts_with("2025-01-01"),
            "hi should be 2025-01-01, got {hi_str}"
        );
    }

    #[test]
    fn merge_keeps_eq_clause_separate() {
        let eq = DateClause::from_parsed_param(&pp("date", SearchPrefix::Eq, "2024-06-15"), "Pt")
            .unwrap();
        let ge = DateClause::from_parsed_param(&pp("date", SearchPrefix::Ge, "2020-01-01"), "Pt")
            .unwrap();
        let mut all = eq;
        all.extend(ge);
        let merged = merge_overlap_windows(all);
        // One merged Overlap (from the lone ge) + the untouched eq Contains.
        assert_eq!(merged.len(), 2, "got: {merged:?}");
        let kinds: Vec<&str> = merged
            .iter()
            .map(|c| match &c.predicate {
                DatePredicate::Overlap { .. } => "overlap",
                DatePredicate::Contains { .. } => "contains",
                _ => "other",
            })
            .collect();
        assert!(kinds.contains(&"overlap"));
        assert!(kinds.contains(&"contains"));
    }

    #[test]
    fn merge_takes_strictest_lo_when_two_ge_present() {
        let mut all = Vec::new();
        all.extend(
            DateClause::from_parsed_param(&pp("date", SearchPrefix::Ge, "1980-01-01"), "Pt")
                .unwrap(),
        );
        all.extend(
            DateClause::from_parsed_param(&pp("date", SearchPrefix::Ge, "1990-06-15"), "Pt")
                .unwrap(),
        );
        all.extend(
            DateClause::from_parsed_param(&pp("date", SearchPrefix::Le, "2010-01-01"), "Pt")
                .unwrap(),
        );
        let merged = merge_overlap_windows(all);
        assert_eq!(merged.len(), 1);
        let DatePredicate::Overlap { lo, .. } = &merged[0].predicate else {
            panic!()
        };
        let lo_str = format_rfc3339(&lo.unwrap().at);
        assert!(
            lo_str.starts_with("1990-06-15"),
            "strictest ge wins, got {lo_str}"
        );
    }

    #[test]
    fn render_overlap_emits_half_infinite_when_one_side_missing() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses =
            DateClause::from_parsed_param(&pp("date", SearchPrefix::Ge, "2024-01-01"), "Pt")
                .unwrap();
        let sql = clauses[0].render(&mut builder);
        assert!(
            sql.contains("tstzrange(") && sql.contains(", NULL,") && sql.contains("'[)'"),
            "ge → half-infinite [lo, NULL, '[)'): {sql}"
        );
    }

    #[test]
    fn render_contains_uses_lt_at_op() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses =
            DateClause::from_parsed_param(&pp("date", SearchPrefix::Eq, "2024-06-15"), "Pt")
                .unwrap();
        let sql = clauses[0].render(&mut builder);
        assert!(sql.contains("sid.rng <@"), "eq → `<@`: {sql}");
        assert!(!sql.contains("sid.rng &&"), "eq must not use overlap");
    }

    #[test]
    fn render_not_contains_for_ne() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses =
            DateClause::from_parsed_param(&pp("date", SearchPrefix::Ne, "2024-06-15"), "Pt")
                .unwrap();
        let sql = clauses[0].render(&mut builder);
        assert!(
            sql.starts_with("NOT EXISTS") && sql.contains("sid.rng <@"),
            "ne → NOT EXISTS … <@: {sql}"
        );
    }

    #[test]
    fn ir_render_date_clauses_as_or_preserves_comma_or_shape() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let param = ParsedParam {
            name: "date".to_string(),
            modifier: None,
            values: vec![
                ParsedValue {
                    prefix: Some(SearchPrefix::Eq),
                    raw: "2024-06-15".to_string(),
                },
                ParsedValue {
                    prefix: Some(SearchPrefix::Eq),
                    raw: "2024-06-16".to_string(),
                },
            ],
        };
        let clauses = DateClause::from_parsed_param(&param, "Patient").unwrap();
        let sql = crate::ir::render_date_clauses_as_or(&mut builder, &clauses).unwrap();

        assert!(sql.contains(" OR "), "comma values must render OR: {sql}");
        assert_eq!(sql.matches("EXISTS").count(), 2, "got: {sql}");
    }
}
