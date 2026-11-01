//! The check an SQLQuery or SQLView passes before its SQL runs on the server's
//! database.
//!
//! The SQL comes from the caller (inline, or a Library anyone with write
//! access stored), so it is confined twice:
//!
//! 1. Here, statically, on the PostgreSQL syntax tree (banshee): one `SELECT`,
//!    reading only the relations the Library declares (`relatedArtifact.label`)
//!    and its own CTEs, calling only functions from [`ALLOWED_FUNCTIONS`]
//!    (pure computation: no `set_config`, no `pg_*`, no dynamic SQL), unqualified.
//!    Anything the tree does not show plainly is rejected.
//! 2. At run time, by the database: a read-only transaction under a role with
//!    no table privileges, so the statement can read nothing but the job's
//!    temporary dependency tables even if a construct slipped past the check.
//!
//! The check is fail-closed: SQL that does not parse, or a construct it does
//! not recognize, is rejected rather than run.
//!
//! Parameters are written `:name` (SQL on FHIR, SQLQuery "Parameters") and
//! become typed bound parameters `$n::type`; values are never interpolated.

use std::collections::{HashMap, HashSet};

use banshee_syntax::{NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken};

/// Functions a query may call: aggregates, window functions and pure scalar
/// functions over numbers, text, dates, JSON and arrays. Anything that reads
/// or changes server state, runs SQL text or reaches outside the database is
/// absent (`set_config`, `current_setting`, `pg_*`, `query_to_xml`, `lo_*`,
/// `dblink*`, `nextval`, ...).
const ALLOWED_FUNCTIONS: &[&str] = &[
    // aggregates
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "array_agg",
    "string_agg",
    "bool_and",
    "bool_or",
    "every",
    "json_agg",
    "jsonb_agg",
    "json_object_agg",
    "jsonb_object_agg",
    "stddev",
    "stddev_pop",
    "stddev_samp",
    "variance",
    "var_pop",
    "var_samp",
    "corr",
    "covar_pop",
    "covar_samp",
    "regr_slope",
    "regr_intercept",
    "regr_count",
    "regr_r2",
    "percentile_cont",
    "percentile_disc",
    "mode",
    "bit_and",
    "bit_or",
    "any_value",
    // window functions
    "row_number",
    "rank",
    "dense_rank",
    "percent_rank",
    "cume_dist",
    "ntile",
    "lag",
    "lead",
    "first_value",
    "last_value",
    "nth_value",
    // conditional and comparison
    "coalesce",
    "nullif",
    "greatest",
    "least",
    "num_nulls",
    "num_nonnulls",
    // numbers
    "abs",
    "ceil",
    "ceiling",
    "floor",
    "round",
    "trunc",
    "mod",
    "power",
    "pow",
    "sqrt",
    "cbrt",
    "exp",
    "ln",
    "log",
    "log10",
    "sign",
    "div",
    "width_bucket",
    "scale",
    "gcd",
    "lcm",
    "trim_scale",
    "to_number",
    // text
    "lower",
    "upper",
    "length",
    "char_length",
    "character_length",
    "octet_length",
    "bit_length",
    "substring",
    "substr",
    "position",
    "strpos",
    "trim",
    "btrim",
    "ltrim",
    "rtrim",
    "replace",
    "split_part",
    "concat",
    "concat_ws",
    "left",
    "right",
    "lpad",
    "rpad",
    "initcap",
    "reverse",
    "repeat",
    "starts_with",
    "translate",
    "overlay",
    "format",
    "regexp_replace",
    "regexp_match",
    "regexp_matches",
    "regexp_like",
    "regexp_count",
    "regexp_substr",
    "regexp_split_to_array",
    "regexp_split_to_table",
    "string_to_array",
    "string_to_table",
    "array_to_string",
    "to_char",
    "md5",
    "encode",
    "decode",
    "chr",
    "ascii",
    "quote_literal",
    "quote_nullable",
    "normalize",
    "unistr",
    // dates and times
    "now",
    "date_trunc",
    "date_part",
    "extract",
    "age",
    "make_date",
    "make_time",
    "make_timestamp",
    "make_timestamptz",
    "make_interval",
    "to_date",
    "to_timestamp",
    "date_bin",
    "justify_days",
    "justify_hours",
    "justify_interval",
    "isfinite",
    "timezone",
    "date_add",
    "date_subtract",
    // JSON
    "to_json",
    "to_jsonb",
    "json_build_object",
    "jsonb_build_object",
    "json_build_array",
    "jsonb_build_array",
    "json_object",
    "jsonb_object",
    "json_array_elements",
    "jsonb_array_elements",
    "json_array_elements_text",
    "jsonb_array_elements_text",
    "json_each",
    "jsonb_each",
    "json_each_text",
    "jsonb_each_text",
    "json_extract_path",
    "jsonb_extract_path",
    "json_extract_path_text",
    "jsonb_extract_path_text",
    "json_array_length",
    "jsonb_array_length",
    "json_typeof",
    "jsonb_typeof",
    "json_object_keys",
    "jsonb_object_keys",
    "jsonb_strip_nulls",
    "json_strip_nulls",
    "jsonb_path_query",
    "jsonb_path_query_array",
    "jsonb_path_query_first",
    "jsonb_path_exists",
    "jsonb_path_match",
    "jsonb_set",
    "jsonb_insert",
    "jsonb_pretty",
    "jsonb_concat",
    "row_to_json",
    // arrays and sets
    "array_length",
    "array_lower",
    "array_upper",
    "array_ndims",
    "array_dims",
    "cardinality",
    "array_position",
    "array_positions",
    "array_append",
    "array_prepend",
    "array_cat",
    "array_remove",
    "array_replace",
    "array_fill",
    "unnest",
    "generate_series",
    "generate_subscripts",
    // identifiers
    "gen_random_uuid",
];

