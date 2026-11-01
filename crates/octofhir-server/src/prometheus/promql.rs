//! Scoping PromQL queries to this deployment's series.
//!
//! Dashboards send arbitrary PromQL, but the Prometheus they read from is often shared with other
//! services. Every query is parsed and the configured selector's matchers (`{job="octofhir"}`) are
//! appended to each vector selector in it before it is sent. Matchers on the same label are ANDed
//! in PromQL, so an appended matcher can only narrow what a selector reads: a query that asks for
//! `{job="other"}` gets `{job="other",job="octofhir"}` and comes back empty.

use std::collections::BTreeMap;

use std::time::Duration;

use promql_parser::label::Matcher;
use promql_parser::parser::{self, Expr, VectorSelector};
use promql_parser::util::duration::display_duration;

/// Longest query accepted, in bytes. Dashboards stay far below it; the cap keeps a pasted blob
/// from reaching the parser.
pub const MAX_QUERY_LEN: usize = 16 * 1024;

/// Parses a selector such as `{job="octofhir", env="prod"}` into its matchers. It must consist of
/// label matchers only: no metric name, `or` groups, offset or `@`.
pub fn parse_selector(selector: &str) -> Result<Vec<Matcher>, String> {
    let expr = parser::parse(selector.trim())?;
    let Expr::VectorSelector(vs) = expr else {
        return Err("must be a series selector such as {job=\"octofhir\"}".into());
    };
    if vs.name.is_some() || vs.offset.is_some() || vs.at.is_some() {
        return Err("must contain label matchers only, without a metric name, offset or @".into());
    }
    if !vs.matchers.or_matchers.is_empty() {
        return Err("must not use `or` between matchers".into());
    }
    if vs.matchers.matchers.is_empty() {
        return Err("must contain at least one label matcher".into());
    }
    Ok(vs.matchers.matchers)
}

/// Parses `query`, appends `scope` to every vector selector and renders the result.
pub fn scope_query(query: &str, scope: &[Matcher]) -> Result<String, String> {
    if query.len() > MAX_QUERY_LEN {
        return Err(format!("query is longer than {MAX_QUERY_LEN} bytes"));
    }
    let mut expr = parser::parse(query)?;
    scope_expr(&mut expr, scope)?;
    Ok(expr.to_string())
}

/// Dashboard variables, written `$name` or `${name}` like Grafana's so a dashboard exports
/// unchanged. Built-ins (`$__rate_interval`, `$__interval`, `$__range`) are durations in seconds;
/// user variables are label values and are escaped for a double-quoted string.
#[derive(Debug, Default, Clone)]
pub struct Variables {
    values: BTreeMap<String, String>,
}

/// One substitution, in 1-based character columns of the original and the expanded query, so
/// error positions can be mapped back to what the user wrote.
#[derive(Debug, Clone, Copy)]
struct Shift {
    line: u32,
    original_start: u32,
    original_len: u32,
    expanded_start: u32,
    expanded_len: u32,
}

/// A query after variable substitution.
#[derive(Debug, Clone)]
pub struct Expanded {
    pub query: String,
    shifts: Vec<Shift>,
}

impl Variables {
    pub fn new(rate_interval_secs: u64, interval_secs: u64, range_secs: u64) -> Self {
        let mut values = BTreeMap::new();
        values.insert("__rate_interval".into(), format!("{rate_interval_secs}s"));
        values.insert("__interval".into(), format!("{interval_secs}s"));
        values.insert("__range".into(), format!("{range_secs}s"));
        Self { values }
    }

    /// Adds user variables; names that are not identifiers or shadow a built-in are ignored.
    pub fn with_user(mut self, user: &BTreeMap<String, String>) -> Self {
        for (name, value) in user {
            if is_identifier(name) && !name.starts_with("__") {
                self.values.insert(name.clone(), escape_string(value));
            }
        }
        self
    }