/// SQL ready to run: placeholders numbered, every name checked.
#[derive(Debug)]
pub(crate) struct CheckedSql {
    /// The statement, with `:name` parameters as `$n::type`.
    pub sql: String,
    /// The parameter bound to `$n` is `parameters[n - 1]`.
    pub parameters: Vec<String>,
}

/// Check `sql` and number its parameters.
///
/// `relations` are the table names the statement may read besides its own
/// CTEs (the Library's dependency labels); `declared` maps each declared
/// parameter name to the PostgreSQL type its placeholder is cast to.
pub(crate) fn check(
    sql: &str,
    relations: &HashSet<String>,
    declared: &HashMap<String, &'static str>,
) -> Result<CheckedSql, String> {
    let (sql, parameters) = number_parameters(sql, declared)?;
    let parse = banshee_parser::parse(&sql);
    if let Some(error) = parse.errors().first() {
        return Err(format!("The SQL does not parse: {}", error.message));
    }
    let root = parse.syntax();
    let statements: Vec<SyntaxNode> = root.children().cloned().collect();
    match statements.as_slice() {
        [statement] if statement.kind() == SyntaxKind::SELECT_STMT => {}
        [_] => return Err("The SQL must be a single SELECT statement".into()),
        [] => return Err("The SQL has no statement".into()),
        _ => return Err("The SQL must be a single statement".into()),
    }

    let ctes: HashSet<String> = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::CTE)
        .filter_map(|cte| significant_tokens(cte).next())
        .filter_map(|token| identifier(&token))
        .collect();
    let allowed: HashSet<&str> = ALLOWED_FUNCTIONS.iter().copied().collect();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::FUNC_CALL => check_call(node, &allowed)?,
            SyntaxKind::TABLE_REF => check_table_ref(node, relations, &ctes, &allowed)?,
            _ => {}
        }
    }
    check_calls_are_recognized(&root)?;
    Ok(CheckedSql { sql, parameters })
}

/// Replace each `:name` of a declared parameter by `$n::type` (one number per
/// name, in order of first use), and reject what cannot be a single
/// statement with only these parameters: `$n` written by the author, or a
/// `;` other than a trailing one.
fn number_parameters(
    sql: &str,
    declared: &HashMap<String, &'static str>,
) -> Result<(String, Vec<String>), String> {
    let source = sql.trim_end().trim_end_matches(';');
    let tokens = banshee_lexer::tokenize(source);
    let mut spans = Vec::with_capacity(tokens.len());
    let mut offset = 0_usize;
    for token in &tokens {
        let len = usize::from(token.len);
        spans.push((token.kind, offset, offset + len));
        offset += len;
    }

    let mut out = String::with_capacity(source.len());
    let mut parameters: Vec<String> = Vec::new();
    let mut index = 0;
    while index < spans.len() {
        let (kind, start, end) = spans[index];
        match kind {
            SyntaxKind::PARAM => {
                return Err(format!(
                    "Positional parameter {} is not allowed; declare a Library parameter and write :name",
                    &source[start..end]
                ));
            }
            SyntaxKind::SEMICOLON => {
                return Err("The SQL must be a single statement".into());
            }
            SyntaxKind::COLON => {
                if let Some(&(SyntaxKind::IDENT, name_start, name_end)) = spans.get(index + 1) {
                    let name = &source[name_start..name_end];
                    if let Some(sql_type) = declared.get(name) {
                        let number = match parameters.iter().position(|known| known == name) {
                            Some(position) => position + 1,
                            None => {
                                parameters.push(name.to_owned());
                                parameters.len()
                            }
                        };
                        out.push_str(&format!("${number}::{sql_type}"));
                        index += 2;
                        continue;
                    }
                }
                out.push_str(&source[start..end]);
            }
            _ => out.push_str(&source[start..end]),
        }
        index += 1;
    }
    Ok((out, parameters))
}