    pub fn expand(&self, query: &str) -> Expanded {
        let mut out = String::with_capacity(query.len());
        let mut shifts = Vec::new();
        let (mut line, mut original_col, mut expanded_col) = (1u32, 1u32, 1u32);
        let mut rest = query;
        while let Some(ch) = rest.chars().next() {
            if ch == '$'
                && let Some((name, consumed)) = reference(rest)
                && let Some(value) = self.values.get(name)
            {
                let original_len = rest[..consumed].chars().count() as u32;
                let expanded_len = value.chars().count() as u32;
                shifts.push(Shift {
                    line,
                    original_start: original_col,
                    original_len,
                    expanded_start: expanded_col,
                    expanded_len,
                });
                out.push_str(value);
                original_col += original_len;
                expanded_col += expanded_len;
                rest = &rest[consumed..];
                continue;
            }
            out.push(ch);
            if ch == '\n' {
                line += 1;
                original_col = 1;
                expanded_col = 1;
            } else {
                original_col += 1;
                expanded_col += 1;
            }
            rest = &rest[ch.len_utf8()..];
        }
        Expanded { query: out, shifts }
    }
}

impl Expanded {
    /// Maps a 1-based position in the expanded query back to the query as written. A position
    /// inside substituted text points at the variable reference.
    pub fn original_position(&self, line: u32, column: u32) -> (u32, u32) {
        let last = self
            .shifts
            .iter()
            .rev()
            .find(|shift| shift.line == line && shift.expanded_start <= column);
        let Some(shift) = last else {
            return (line, column);
        };
        let expanded_end = shift.expanded_start + shift.expanded_len;
        if column < expanded_end {
            (line, shift.original_start)
        } else {
            (
                line,
                shift.original_start + shift.original_len + (column - expanded_end),
            )
        }
    }
}

/// `$name` or `${name}` at the start of `text`: the name and the bytes it spans.
fn reference(text: &str) -> Option<(&str, usize)> {
    let body = text.strip_prefix('$')?;
    if let Some(braced) = body.strip_prefix('{') {
        let end = braced.find('}')?;
        let name = &braced[..end];
        return is_identifier(name).then_some((name, end + 3));
    }
    let end = body
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(body.len());
    let name = &body[..end];
    is_identifier(name).then_some((name, end + 1))
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Escapes a value for the inside of a double-quoted PromQL string.
fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

/// Built-in duration variables and the placeholder each stands for while a template is parsed.
/// The odd millisecond counts never appear in a real query.
const TEMPLATE_DURATIONS: [(&str, u64); 3] = [
    ("__rate_interval", 7_919_001),
    ("__interval", 7_919_002),
    ("__range", 7_919_003),
];

/// Scopes a query that still contains variables, keeping them for another tool (Grafana) to
/// substitute: duration built-ins become placeholder durations and user variables placeholder
/// names while the query is parsed and scoped, then are put back. User variables are supported
/// inside strings (`instance=~"$instance"`), which is where label-value variables go.
pub fn scope_template(query: &str, scope: &[Matcher]) -> Result<String, String> {
    let mut out = String::with_capacity(query.len());
    let mut users = Vec::new();
    let mut rest = query;
    while let Some(ch) = rest.chars().next() {
        if ch == '$'
            && let Some((name, consumed)) = reference(rest)
        {
            match TEMPLATE_DURATIONS
                .iter()
                .find(|(builtin, _)| *builtin == name)
            {
                Some((_, ms)) => out.push_str(&format!("{ms}ms")),
                None => {
                    out.push_str(&user_placeholder(name));
                    users.push(name.to_string());
                }
            }
            rest = &rest[consumed..];
            continue;
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    let mut scoped = scope_query(&out, scope)?;
    for (name, ms) in TEMPLATE_DURATIONS {
        let shown = display_duration(&Duration::from_millis(ms));
        scoped = scoped.replace(&shown, &format!("${name}"));
    }
    for name in users {
        scoped = scoped.replace(&user_placeholder(&name), &format!("${name}"));
    }
    Ok(scoped)
}

fn user_placeholder(name: &str) -> String {
    format!("__octofhir_var_{name}__")
}

fn scope_expr(expr: &mut Expr, scope: &[Matcher]) -> Result<(), String> {
    match expr {
        Expr::VectorSelector(vs) => scope_selector(vs, scope),
        Expr::MatrixSelector(ms) => scope_selector(&mut ms.vs, scope),
        Expr::Aggregate(agg) => {
            scope_expr(&mut agg.expr, scope)?;
            if let Some(param) = agg.param.as_mut() {
                scope_expr(param, scope)?;
            }
        }
        Expr::Unary(unary) => scope_expr(&mut unary.expr, scope)?,
        Expr::Binary(binary) => {
            scope_expr(&mut binary.lhs, scope)?;
            scope_expr(&mut binary.rhs, scope)?;
        }
        Expr::Paren(paren) => scope_expr(&mut paren.expr, scope)?,
        Expr::Subquery(subquery) => scope_expr(&mut subquery.expr, scope)?,
        Expr::Call(call) => {
            for arg in call.args.args.iter_mut() {
                scope_expr(arg, scope)?;
            }
        }
        Expr::NumberLiteral(_) | Expr::StringLiteral(_) => {}
        // Extensions are opaque to the walk, so their selectors could not be scoped.
        Expr::Extension(_) => return Err("unsupported expression".into()),
    }
    Ok(())
}

fn scope_selector(vs: &mut VectorSelector, scope: &[Matcher]) {
    let matchers = &mut vs.matchers;
    if matchers.or_matchers.is_empty() {
        append_missing(&mut matchers.matchers, scope);
    } else {
        // `{a="1" or b="2"}` reads each group separately, so each one is scoped.
        for group in matchers.or_matchers.iter_mut() {
            append_missing(group, scope);
        }
    }
}

fn append_missing(matchers: &mut Vec<Matcher>, scope: &[Matcher]) {
    for matcher in scope {
        if !matchers.contains(matcher) {
            matchers.push(matcher.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Vec<Matcher> {
        parse_selector(r#"{job="octofhir"}"#).unwrap()
    }

    fn scoped(query: &str) -> String {
        scope_query(query, &scope()).unwrap()
    }

    #[test]
    fn selector_must_be_matchers_only() {
        assert_eq!(scope().len(), 1);
        assert_eq!(
            parse_selector(r#"{job="a", env=~"prod|stage"}"#)
                .unwrap()
                .len(),
            2
        );
        assert!(parse_selector("up").is_err());
        assert!(parse_selector(r#"up{job="a"}"#).is_err());
        assert!(parse_selector(r#"{job="a"} offset 5m"#).is_err());
        assert!(parse_selector(r#"{job="a" or job="b"}"#).is_err());
        assert!(parse_selector("sum(x)").is_err());
        assert!(parse_selector("{").is_err());
    }

    #[test]
    fn scopes_a_plain_metric() {
        assert_eq!(scoped("up"), r#"up{job="octofhir"}"#);
    }

    #[test]
    fn scopes_every_selector_in_nested_expressions() {
        let query = r#"sum by (path) (rate(octofhir_http_requests_total{status_class="5xx"}[5m])) / sum by (path) (rate(octofhir_http_requests_total[5m]))"#;
        let out = scoped(query);
        assert_eq!(out.matches(r#"job="octofhir""#).count(), 2, "{out}");
        assert!(out.contains(r#"status_class="5xx""#), "{out}");
    }

    #[test]
    fn scopes_histogram_quantile_and_subqueries() {
        let out = scoped(
            "histogram_quantile(0.95, sum by (le) (rate(octofhir_http_request_duration_seconds_bucket[5m])))",
        );
        assert!(
            out.contains(r#"octofhir_http_request_duration_seconds_bucket{job="octofhir"}[5m]"#),
            "{out}"
        );
        let out = scoped("max_over_time(rate(x[1m])[10m:1m])");
        assert!(out.contains(r#"x{job="octofhir"}[1m]"#), "{out}");
    }

    #[test]
    fn scopes_aggregation_params_and_offsets() {
        let out = scoped("topk(scalar(count(y)), x) - x offset 1d");
        assert_eq!(out.matches(r#"job="octofhir""#).count(), 3, "{out}");
        assert!(out.contains("offset 1d"), "{out}");
    }

    #[test]
    fn a_conflicting_job_matcher_only_narrows() {
        let out = scoped(r#"up{job="other"}"#);
        assert!(
            out.contains(r#"job="other""#) && out.contains(r#"job="octofhir""#),
            "{out}"
        );
    }

    #[test]
    fn existing_identical_matcher_is_not_duplicated() {
        assert_eq!(scoped(r#"up{job="octofhir"}"#), r#"up{job="octofhir"}"#);
    }

    #[test]
    fn scopes_each_or_group() {
        let out = scoped(r#"{__name__="a" or __name__="b"}"#);
        assert_eq!(out.matches(r#"job="octofhir""#).count(), 2, "{out}");
    }

    #[test]
    fn leaves_literals_alone() {
        assert_eq!(scoped("1 + 2"), "1 + 2");
    }

    #[test]
    fn rejects_invalid_and_oversized_queries() {
        assert!(scope_query("sum(", &scope()).is_err());
        assert!(scope_query(&"a".repeat(MAX_QUERY_LEN + 1), &scope()).is_err());
    }

    #[test]
    fn expands_builtin_and_user_variables() {
        let mut user = BTreeMap::new();
        user.insert("instance".to_string(), "pod-a:8888".to_string());
        user.insert("__range".to_string(), "evil".to_string());
        let vars = Variables::new(60, 15, 3600).with_user(&user);
        let out =
            vars.expand(r#"rate(x{instance=~"$instance"}[$__rate_interval]) + y[${__range}]"#);
        assert_eq!(
            out.query,
            r#"rate(x{instance=~"pod-a:8888"}[60s]) + y[3600s]"#
        );
        // Unknown variables stay as written (and then fail to parse if outside a string).
        assert_eq!(vars.expand("$nope").query, "$nope");
    }

    #[test]
    fn user_values_cannot_break_out_of_strings() {
        let mut user = BTreeMap::new();
        user.insert(
            "instance".to_string(),
            r#"a"} or vector(1) or x{b=""#.to_string(),
        );
        let out = Variables::new(60, 15, 60)
            .with_user(&user)
            .expand(r#"up{instance="$instance"}"#);
        let scoped = scope_query(&out.query, &scope()).unwrap();
        // The whole value stays one string literal of the instance matcher.
        assert_eq!(
            scoped,
            r#"up{instance="a\"} or vector(1) or x{b=\"",job="octofhir"}"#
        );
    }

    #[test]
    fn maps_error_columns_back_through_substitutions() {
        let vars = Variables::new(60, 15, 60);
        let out = vars.expand("rate(x[$__rate_interval]) +");
        // `rate(x[60s]) +` — the error at the end (col 15) is col 28 as written.
        assert_eq!(out.original_position(1, 15), (1, 28));
        assert_eq!(out.original_position(1, 3), (1, 3));
        // Inside `60s` points at `$__rate_interval`.
        assert_eq!(out.original_position(1, 9), (1, 8));
    }

    #[test]
    fn templates_keep_their_variables() {
        let out = scope_template(
            r#"sum by (status_class) (rate(octofhir_http_requests_total{instance=~"$instance"}[$__rate_interval]))"#,
            &scope(),
        )
        .unwrap();
        assert_eq!(
            out,
            r#"sum by (status_class) (rate(octofhir_http_requests_total{instance=~"$instance",job="octofhir"}[$__rate_interval]))"#
        );
        let out = scope_template("increase(x[${__range}])", &scope()).unwrap();
        assert_eq!(out, r#"increase(x{job="octofhir"}[$__range])"#);
    }

    #[test]
    fn scoped_queries_parse_again() {
        for query in [
            "rate(x[5m])",
            r#"sum without (instance) (x{a=~"b.*"})"#,
            "x @ 1700000000",
            "x > bool 1",
            "a * on (instance) group_left (version) b",
            r#"label_replace(up, "dst", "$1", "src", "(.*)")"#,
        ] {
            let out = scoped(query);
            parser::parse(&out).unwrap_or_else(|e| panic!("{query} -> {out}: {e}"));
        }
    }
}