/// A function call: an unqualified name from the allowlist.
fn check_call(node: &SyntaxNode, allowed: &HashSet<&str>) -> Result<(), String> {
    // `f(x) FILTER (...)` / `f(x) OVER (...)` wrap the call in another
    // FUNC_CALL; the inner one carries the name.
    if !direct_tokens(node).any(|token| token.kind() == SyntaxKind::L_PAREN) {
        return Ok(());
    }
    let name = callee(node)?;
    if allowed.contains(name.as_str()) {
        Ok(())
    } else {
        Err(format!(
            "Function {name}() is not allowed in SQL on FHIR queries"
        ))
    }
}

/// A FROM item: a derived table, an allowed table function, or a relation
/// the job provides (a dependency or one of the statement's CTEs).
fn check_table_ref(
    node: &SyntaxNode,
    relations: &HashSet<String>,
    ctes: &HashSet<String>,
    allowed: &HashSet<&str>,
) -> Result<(), String> {
    let mut tokens = significant_tokens(node).peekable();
    while tokens
        .peek()
        .is_some_and(|token| matches!(token.kind(), SyntaxKind::LATERAL_KW | SyntaxKind::ONLY_KW))
    {
        tokens.next();
    }
    let Some(first) = tokens.peek().cloned() else {
        return Err("Unrecognized FROM item".into());
    };
    if first.kind() == SyntaxKind::L_PAREN {
        // A derived table: its own SELECT is checked node by node.
        return Ok(());
    }
    let name = identifier(&first)
        .ok_or_else(|| format!("Unrecognized FROM item starting with {}", first.text()))?;
    tokens.next();
    match tokens.peek().map(|token| token.kind()) {
        Some(SyntaxKind::DOT) => Err(format!(
            "Qualified name {name}.… is not allowed; read the Library's dependencies by their label"
        )),
        Some(SyntaxKind::L_PAREN) => {
            if allowed.contains(name.as_str()) {
                Ok(())
            } else {
                Err(format!(
                    "Function {name}() is not allowed in SQL on FHIR queries"
                ))
            }
        }
        _ if relations.contains(&name) || ctes.contains(&name) => Ok(()),
        _ => Err(format!(
            "Table {name} is not a dependency of this Library (relatedArtifact label) nor a CTE"
        )),
    }
}

/// Every name directly followed by `(` is a call the checks above saw (a
/// function or a FROM item), a CTE or alias column list, or a type modifier;
/// a call anywhere else is a construct this check does not know.
fn check_calls_are_recognized(root: &SyntaxNode) -> Result<(), String> {
    let tokens: Vec<SyntaxToken> = root
        .descendants_with_tokens()
        .filter_map(|element| match element {
            NodeOrToken::Token(token) if !token.kind().is_trivia() => Some(token.clone()),
            _ => None,
        })
        .collect();
    for pair in tokens.windows(2) {
        let [name, next] = pair else { continue };
        if next.kind() != SyntaxKind::L_PAREN
            || !matches!(name.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
        {
            continue;
        }
        let parent = name.parent().kind();
        if !matches!(
            parent,
            SyntaxKind::FUNC_CALL | SyntaxKind::TABLE_REF | SyntaxKind::CTE | SyntaxKind::TYPE_NAME
        ) {
            return Err(format!("Unrecognized call {}(...) in the SQL", name.text()));
        }
    }
    Ok(())
}

/// The function name of a call node: its tokens before `(`, unqualified.
fn callee(node: &SyntaxNode) -> Result<String, String> {
    let mut name = None;
    for token in direct_tokens(node) {
        match token.kind() {
            SyntaxKind::L_PAREN => break,
            SyntaxKind::DOT => {
                return Err("Qualified function names are not allowed".into());
            }
            kind if kind.is_trivia() => {}
            _ if name.is_some() => {
                return Err(format!("Unrecognized function call near {}", token.text()));
            }
            _ => {
                name =
                    Some(identifier(&token).unwrap_or_else(|| token.text().to_ascii_lowercase()));
            }
        }
    }
    name.ok_or_else(|| "Function call without a name".into())
}

/// The name an identifier token denotes: unquoted folds to lower case, quoted
/// is taken as written.
fn identifier(token: &SyntaxToken) -> Option<String> {
    match token.kind() {
        SyntaxKind::IDENT => Some(token.text().to_ascii_lowercase()),
        SyntaxKind::QUOTED_IDENT => {
            let text = token.text();
            let inner = text.strip_prefix('"')?.strip_suffix('"')?;
            Some(inner.replace("\"\"", "\""))
        }
        _ => None,
    }
}

fn direct_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + '_ {
    node.children_with_tokens()
        .filter_map(|element| match element {
            NodeOrToken::Token(token) => Some(token.clone()),
            NodeOrToken::Node(_) => None,
        })
}

/// The node's direct tokens, without whitespace and comments.
fn significant_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + '_ {
    direct_tokens(node).filter(|token| !token.kind().is_trivia())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relations() -> HashSet<String> {
        ["patient".to_owned(), "obs".to_owned()].into()
    }

    fn declared() -> HashMap<String, &'static str> {
        [
            ("patient_id".to_owned(), "text"),
            ("from_date".to_owned(), "date"),
        ]
        .into()
    }

    fn accepts(sql: &str) -> CheckedSql {
        check(sql, &relations(), &declared()).unwrap_or_else(|error| panic!("{sql}: {error}"))
    }

    fn rejects(sql: &str) -> String {
        check(sql, &relations(), &declared()).expect_err(sql)
    }

    #[test]
    fn analytic_queries_over_dependencies_pass() {
        accepts("SELECT gender, count(*) FROM patient GROUP BY gender");
        accepts(
            "WITH recent AS (SELECT * FROM obs WHERE effective >= :from_date) \
             SELECT p.id, count(r.id) FILTER (WHERE r.value > 1), \
             row_number() OVER (PARTITION BY p.gender ORDER BY p.id) \
             FROM patient p LEFT JOIN recent r ON r.subject = p.id \
             WHERE p.id = :patient_id GROUP BY p.id, p.gender;",
        );
        accepts("SELECT x::numeric(10, 2), CAST(y AS varchar(5)) FROM patient AS t(x, y)");
        accepts("SELECT g FROM generate_series(1, 3) g, LATERAL (SELECT 1) l");
        accepts("SELECT 1 FROM patient UNION ALL SELECT 2 FROM obs");
        accepts("SELECT extract(year FROM birth_date) FROM patient");
    }

    #[test]
    fn parameters_become_typed_numbered_placeholders() {
        let checked = accepts(
            "SELECT * FROM obs WHERE subject = :patient_id AND d >= :from_date AND s <> :patient_id",
        );
        assert_eq!(
            checked.sql,
            "SELECT * FROM obs WHERE subject = $1::text AND d >= $2::date AND s <> $1::text"
        );
        assert_eq!(checked.parameters, ["patient_id", "from_date"]);
        // A cast and a name in a string are not parameters
        let checked = accepts("SELECT ':patient_id', x::text FROM patient");
        assert!(checked.parameters.is_empty());
    }

    #[test]
    fn other_tables_qualified_names_and_statements_are_rejected() {
        assert!(rejects("SELECT * FROM users").contains("not a dependency"));
        assert!(rejects("SELECT * FROM public.patient").contains("Qualified"));
        assert!(rejects("SELECT * FROM pg_catalog.pg_authid").contains("Qualified"));
        assert!(rejects("DELETE FROM patient").contains("single SELECT"));
        assert!(rejects("SELECT 1; SELECT 2").contains("single statement"));
        assert!(rejects("SELECT * FROM patient WHERE id = $1").contains("Positional"));
        assert!(rejects("SELECT * FROM (SELECT * FROM auth_user) u").contains("auth_user"));
        assert!(rejects("SELEC 1").contains("does not parse"));
    }

    #[test]
    fn functions_outside_the_allowlist_are_rejected() {
        for sql in [
            "SELECT set_config('role', 'postgres', true)",
            "SELECT \"set_config\"('role', 'postgres', true)",
            "SELECT pg_catalog.set_config('role', 'postgres', true)",
            "SELECT current_setting('role')",
            "SELECT query_to_xml('select 1', true, true, '')",
            "SELECT pg_sleep(10)",
            "SELECT * FROM dblink('x', 'select 1') AS t",
            "SELECT count(*) FILTER (WHERE pg_sleep(1) IS NULL) FROM patient",
            "SELECT (SELECT lo_import('/etc/passwd'))",
        ] {
            let error = rejects(sql);
            assert!(
                error.contains("not allowed") || error.contains("Qualified"),
                "{sql}: {error}"
            );
        }
    }
}
