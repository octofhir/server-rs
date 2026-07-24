use crate::ir::ast::{
    CompositeClause, CompositeComponentPredicate, CompositePredicate, IdClause, IdPredicate,
    NumberClause, NumberPredicate, QuantityClause, QuantityPredicate, StringClause,
    StringPredicate, TokenClause, TokenIndexShape, TokenPredicate, UriClause, UriPredicate,
};
use crate::ir::sql::{RangeOp, SelectStmt, SqlExpr, SqlFrom, SqlOp, SqlTerm};
use crate::parameters::SearchParameterType;
use crate::parameters::SearchPrefix;
use crate::sql_builder::{SqlBuilder, SqlBuilderError, build_jsonb_accessor};
use crate::types::date_ast::{Bound, DateClause, DatePredicate, PeriodClause, PeriodPredicate};
use octofhir_core::text::normalize_string;

/// Combine rendered per-value predicate expressions into a single OR group.
/// Returns `None` for an empty input, the lone expression for one, else `Or`.
fn or_exprs(mut exprs: Vec<SqlExpr>) -> Option<SqlExpr> {
    match exprs.len() {
        0 => None,
        1 => Some(exprs.pop().unwrap()),
        _ => Some(SqlExpr::Or(exprs)),
    }
}

/// Render date clauses against a single timestamptz column.
pub fn render_date_column_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[DateClause],
    column: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| date_column_clause_expr(builder, clause, column))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render date clauses against a JSONB text extraction path cast to timestamptz.
pub fn render_date_text_path_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[DateClause],
    jsonb_path: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| date_text_path_clause_expr(builder, clause, jsonb_path))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render Period clauses against a JSONB object with `start` and `end`.
pub fn render_period_path_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[PeriodClause],
    jsonb_path: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| period_path_clause_expr(builder, clause, jsonb_path))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render composite tuple clauses as OR of AND-combined component predicates.
pub fn render_composite_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[CompositeClause],
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| render_composite_clause_expr(builder, clause))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// Render logical id clauses as one OR group over a resource id column.
pub fn render_id_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[IdClause],
    id_column: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| id_clause_expr(builder, clause, id_column))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render scalar JSONB string clauses as one OR group.
pub fn render_string_path_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
    jsonb_path: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| string_path_clause_expr(builder, clause, jsonb_path))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render string clauses over an array of FHIR objects.
pub fn render_string_array_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
    array_path: &str,
    field_name: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| string_array_clause_expr(builder, clause, array_path, field_name))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render HumanName string clauses across family, text, and given.
pub fn render_string_human_name_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
    array_path: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| string_human_name_clause_expr(builder, clause, array_path))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render scalar URI clauses as one OR group.
pub fn render_uri_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[UriClause],
    path: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| uri_clause_expr(builder, clause, path))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render URI-array clauses as one OR group.
pub fn render_uri_array_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[UriClause],
    array_path: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| uri_array_clause_expr(builder, clause, array_path))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

/// Render number clauses as one OR group over the current JSONB numeric-cast path.
pub fn render_number_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[NumberClause],
    jsonb_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| number_clause_expr(builder, clause, jsonb_path))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// Render quantity clauses as one OR group over the current JSONB numeric-cast path.
pub fn render_quantity_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[QuantityClause],
    jsonb_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| quantity_clause_expr(builder, clause, jsonb_path, None))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// Render quantity clauses with full-resource containment for system/code
/// constraints where possible, so the generic resource GIN index can prefilter
/// before numeric comparison.
pub fn render_quantity_containment_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[QuantityClause],
    jsonb_path: &str,
    path_segments: &[String],
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| quantity_clause_expr(builder, clause, jsonb_path, Some(path_segments)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// jsonpath boolean condition over `@.value`, mirroring numeric_comparison_expr's
/// FHIR precision semantics (eq/ne/ap are implicit ranges).
fn quantity_value_jsonpath_cond(
    prefix: crate::parameters::SearchPrefix,
    number: &RenderDecimalParts,
) -> String {
    use crate::parameters::SearchPrefix;
    match prefix {
        SearchPrefix::Eq => {
            let (lo, hi) = number.implicit_eq_bounds();
            format!("@.\"value\" >= {lo} && @.\"value\" < {hi}")
        }
        SearchPrefix::Ne => {
            let (lo, hi) = number.implicit_eq_bounds();
            format!("(@.\"value\" < {lo} || @.\"value\" >= {hi})")
        }
        SearchPrefix::Gt | SearchPrefix::Sa => format!("@.\"value\" > {}", number.format()),
        SearchPrefix::Lt | SearchPrefix::Eb => format!("@.\"value\" < {}", number.format()),
        SearchPrefix::Ge => format!("@.\"value\" >= {}", number.format()),
        SearchPrefix::Le => format!("@.\"value\" <= {}", number.format()),
        SearchPrefix::Ap => {
            let (lo, hi) = number.approximate_bounds();
            format!("@.\"value\" >= {lo} && @.\"value\" <= {hi}")
        }
    }
}

/// GiST-servable numeric prefilter: overlap the min/max hull of all `.value` scalars
/// matched by `hull_jp` (as a `numrange`, matching the functional hull index) against
/// the query range. `hull && [q,)` proves "some value reaches a lower bound", etc.
/// Returns None for `Ne` (low selectivity — let the `@?` recheck handle it). Single-bound
/// prefixes are exactly equivalent; `eq`/`ap` are a superset (the `@?` recheck stays).
fn quantity_hull_prefilter(
    col: &str,
    hull_jp: &str,
    prefix: crate::parameters::SearchPrefix,
    number: &RenderDecimalParts,
) -> Option<String> {
    use crate::parameters::SearchPrefix;
    let hull = format!("fhir_qty_hull_range({col}, '{hull_jp}'::jsonpath)");
    let q = match prefix {
        SearchPrefix::Eq => {
            let (lo, hi) = number.implicit_eq_bounds();
            format!("numrange({lo}::numeric, {hi}::numeric, '[)')")
        }
        SearchPrefix::Ap => {
            let (lo, hi) = number.approximate_bounds();
            format!("numrange({lo}::numeric, {hi}::numeric, '[]')")
        }
        SearchPrefix::Gt | SearchPrefix::Sa => {
            format!("numrange({}::numeric, NULL, '()')", number.format())
        }
        SearchPrefix::Ge => format!("numrange({}::numeric, NULL, '[)')", number.format()),
        SearchPrefix::Lt | SearchPrefix::Eb => {
            format!("numrange(NULL, {}::numeric, '()')", number.format())
        }
        SearchPrefix::Le => format!("numrange(NULL, {}::numeric, '(]')", number.format()),
        SearchPrefix::Ne => return None,
    };
    Some(format!("{hull} && {q}"))
}

/// Render quantity clauses whose value element sits under a repeating parent
/// (e.g. `Observation.component.valueQuantity`) as a correlated `@?` jsonpath
/// over the array. A plain `resource->parent->valueQuantity->>value` cast reads
/// NULL across an array parent (silent recall 0); the array filter is correct and
/// the whole-resource GIN serves the existence probe. Values embed as jsonpath
/// literals (numeric values validated, strings escaped).
pub fn render_quantity_array_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[QuantityClause],
    path_segments: &[String],
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    // $."seg0"[*]..."segLast"  — lax `[*]` on every non-leaf segment.
    let mut base = String::from("$");
    for (i, seg) in path_segments.iter().enumerate() {
        base.push_str(&format!(".\"{}\"", jp_quote(seg)));
        if i + 1 < path_segments.len() {
            base.push_str("[*]");
        }
    }
    let col = builder.resource_column().to_string();
    // Numeric min/max hull jsonpath (identical to the functional btree index's), used
    // as an indexable prefilter ANDed with the exact `@?` recheck.
    let hull_jp =
        crate::sql_builder::quantity_hull_value_jsonpath(path_segments).replace('\'', "''");
    let mut exprs = Vec::new();
    for clause in clauses {
        let QuantityPredicate::Comparison {
            prefix,
            value,
            system,
            code,
        } = &clause.predicate
        else {
            continue;
        };
        let number =
            RenderDecimalParts::parse(value).map_err(|_| invalid_quantity_number(value))?;
        let mut conds = vec![quantity_value_jsonpath_cond(*prefix, &number)];
        if let Some(system) = system.as_deref().filter(|s| !s.is_empty()) {
            conds.push(format!("@.\"system\" == \"{}\"", jp_quote(system)));
        }
        if let Some(code) = code.as_deref().filter(|c| !c.is_empty()) {
            let c = jp_quote(code);
            conds.push(format!("(@.\"code\" == \"{c}\" || @.\"unit\" == \"{c}\")"));
        }
        let filter = format!("{base} ? ({})", conds.join(" && ")).replace('\'', "''");
        let exact = SqlExpr::Raw(format!("{col} @? '{filter}'"));
        // Hull prefilter (btree-servable) ANDed with the exact filter. The hull is a
        // superset (eq/ap span two components) so the `@?` recheck stays for exactness;
        // single-bound prefixes are exactly equivalent but the recheck is harmless.
        exprs.push(
            match quantity_hull_prefilter(&col, &hull_jp, *prefix, &number) {
                Some(pre) => SqlExpr::And(vec![SqlExpr::Raw(pre), exact]),
                None => exact,
            },
        );
    }
    Ok(or_exprs(exprs))
}

/// Index-servable numeric condition over the union min/max btrees (`maxfn`/`minfn` =
/// `fhir_qty_extract_max/min_numeric(resource, <all value paths>)`). Single-bound
/// prefixes are EXACT — `some value > N` ⇔ `max > N`, `some value < N` ⇔ `min < N`.
/// `eq`/`ap` give a btree-servable SUPERSET (the `@?` recheck enforces exactness).
/// Returns None for `Ne` (no index help — recheck only).
fn quantity_union_index_cond(
    maxfn: &str,
    minfn: &str,
    prefix: crate::parameters::SearchPrefix,
    number: &RenderDecimalParts,
) -> Option<String> {
    use crate::parameters::SearchPrefix;
    Some(match prefix {
        SearchPrefix::Gt | SearchPrefix::Sa => format!("{maxfn} > {}", number.format()),
        SearchPrefix::Ge => format!("{maxfn} >= {}", number.format()),
        SearchPrefix::Lt | SearchPrefix::Eb => format!("{minfn} < {}", number.format()),
        SearchPrefix::Le => format!("{minfn} <= {}", number.format()),
        SearchPrefix::Eq => {
            let (lo, hi) = number.implicit_eq_bounds();
            format!("{maxfn} >= {lo} AND {minfn} < {hi}")
        }
        SearchPrefix::Ap => {
            let (lo, hi) = number.approximate_bounds();
            format!("{maxfn} >= {lo} AND {minfn} <= {hi}")
        }
        SearchPrefix::Ne => return None,
    })
}

/// Exact `@?` recheck over every value location, OR'd: `col @? '<loc> ? (<value cond>
/// [&& system] [&& code])'`. Needed for `eq`/`ne`/`ap` (the min/max prefilter is a
/// superset) and whenever system/code constrain the match (min/max says nothing about
/// the unit). Mirrors the value-cond semantics of `quantity_value_jsonpath_cond`.
fn quantity_union_recheck(
    col: &str,
    paths: &[Vec<String>],
    prefix: crate::parameters::SearchPrefix,
    number: &RenderDecimalParts,
    system: &Option<String>,
    code: &Option<String>,
) -> String {
    let arms = paths
        .iter()
        .map(|segs| {
            let mut base = String::from("$");
            for (i, seg) in segs.iter().enumerate() {
                base.push_str(&format!(".\"{}\"", jp_quote(seg)));
                if i + 1 < segs.len() {
                    base.push_str("[*]");
                }
            }
            let mut conds = vec![quantity_value_jsonpath_cond(prefix, number)];
            if let Some(system) = system.as_deref().filter(|s| !s.is_empty()) {
                conds.push(format!("@.\"system\" == \"{}\"", jp_quote(system)));
            }
            if let Some(code) = code.as_deref().filter(|c| !c.is_empty()) {
                let c = jp_quote(code);
                conds.push(format!("(@.\"code\" == \"{c}\" || @.\"unit\" == \"{c}\")"));
            }
            let filter = format!("{base} ? ({})", conds.join(" && ")).replace('\'', "''");
            format!("{col} @? '{filter}'")
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({arms})")
}

/// Render quantity clauses as one OR group, folding EVERY value location (top-level
/// `valueQuantity` and `component[*].valueQuantity`) into a SINGLE min/max btree
/// predicate over `fhir_qty_extract_min/max_numeric(resource, <all paths>)`. This is
/// the index-friendly successor to the per-location OR of a scalar btree and a
/// component hull+`@?` (which the planner could not combine, falling to a Seq Scan):
/// one clean btree expression → one Index Scan, regardless of which location matched.
pub fn render_quantity_union_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[QuantityClause],
    paths: &[Vec<String>],
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    use crate::parameters::SearchPrefix;
    if paths.is_empty() {
        return Ok(None);
    }
    let col = builder.resource_column().to_string();
    let arr = crate::sql_builder::quantity_value_jsonpath_array(paths);
    let maxfn = format!("fhir_qty_extract_max_numeric({col}, {arr})");
    let minfn = format!("fhir_qty_extract_min_numeric({col}, {arr})");
    let hullfn = format!("fhir_qty_hull_range_arr({col}, {arr})");
    let mut exprs = Vec::new();
    for clause in clauses {
        match &clause.predicate {
            QuantityPredicate::Missing { is_missing } => {
                exprs.push(SqlExpr::Raw(format!(
                    "{maxfn} IS {}NULL",
                    if *is_missing { "" } else { "NOT " }
                )));
            }
            QuantityPredicate::Comparison {
                prefix,
                value,
                system,
                code,
            } => {
                let number =
                    RenderDecimalParts::parse(value).map_err(|_| invalid_quantity_number(value))?;
                // Two-sided prefixes (eq/ap) served by a GiST hull-range overlap, not the
                // qmax/qmin btree pair: `qmax >= lo AND qmin < hi` forced a BitmapAnd whose
                // `qmin < hi` half matches most of the table (a huge bitmap the planner must
                // fully materialize before the AND, defeating LIMIT). `hull && numrange` is
                // one selective range probe that streams under LIMIT. Single-bound prefixes
                // stay on the exact qmax/qmin btrees (no over-return, no recheck).
                let index_cond = match prefix {
                    SearchPrefix::Eq => {
                        let (lo, hi) = number.implicit_eq_bounds();
                        Some(format!("{hullfn} && numrange({lo}, {hi}, '[)')"))
                    }
                    SearchPrefix::Ap => {
                        let (lo, hi) = number.approximate_bounds();
                        Some(format!("{hullfn} && numrange({lo}, {hi}, '[]')"))
                    }
                    _ => quantity_union_index_cond(&maxfn, &minfn, *prefix, &number),
                };
                // The min/max prefilter is exact for single-bound prefixes with no
                // unit constraint; otherwise an `@?` recheck enforces the exact match.
                let need_recheck = matches!(
                    prefix,
                    SearchPrefix::Eq | SearchPrefix::Ne | SearchPrefix::Ap
                ) || system.as_deref().is_some_and(|s| !s.is_empty())
                    || code.as_deref().is_some_and(|c| !c.is_empty());
                if !need_recheck {
                    if let Some(cond) = index_cond {
                        exprs.push(SqlExpr::Raw(cond));
                    }
                } else {
                    let recheck =
                        quantity_union_recheck(&col, paths, *prefix, &number, system, code);
                    match index_cond {
                        Some(cond) => exprs.push(SqlExpr::And(vec![
                            SqlExpr::Raw(cond),
                            SqlExpr::Raw(recheck),
                        ])),
                        None => exprs.push(SqlExpr::Raw(recheck)),
                    }
                }
            }
        }
    }
    Ok(or_exprs(exprs))
}

/// Render simple-code token clauses as one OR group.
///
/// This covers scalar/array code SearchParameters such as `Patient.gender`.
/// CodeableConcept/Coding and Identifier token renderers remain separate slices.
pub fn render_token_simple_code_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    path_segments: &[String],
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| {
            token_simple_code_clause_expr(builder, clause, path_segments)
                .map(|cond| token_apply_negation(clause, cond))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// Render scalar text-path code token clauses as one OR group.
pub fn render_token_scalar_code_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    jsonb_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| {
            token_scalar_code_clause_expr(builder, clause, jsonb_path)
                .map(|cond| token_apply_negation(clause, cond))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// Render Coding/CodeableConcept token clauses as one OR group.
pub fn render_token_coding_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    path_segments: &[String],
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| render_token_coding_clause(builder, clause, path_segments).map(SqlExpr::Raw))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// Render token clauses for an ARRAY-valued CodeableConcept/Coding field
/// (e.g. `Observation.category`, cardinality 0..*) as one OR group.
///
/// The scalar coding render (`render_token_coding_clause`) builds object-leaf
/// containment (`@> {"path":{"coding":[...]}}`) and `path->'coding'` traversal,
/// both of which silently miss when `path` holds an ARRAY — `array @> object` is
/// false and `array->'coding'` is NULL. This variant array-wraps the containment
/// leaf (`@> {"path":[{"coding":[...]}]}`, GIN-indexed) for the system/code cases,
/// and iterates the outer array for the cases containment can't express
/// (`|code` system-absence, `:text`).
pub fn render_token_coding_array_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    array_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| {
            render_token_coding_array_clause(builder, clause, array_path).map(SqlExpr::Raw)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

fn render_token_coding_array_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    array_path: &str,
) -> Result<String, SqlBuilderError> {
    // Subtree containment: `<array_path> @> '[ <cc_or_coding> ]'`. Driving the
    // predicate off the array subtree (e.g. resource->'category') — rather than the
    // whole resource (resource @> {category:[...]}) — lets a dedicated functional GIN
    // on that subtree serve it. The planner reliably picks the small subtree index;
    // it falls back to a Seq Scan for the whole-resource form because the global
    // resource GIN is estimated too non-selective under LIMIT.
    let contains = |builder: &mut SqlBuilder, leaf: serde_json::Value| {
        jsonb_contains_expr(builder, array_path, serde_json::Value::Array(vec![leaf]))
    };
    let condition = match &clause.predicate {
        // CodeableConcept (coding[]) and bare-Coding shapes, both array-wrapped.
        TokenPredicate::AnySystemCode { code } => render_sql_expr(&SqlExpr::Or(vec![
            contains(builder, serde_json::json!({"coding": [{"code": code}]})),
            contains(builder, serde_json::json!({"code": code})),
        ])),
        TokenPredicate::SystemCode { system, code } => render_sql_expr(&SqlExpr::Or(vec![
            contains(
                builder,
                serde_json::json!({"coding": [{"system": system, "code": code}]}),
            ),
            contains(builder, serde_json::json!({"system": system, "code": code})),
        ])),
        TokenPredicate::SystemAnyCode { system } => render_sql_expr(&SqlExpr::Or(vec![
            contains(builder, serde_json::json!({"coding": [{"system": system}]})),
            contains(builder, serde_json::json!({"system": system})),
        ])),
        // `|code` (system must be absent) — `@>` can't prove absence; iterate the
        // outer array and reuse the per-element no-system predicate.
        TokenPredicate::NoSystemCode { code } => render_sql_expr(&jsonb_array_exists_expr(
            array_path,
            "e",
            token_no_system_code_expr(builder, "e", code),
        )),
        TokenPredicate::Missing { is_missing } => {
            render_sql_expr(&jsonb_presence_expr(array_path, *is_missing))
        }
        TokenPredicate::DisplayText { text } => {
            let p = builder.add_text_param(format!("%{text}%"));
            format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS e, \
                 jsonb_array_elements(e->'coding') AS c WHERE LOWER(c->>'display') LIKE LOWER(${p}))"
            )
        }
        TokenPredicate::TerminologySet { modifier, .. } => {
            return Err(SqlBuilderError::NotImplemented(format!(
                "{} modifier requires terminology provider",
                token_set_modifier_name(*modifier)
            )));
        }
        TokenPredicate::IdentifierOfType { .. } => {
            return Err(SqlBuilderError::InvalidModifier("OfType".to_string()));
        }
    };

    if clause.negated {
        Ok(format!("({condition}) = false"))
    } else {
        Ok(condition)
    }
}

/// Render scalar (non-array) Coding/CodeableConcept token clauses as subtree `@>`
/// containment, e.g. `resource->'class' @> '{"code":"AMB"}'`. Driving the predicate
/// off the subtree (not the whole resource) lets a dedicated functional GIN on that
/// subtree serve it — the planner skips the global resource GIN as non-selective under
/// LIMIT. Shapes `@>` can't express (`|code` absence, `:text`, `:missing`) fall back to
/// the path-based render.
pub fn render_token_coding_subtree_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    subtree_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| {
            render_token_coding_subtree_clause(builder, clause, subtree_path).map(SqlExpr::Raw)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

fn render_token_coding_subtree_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    subtree_path: &str,
) -> Result<String, SqlBuilderError> {
    match token_coding_subtree_containment_expr(builder, clause, subtree_path, false) {
        // Absence / text / missing aren't `@>`-expressible — keep the path-based form
        // (handles its own negation).
        None => render_token_path_clause(builder, clause, subtree_path),
        Some(expr) => {
            let condition = render_sql_expr(&expr);
            if clause.negated {
                Ok(format!("({condition}) = false"))
            } else {
                Ok(condition)
            }
        }
    }
}

/// Subtree `@>` containment for a Coding/CodeableConcept token clause, e.g.
/// `<subtree> @> '{"coding":[{"code":"X"}]}'`, covering both bare-Coding and
/// CodeableConcept shapes (OR'd) so it serves either element type and matches a
/// dedicated subtree GIN. `None` for predicates `@>` can't express (`|code` absence,
/// `:text`, `:missing`). Negation is the caller's responsibility.
fn token_coding_subtree_containment_expr(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    subtree_path: &str,
    inline: bool,
) -> Option<SqlExpr> {
    let contains = |builder: &mut SqlBuilder, leaf: serde_json::Value| {
        if inline {
            jsonb_contains_inline_expr(subtree_path, &leaf)
        } else {
            jsonb_contains_expr(builder, subtree_path, leaf)
        }
    };
    Some(match &clause.predicate {
        TokenPredicate::AnySystemCode { code } => SqlExpr::Or(vec![
            contains(builder, serde_json::json!({"code": code})),
            contains(builder, serde_json::json!({"coding": [{"code": code}]})),
        ]),
        TokenPredicate::SystemCode { system, code } => SqlExpr::Or(vec![
            contains(builder, serde_json::json!({"system": system, "code": code})),
            contains(
                builder,
                serde_json::json!({"coding": [{"system": system, "code": code}]}),
            ),
        ]),
        TokenPredicate::SystemAnyCode { system } => SqlExpr::Or(vec![
            contains(builder, serde_json::json!({"system": system})),
            contains(builder, serde_json::json!({"coding": [{"system": system}]})),
        ]),
        _ => return None,
    })
}

/// Render Identifier token clauses as one OR group.
pub fn render_token_identifier_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    array_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| render_token_identifier_clause(builder, clause, array_path).map(SqlExpr::Raw))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// Render Identifier token clauses as one OR group, using full-resource JSONB
/// containment where it preserves FHIR identifier semantics.
///
/// `resource @> $jsonb` can use the generic resource GIN index. Cases that
/// require proving system absence or field presence still use the array path.
pub fn render_token_identifier_containment_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    path_segments: &[String],
    array_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| {
            render_token_identifier_containment_clause(builder, clause, path_segments, array_path)
                .map(SqlExpr::Raw)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

/// Render generic token clauses over an already-resolved JSONB path.
pub fn render_token_path_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    jsonb_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    let exprs = clauses
        .iter()
        .map(|clause| render_token_path_clause(builder, clause, jsonb_path).map(SqlExpr::Raw))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(or_exprs(exprs))
}

fn render_token_identifier_containment_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    path_segments: &[String],
    array_path: &str,
) -> Result<String, SqlBuilderError> {
    let resource_col = builder.resource_column().to_string();
    let condition = match &clause.predicate {
        TokenPredicate::AnySystemCode { code } => render_identifier_containment(
            builder,
            &resource_col,
            path_segments,
            serde_json::json!({"value": code}),
        ),
        TokenPredicate::SystemAnyCode { system } => render_identifier_containment(
            builder,
            &resource_col,
            path_segments,
            serde_json::json!({"system": system}),
        ),
        TokenPredicate::SystemCode { system, code } => render_identifier_containment(
            builder,
            &resource_col,
            path_segments,
            serde_json::json!({"system": system, "value": code}),
        ),
        TokenPredicate::IdentifierOfType {
            system,
            code,
            value,
        } => render_identifier_containment(
            builder,
            &resource_col,
            path_segments,
            serde_json::json!({
                "type": {"coding": [{"system": system, "code": code}]},
                "value": value
            }),
        ),
        TokenPredicate::NoSystemCode { code } => {
            render_identifier_no_system_value(builder, array_path, code)
        }
        TokenPredicate::Missing { is_missing } => {
            if *is_missing {
                format!("({array_path} IS NULL OR jsonb_array_length({array_path}) = 0)")
            } else {
                format!("({array_path} IS NOT NULL AND jsonb_array_length({array_path}) > 0)")
            }
        }
        TokenPredicate::DisplayText { .. } | TokenPredicate::TerminologySet { .. } => {
            return Err(SqlBuilderError::InvalidModifier(format!(
                "{:?}",
                clause.predicate
            )));
        }
    };

    if clause.negated {
        Ok(format!("({condition}) = false"))
    } else {
        Ok(condition)
    }
}

fn render_identifier_containment(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    identifier_value: serde_json::Value,
) -> String {
    let containment = build_nested_json_containment(
        path_segments,
        serde_json::Value::Array(vec![identifier_value]),
    );
    let p = builder.add_json_param(containment.to_string());
    format!("{resource_col} @> ${p}::jsonb")
}

fn number_clause_expr(
    builder: &mut SqlBuilder,
    clause: &NumberClause,
    jsonb_path: &str,
) -> Result<SqlExpr, SqlBuilderError> {
    match &clause.predicate {
        NumberPredicate::Missing { is_missing } => Ok(jsonb_presence_expr(jsonb_path, *is_missing)),
        NumberPredicate::Comparison { prefix, value } => {
            let number = RenderDecimalParts::parse(value)?;
            Ok(numeric_comparison_expr(
                builder, jsonb_path, *prefix, &number,
            ))
        }
    }
}

fn uri_clause_expr(builder: &mut SqlBuilder, clause: &UriClause, path: &str) -> SqlExpr {
    match &clause.predicate {
        UriPredicate::Exact { value } => {
            let p = builder.add_text_param(value);
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(path.to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            }
        }
        UriPredicate::Below { value } => {
            let escaped = escape_like_pattern(value);
            let p = builder.add_text_param(format!("{escaped}%"));
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(path.to_string()),
                op: SqlOp::Like,
                rhs: SqlTerm::Param(p),
            }
        }
        UriPredicate::Above { value } => {
            let p = builder.add_text_param(value);
            SqlExpr::Compare {
                lhs: SqlTerm::Param(p),
                op: SqlOp::Like,
                rhs: SqlTerm::Raw(format!("{path} || '%'")),
            }
        }
        UriPredicate::Contains { value } => {
            let escaped = escape_like_pattern(&value.to_lowercase());
            let p = builder.add_text_param(format!("%{escaped}%"));
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("LOWER({path})")),
                op: SqlOp::Like,
                rhs: SqlTerm::Param(p),
            }
        }
        UriPredicate::Missing { is_missing } => uri_scalar_presence_expr(path, *is_missing),
    }
}

/// Wrap a JSONB path in a CASE that normalizes scalar strings to a
/// singleton array, so `jsonb_array_elements_text` is safe regardless of
/// whether the resolved element_type_hint marked the field as an array.
fn jsonb_uri_array_normalized(array_path: &str) -> String {
    format!(
        "CASE \
         WHEN jsonb_typeof({array_path}) = 'array' THEN {array_path} \
         WHEN jsonb_typeof({array_path}) = 'string' THEN jsonb_build_array({array_path}) \
         ELSE '[]'::jsonb \
         END"
    )
}

fn uri_array_clause_expr(
    builder: &mut SqlBuilder,
    clause: &UriClause,
    array_path: &str,
) -> SqlExpr {
    let normalized = jsonb_uri_array_normalized(array_path);
    match &clause.predicate {
        UriPredicate::Exact { value } => {
            let p = builder.add_text_param(value);
            jsonb_array_text_exists_expr(
                &normalized,
                "uri",
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident("uri".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(p),
                },
            )
        }
        UriPredicate::Below { value } => {
            let escaped = escape_like_pattern(value);
            let p = builder.add_text_param(format!("{escaped}%"));
            jsonb_array_text_exists_expr(
                &normalized,
                "uri",
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident("uri".to_string()),
                    op: SqlOp::Like,
                    rhs: SqlTerm::Param(p),
                },
            )
        }
        UriPredicate::Above { value } => {
            let p = builder.add_text_param(value);
            jsonb_array_text_exists_expr(
                &normalized,
                "uri",
                SqlExpr::Compare {
                    lhs: SqlTerm::Param(p),
                    op: SqlOp::Like,
                    rhs: SqlTerm::Raw("uri || '%'".to_string()),
                },
            )
        }
        UriPredicate::Contains { value } => {
            let escaped = escape_like_pattern(&value.to_lowercase());
            let p = builder.add_text_param(format!("%{escaped}%"));
            jsonb_array_text_exists_expr(
                &normalized,
                "uri",
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident("LOWER(uri)".to_string()),
                    op: SqlOp::Like,
                    rhs: SqlTerm::Param(p),
                },
            )
        }
        UriPredicate::Missing { is_missing } => jsonb_array_presence_expr(array_path, *is_missing),
    }
}

fn uri_scalar_presence_expr(path: &str, is_missing: bool) -> SqlExpr {
    let path = SqlTerm::Ident(path.to_string());
    let null_literal = SqlTerm::Raw("'null'".to_string());
    let empty_literal = SqlTerm::Raw("'\"\"'".to_string());

    if is_missing {
        SqlExpr::Or(vec![
            SqlExpr::IsNull(path.clone()),
            SqlExpr::Compare {
                lhs: path.clone(),
                op: SqlOp::Eq,
                rhs: null_literal,
            },
            SqlExpr::Compare {
                lhs: path,
                op: SqlOp::Eq,
                rhs: empty_literal,
            },
        ])
    } else {
        SqlExpr::And(vec![
            SqlExpr::IsNotNull(path.clone()),
            SqlExpr::Compare {
                lhs: path.clone(),
                op: SqlOp::Ne,
                rhs: null_literal,
            },
            SqlExpr::Compare {
                lhs: path,
                op: SqlOp::Ne,
                rhs: empty_literal,
            },
        ])
    }
}

fn jsonb_array_presence_expr(array_path: &str, is_missing: bool) -> SqlExpr {
    let array = SqlTerm::Ident(array_path.to_string());
    let len = SqlTerm::Raw(format!("jsonb_array_length({array_path})"));

    if is_missing {
        SqlExpr::Or(vec![
            SqlExpr::IsNull(array),
            SqlExpr::Compare {
                lhs: len,
                op: SqlOp::Eq,
                rhs: SqlTerm::Integer(0),
            },
        ])
    } else {
        SqlExpr::And(vec![
            SqlExpr::IsNotNull(array),
            SqlExpr::Compare {
                lhs: len,
                op: SqlOp::Gt,
                rhs: SqlTerm::Integer(0),
            },
        ])
    }
}

fn jsonb_array_text_exists_expr(array_path: &str, alias: &str, where_clause: SqlExpr) -> SqlExpr {
    SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: format!("jsonb_array_elements_text({array_path})"),
            alias: Some(alias.to_string()),
        },
        where_clause: Some(where_clause),
    }))
}

fn string_human_name_clause_expr(
    builder: &mut SqlBuilder,
    clause: &StringClause,
    array_path: &str,
) -> SqlExpr {
    match &clause.predicate {
        StringPredicate::Prefix { value } => {
            let normalized = normalize_string(value);
            let escaped = escape_like_pattern(&normalized);
            let p = builder.add_text_param(format!("{escaped}%"));
            jsonb_array_exists_expr(
                array_path,
                "name",
                SqlExpr::Or(vec![
                    unaccent_like_expr("name->>'family'", p),
                    unaccent_like_expr("name->>'text'", p),
                    jsonb_nested_text_array_match_expr(
                        "name->'given'",
                        "g",
                        unaccent_like_expr("g", p),
                    ),
                ]),
            )
        }
        StringPredicate::Exact { value } => {
            let p = builder.add_text_param(value);
            jsonb_array_exists_expr(
                array_path,
                "name",
                SqlExpr::Or(vec![
                    text_eq_expr("name->>'family'", p),
                    text_eq_expr("name->>'text'", p),
                    jsonb_nested_text_array_match_expr("name->'given'", "g", text_eq_expr("g", p)),
                ]),
            )
        }
        StringPredicate::Contains { value } => {
            let normalized = normalize_string(value);
            let escaped = escape_like_pattern(&normalized);
            let p = builder.add_text_param(format!("%{escaped}%"));
            jsonb_array_exists_expr(
                array_path,
                "name",
                SqlExpr::Or(vec![
                    unaccent_like_expr("name->>'family'", p),
                    unaccent_like_expr("name->>'text'", p),
                    jsonb_nested_text_array_match_expr(
                        "name->'given'",
                        "g",
                        unaccent_like_expr("g", p),
                    ),
                ]),
            )
        }
        StringPredicate::Text { value } => {
            let resource_col = builder.resource_column().to_string();
            let p = builder.add_text_param(value);
            SqlExpr::Raw(format!(
                "to_tsvector('english', {resource_col}->>'text') @@ plainto_tsquery('english', ${p})"
            ))
        }
        StringPredicate::Missing { is_missing } => {
            jsonb_array_presence_expr(array_path, *is_missing)
        }
    }
}

fn date_column_clause_expr(builder: &mut SqlBuilder, clause: &DateClause, column: &str) -> SqlExpr {
    match &clause.predicate {
        DatePredicate::Contains { q } => timestamp_window_expr(
            builder,
            column,
            Some(Bound {
                at: q.start,
                inclusive: true,
            }),
            Some(Bound {
                at: q.end,
                inclusive: false,
            }),
        ),
        DatePredicate::NotContains { q } => {
            let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
            let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
            SqlExpr::Or(vec![
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident(column.to_string()),
                    op: SqlOp::Lt,
                    rhs: SqlTerm::ParamCast {
                        index: p_lo,
                        cast: "timestamptz",
                    },
                },
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident(column.to_string()),
                    op: SqlOp::Ge,
                    rhs: SqlTerm::ParamCast {
                        index: p_hi,
                        cast: "timestamptz",
                    },
                },
            ])
        }
        DatePredicate::Overlap { lo, hi } => timestamp_window_expr(builder, column, *lo, *hi),
        // Column-based path: target value is a single timestamp, not a range.
        // ge → target >= upper(q) OR target ∈ q (i.e. target >= lower(q)).
        // Combined: target >= lower(q).
        DatePredicate::Ge { q } => {
            let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(column.to_string()),
                op: SqlOp::Ge,
                rhs: SqlTerm::ParamCast {
                    index: p_lo,
                    cast: "timestamptz",
                },
            }
        }
        // le → target < lower(q) OR target ∈ q (target < upper(q)).
        // Combined: target < upper(q).
        DatePredicate::Le { q } => {
            let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(column.to_string()),
                op: SqlOp::Lt,
                rhs: SqlTerm::ParamCast {
                    index: p_hi,
                    cast: "timestamptz",
                },
            }
        }
        DatePredicate::StrictlyAfter { q } => {
            let p = builder.add_timestamp_param(format_rfc3339(&q.end));
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(column.to_string()),
                op: SqlOp::Ge,
                rhs: SqlTerm::ParamCast {
                    index: p,
                    cast: "timestamptz",
                },
            }
        }
        DatePredicate::StrictlyBefore { q } => {
            let p = builder.add_timestamp_param(format_rfc3339(&q.start));
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(column.to_string()),
                op: SqlOp::Lt,
                rhs: SqlTerm::ParamCast {
                    index: p,
                    cast: "timestamptz",
                },
            }
        }
        DatePredicate::Missing { is_missing } => {
            if *is_missing {
                SqlExpr::IsNull(SqlTerm::Ident(column.to_string()))
            } else {
                SqlExpr::IsNotNull(SqlTerm::Ident(column.to_string()))
            }
        }
    }
}

fn date_text_path_clause_expr(
    builder: &mut SqlBuilder,
    clause: &DateClause,
    jsonb_path: &str,
) -> SqlExpr {
    let timestamp_expr = format!("({jsonb_path})::timestamptz");
    date_column_clause_expr(builder, clause, &timestamp_expr)
}

fn period_path_clause_expr(
    builder: &mut SqlBuilder,
    clause: &PeriodClause,
    jsonb_path: &str,
) -> SqlExpr {
    let start_path = format!("{jsonb_path}->>'start'");
    let end_path = format!("{jsonb_path}->>'end'");

    match &clause.predicate {
        PeriodPredicate::Overlaps { q } => {
            let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
            let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
            SqlExpr::And(vec![
                SqlExpr::Or(vec![
                    SqlExpr::IsNull(SqlTerm::Ident(start_path.clone())),
                    timestamp_text_compare_expr(&start_path, SqlOp::Lt, p_hi),
                ]),
                SqlExpr::Or(vec![
                    SqlExpr::IsNull(SqlTerm::Ident(end_path.clone())),
                    timestamp_text_compare_expr(&end_path, SqlOp::Ge, p_lo),
                ]),
            ])
        }
        PeriodPredicate::NotOverlaps { q } => {
            let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
            let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
            SqlExpr::Or(vec![
                SqlExpr::And(vec![
                    SqlExpr::IsNotNull(SqlTerm::Ident(start_path.clone())),
                    timestamp_text_compare_expr(&start_path, SqlOp::Ge, p_hi),
                ]),
                SqlExpr::And(vec![
                    SqlExpr::IsNotNull(SqlTerm::Ident(end_path.clone())),
                    timestamp_text_compare_expr(&end_path, SqlOp::Lt, p_lo),
                ]),
            ])
        }
        PeriodPredicate::StartsAtOrAfter { at } => {
            let p = builder.add_timestamp_param(format_rfc3339(at));
            timestamp_text_compare_expr(&start_path, SqlOp::Ge, p)
        }
        PeriodPredicate::EndsBefore { at } => {
            let p = builder.add_timestamp_param(format_rfc3339(at));
            SqlExpr::And(vec![
                SqlExpr::IsNotNull(SqlTerm::Ident(end_path.clone())),
                timestamp_text_compare_expr(&end_path, SqlOp::Lt, p),
            ])
        }
        PeriodPredicate::HasAnyBoundAtOrAfter { at } => {
            let p = builder.add_timestamp_param(format_rfc3339(at));
            SqlExpr::Or(vec![
                timestamp_text_compare_expr(&start_path, SqlOp::Ge, p),
                SqlExpr::And(vec![
                    SqlExpr::IsNotNull(SqlTerm::Ident(end_path.clone())),
                    timestamp_text_compare_expr(&end_path, SqlOp::Ge, p),
                ]),
            ])
        }
        PeriodPredicate::BoundsBefore { at } => {
            let p = builder.add_timestamp_param(format_rfc3339(at));
            SqlExpr::And(vec![
                SqlExpr::Or(vec![
                    SqlExpr::IsNull(SqlTerm::Ident(start_path.clone())),
                    timestamp_text_compare_expr(&start_path, SqlOp::Lt, p),
                ]),
                SqlExpr::Or(vec![
                    SqlExpr::IsNull(SqlTerm::Ident(end_path.clone())),
                    timestamp_text_compare_expr(&end_path, SqlOp::Lt, p),
                ]),
            ])
        }
    }
}

fn timestamp_text_compare_expr(path: &str, op: SqlOp, param: usize) -> SqlExpr {
    SqlExpr::Compare {
        lhs: SqlTerm::Raw(format!("({path})::timestamptz")),
        op,
        rhs: SqlTerm::ParamCast {
            index: param,
            cast: "timestamptz",
        },
    }
}

/// In-place date predicate over a functional date-range expression on the
/// resource JSONB (no sidecar table). `range_expr` must be exactly
/// `tstzrange(fhir_extract_date_min(col,paths), fhir_extract_date_max(col,paths), '[]')`
/// — the same expression the matching GiST functional index is built on, so the
/// planner can use the index. `min_expr` is `fhir_extract_date_min(col,paths)`,
/// used for `:missing`.
fn date_inplace_clause_expr(
    builder: &mut SqlBuilder,
    clause: &DateClause,
    hull_expr: &str,
    mr_expr: &str,
    min_expr: &str,
    single_guard: &str,
) -> SqlExpr {
    // Two expressions over the same paths:
    //   hull = tstzrange(min, max)  — the cheap min/max span. Backs the GiST
    //          functional index (cheap to maintain AND to scan) and serves the
    //          indexable `&&`/`>>`/`<<` prefilter. Over-matches the gaps between
    //          disjoint values of a repeating date element.
    //   mr   = fhir_extract_date_multirange(...) — exact per-occurrence
    //          multirange. Expensive to compute (~1ms/row), so it is NEVER indexed
    //          and only rechecks the hull's candidate rows.
    // `single_guard` is a cheap `jsonb_typeof` test that is TRUE when the row's date
    // element holds a single occurrence (scalar string or a lone Period object). For
    // those rows the hull IS exact, so the recheck is skipped via `(guard OR recheck)`
    // — short-circuiting the per-row multirange for the overwhelmingly common
    // single-valued case while staying exact for repeating/Timing elements.
    let hull = || SqlTerm::Raw(hull_expr.to_string());
    let guarded = |recheck: SqlExpr| -> SqlExpr {
        SqlExpr::Or(vec![SqlExpr::Raw(single_guard.to_string()), recheck])
    };
    let prefilter_then_recheck = |op: RangeOp, rhs: SqlTerm| -> SqlExpr {
        SqlExpr::And(vec![
            SqlExpr::RangeOp {
                lhs: hull(),
                op,
                rhs: rhs.clone(),
            },
            guarded(SqlExpr::RangeOp {
                lhs: SqlTerm::Raw(mr_expr.to_string()),
                op,
                rhs,
            }),
        ])
    };
    match &clause.predicate {
        // `eq`: some occurrence range is contained in the query range. The hull
        // `&&` query range is a superset of the true matches, a sound indexable
        // prefilter; the EXISTS over `unnest(mr)` is the exact per-occurrence recheck.
        DatePredicate::Contains { q } => {
            let qterm = date_range_term(builder, q);
            let qsql = render_term(&qterm);
            SqlExpr::And(vec![
                SqlExpr::RangeOp {
                    lhs: hull(),
                    op: RangeOp::Overlaps,
                    rhs: qterm,
                },
                guarded(SqlExpr::Raw(format!(
                    "EXISTS (SELECT 1 FROM unnest({mr_expr}) g WHERE g <@ {qsql})"
                ))),
            ])
        }
        DatePredicate::NotContains { q } => {
            let qterm = date_range_term(builder, q);
            let qsql = render_term(&qterm);
            SqlExpr::Raw(format!(
                "NOT EXISTS (SELECT 1 FROM unnest({mr_expr}) g WHERE g <@ {qsql})"
            ))
        }
        DatePredicate::Overlap { lo, hi } => {
            prefilter_then_recheck(RangeOp::Overlaps, timestamp_range_term(builder, *lo, *hi))
        }
        DatePredicate::Ge { q } => prefilter_then_recheck(
            RangeOp::Overlaps,
            timestamp_range_term(
                builder,
                Some(Bound {
                    at: q.start,
                    inclusive: true,
                }),
                None,
            ),
        ),
        DatePredicate::Le { q } => prefilter_then_recheck(
            RangeOp::Overlaps,
            timestamp_range_term(
                builder,
                None,
                Some(Bound {
                    at: q.end,
                    inclusive: false,
                }),
            ),
        ),
        // `sa`/`eb`: strictly-after / strictly-before key off the extreme occurrence,
        // shared by hull and multirange — pure index op, no recheck.
        DatePredicate::StrictlyAfter { q } => SqlExpr::RangeOp {
            lhs: hull(),
            op: RangeOp::StrictlyAfter,
            rhs: timestamp_range_term(
                builder,
                Some(Bound {
                    at: q.end,
                    inclusive: true,
                }),
                Some(Bound {
                    at: q.end,
                    inclusive: true,
                }),
            ),
        },
        DatePredicate::StrictlyBefore { q } => SqlExpr::RangeOp {
            lhs: hull(),
            op: RangeOp::StrictlyBefore,
            rhs: timestamp_range_term(
                builder,
                Some(Bound {
                    at: q.start,
                    inclusive: true,
                }),
                Some(Bound {
                    at: q.start,
                    inclusive: true,
                }),
            ),
        },
        DatePredicate::Missing { is_missing } => {
            if *is_missing {
                SqlExpr::IsNull(SqlTerm::Raw(min_expr.to_string()))
            } else {
                SqlExpr::IsNotNull(SqlTerm::Raw(min_expr.to_string()))
            }
        }
    }
}

/// Render in-place date clauses (one OR group). `mr_expr` is the exact
/// per-occurrence multirange, served directly by the GiST functional index;
/// `min_expr` backs `:missing`. See [`date_inplace_clause_expr`].
pub fn render_date_inplace_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[DateClause],
    hull_expr: &str,
    mr_expr: &str,
    min_expr: &str,
    single_guard: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|clause| {
            date_inplace_clause_expr(builder, clause, hull_expr, mr_expr, min_expr, single_guard)
        })
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

fn date_range_term(builder: &mut SqlBuilder, q: &crate::types::date::DateRange) -> SqlTerm {
    let p_lo = builder.add_timestamp_param(format_rfc3339(&q.start));
    let p_hi = builder.add_timestamp_param(format_rfc3339(&q.end));
    SqlTerm::TimestampRange {
        lo: Box::new(SqlTerm::ParamCast {
            index: p_lo,
            cast: "timestamptz",
        }),
        hi: Box::new(SqlTerm::ParamCast {
            index: p_hi,
            cast: "timestamptz",
        }),
        bounds: "[)",
    }
}

fn timestamp_range_term(builder: &mut SqlBuilder, lo: Option<Bound>, hi: Option<Bound>) -> SqlTerm {
    let lo_term = match lo {
        Some(bound) => {
            let p = builder.add_timestamp_param(format_rfc3339(&bound.at));
            SqlTerm::ParamCast {
                index: p,
                cast: "timestamptz",
            }
        }
        None => SqlTerm::Null,
    };
    let hi_term = match hi {
        Some(bound) => {
            let p = builder.add_timestamp_param(format_rfc3339(&bound.at));
            SqlTerm::ParamCast {
                index: p,
                cast: "timestamptz",
            }
        }
        None => SqlTerm::Null,
    };
    SqlTerm::TimestampRange {
        lo: Box::new(lo_term),
        hi: Box::new(hi_term),
        bounds: range_bounds_token(
            lo.map(|b| b.inclusive).unwrap_or(true),
            hi.map(|b| b.inclusive).unwrap_or(false),
        ),
    }
}

fn range_bounds_token(lo_inc: bool, hi_inc: bool) -> &'static str {
    match (lo_inc, hi_inc) {
        (true, true) => "[]",
        (true, false) => "[)",
        (false, true) => "(]",
        (false, false) => "()",
    }
}

fn render_composite_clause_expr(
    builder: &mut SqlBuilder,
    clause: &CompositeClause,
) -> Result<SqlExpr, SqlBuilderError> {
    let components = match &clause.predicate {
        CompositePredicate::Tuple { components, .. } => components,
        CompositePredicate::Missing { .. } => {
            return Err(SqlBuilderError::NotImplemented(
                "composite :missing requires a materialized composite strategy".to_string(),
            ));
        }
    };
    let rt = &clause.resource_type;

    // Resolve each component's candidate JSONB paths. `combo-*` components bind to a
    // union of co-located paths (top-level AND component[*]); resolve every arm so
    // the composite is searched at each location, OR'd.
    let paths: Vec<Vec<Vec<String>>> = components
        .iter()
        .map(|c| {
            let p = crate::sql_builder::fhirpath_to_jsonb_paths(&c.spec.expression, rt);
            if p.is_empty() {
                vec![crate::sql_builder::fhirpath_to_jsonb_path(
                    &c.spec.expression,
                    rt,
                )]
            } else {
                p
            }
        })
        .collect();

    // Component-array arm — only when EVERY component has a path under `component`.
    // `comp_segs[i]` is component i's path within one array element.
    let comp_segs: Option<Vec<Vec<String>>> = components
        .iter()
        .enumerate()
        .map(|(i, _)| {
            paths[i]
                .iter()
                .find(|p| p.first().map(String::as_str) == Some("component"))
                .map(|p| p[1..].to_vec())
        })
        .collect();

    // Top-level arm — only when EVERY component has a non-`component` path.
    let top_paths: Option<Vec<Vec<String>>> = components
        .iter()
        .enumerate()
        .map(|(i, _)| {
            paths[i]
                .iter()
                .find(|p| p.first().map(String::as_str) != Some("component"))
                .cloned()
        })
        .collect();

    // token+quantity composite (code-value-quantity family): emit the
    // indexed form (code GIN containment AND value min/max btree, no per-row `@?`) so a
    // BitmapAnd of the two scales on large tables. Falls through to the `@?` fold below
    // for every other composite shape.
    if let Some(expr) = render_token_quantity_composite_indexed(builder, components, &paths) {
        return Ok(expr);
    }

    // When a component arm exists, fold every location into ONE `@?` jsonpath
    // `$ ? (<top> || <component>)`. The component existence filter makes the
    // whole-resource GIN serve the predicate as one Bitmap Index Scan; a SQL-level
    // OR of a GIN `@?` and a btree predicate cannot be combined and degrades to a
    // Seq Scan. A top-ONLY composite skips this — its decomposed arm below is
    // backed by the value btree (a top-level `$ ? (... && value > N)` is not
    // GIN-servable). Falls back to the SQL arms for components jsonpath can't express.
    let top_jp = top_paths
        .as_ref()
        .map(|tp| composite_arm_jsonpath_root(components, tp));
    let comp_jp = comp_segs
        .as_ref()
        .map(|cs| composite_arm_jsonpath_component(components, cs));
    let top_ok = top_paths.is_none() || matches!(top_jp, Some(Some(_)));
    let comp_ok = comp_segs.is_none() || matches!(comp_jp, Some(Some(_)));
    if comp_segs.is_some() && top_ok && comp_ok {
        let mut jp_arms: Vec<String> = Vec::new();
        if let Some(Some(a)) = comp_jp.clone() {
            jp_arms.push(a);
        }
        if let Some(Some(a)) = top_jp.clone() {
            jp_arms.push(a);
        }
        if !jp_arms.is_empty() {
            let filter = format!("$ ? ({})", jp_arms.join(" || ")).replace('\'', "''");
            return Ok(SqlExpr::Raw(format!(
                "{} @? '{filter}'",
                builder.resource_column()
            )));
        }
    }

    // Fallback: SQL OR of per-location arms (EXISTS / decomposed), for component
    // shapes jsonpath can't express (string regex, date, sa/eb/ap prefixes).
    let mut arms: Vec<SqlExpr> = Vec::new();
    if let Some(comp_segs) = comp_segs {
        arms.push(build_composite_component_arm(
            builder, components, &comp_segs,
        )?);
    }
    if let Some(top_paths) = top_paths {
        arms.push(build_composite_top_level_arm(
            builder, components, &top_paths,
        )?);
    }
    match arms.len() {
        0 => Ok(SqlExpr::Bool(false)),
        1 => Ok(arms.pop().unwrap()),
        _ => Ok(SqlExpr::Or(arms)),
    }
}

/// Indexed render for a token+quantity composite (the
/// `code-value-quantity` family): `<code containment at every location> AND
/// fhir_qty_extract_max/min(resource, <value union>) <op> N`. NO correlated `@?`
/// recheck — this trades strict same-component correlation for a
/// BitmapAnd of the code GIN and the value btree, which scales (no per-row jsonpath
/// executor). Returns None when the composite isn't exactly one token + one quantity
/// component, so the caller falls back to the general `@?` render.
fn render_token_quantity_composite_indexed(
    builder: &mut SqlBuilder,
    components: &[CompositeComponentPredicate],
    paths: &[Vec<Vec<String>>],
) -> Option<SqlExpr> {
    use crate::parameters::{SearchParameterType, SearchPrefix};
    if components.len() != 2 {
        return None;
    }
    let token_idx = components
        .iter()
        .position(|c| c.spec.search_type == SearchParameterType::Token)?;
    let quant_idx = components
        .iter()
        .position(|c| c.spec.search_type == SearchParameterType::Quantity)?;
    let col = builder.resource_column().to_string();

    // Value prefilter: one min/max btree over EVERY value location (top + component),
    // matching the Fix-A `*-value-quantity` qmax/qmin functional index. SampledData has
    // no `.value` scalar, so drop it to match the index.
    let qpaths: Vec<Vec<String>> = paths[quant_idx]
        .iter()
        .filter(|p| !p.last().is_some_and(|s| s.ends_with("SampledData")))
        .cloned()
        .collect();
    if qpaths.is_empty() {
        return None;
    }
    let arr = crate::sql_builder::quantity_value_jsonpath_array(&qpaths);
    let maxfn = format!("fhir_qty_extract_max_numeric({col}, {arr})");
    let minfn = format!("fhir_qty_extract_min_numeric({col}, {arr})");
    let (prefix_str, num_str) = extract_prefix(&components[quant_idx].value);
    let prefix = match prefix_str {
        "gt" => SearchPrefix::Gt,
        "lt" => SearchPrefix::Lt,
        "ge" => SearchPrefix::Ge,
        "le" => SearchPrefix::Le,
        "ne" => SearchPrefix::Ne,
        "sa" => SearchPrefix::Sa,
        "eb" => SearchPrefix::Eb,
        "ap" => SearchPrefix::Ap,
        _ => SearchPrefix::Eq,
    };
    let number = RenderDecimalParts::parse(num_str).ok()?;
    let value_cond = quantity_union_index_cond(&maxfn, &minfn, prefix, &number)?;

    // Code prefilter: inline whole-resource containment at every token location, OR'd
    // (top-level `code`, and `component.code` array-wrapped). Inlined (not bound) so the
    // generic resource GIN serves it.
    let token_value = &components[token_idx].value;
    let (system, code) = match token_value.split_once('|') {
        Some((s, c)) if !s.is_empty() => (Some(s), c),
        Some((_, c)) => (None, c),
        None => (None, token_value.as_str()),
    };
    if paths[token_idx].is_empty() {
        return None;
    }
    // Build via the SAME helper the partial-index DDL uses, so the query's containment
    // matches the partial's `WHERE` byte-for-byte and the planner uses the partial.
    let code_sql =
        crate::sql_builder::composite_token_containment_sql(&col, &paths[token_idx], system, code);
    Some(SqlExpr::And(vec![
        SqlExpr::Raw(code_sql),
        SqlExpr::Raw(value_cond),
    ]))
}

/// Top-level location arm as a jsonpath predicate rooted at `@` (the resource):
/// AND of each component's clause. None if any component isn't jsonpath-expressible.
fn composite_arm_jsonpath_root(
    components: &[CompositeComponentPredicate],
    top_paths: &[Vec<String>],
) -> Option<String> {
    let mut clauses = Vec::new();
    for (component, segs) in components.iter().zip(top_paths.iter()) {
        clauses.push(component_jsonpath_clause(component, segs)?);
    }
    (!clauses.is_empty()).then(|| clauses.join(" && "))
}

/// Component-array location arm as a correlated jsonpath: `exists(@.component[*] ?
/// (<clauses>))`. None if any component isn't jsonpath-expressible.
fn composite_arm_jsonpath_component(
    components: &[CompositeComponentPredicate],
    comp_segs: &[Vec<String>],
) -> Option<String> {
    let mut clauses = Vec::new();
    for (component, segs) in components.iter().zip(comp_segs.iter()) {
        clauses.push(component_jsonpath_clause(component, segs)?);
    }
    (!clauses.is_empty())
        .then(|| format!("exists(@.\"component\"[*] ? ({}))", clauses.join(" && ")))
}

/// Top-level composite arm: AND of per-component predicates at their non-array
/// paths. Co-located single elements need no correlation.
fn build_composite_top_level_arm(
    builder: &mut SqlBuilder,
    components: &[CompositeComponentPredicate],
    top_paths: &[Vec<String>],
) -> Result<SqlExpr, SqlBuilderError> {
    let col = builder.resource_column().to_string();
    let conditions = components
        .iter()
        .zip(top_paths.iter())
        .map(|(component, segs)| {
            let json_path = build_jsonb_accessor(&col, segs, component_text_leaf(component));
            render_composite_component_at_path_expr(builder, component, &json_path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if conditions.is_empty() {
        Ok(SqlExpr::Bool(true))
    } else {
        Ok(SqlExpr::And(conditions))
    }
}

/// Component-array arm: a correlated `@?` jsonpath over `$.component[*]` so all
/// predicates bind to the same array element (GIN-served). Falls back to an EXISTS
/// over jsonb_array_elements for component shapes jsonpath can't express (string
/// regex, date, sa/eb/ap prefixes). `comp_segs[i]` is component i's path WITHIN one
/// array element (the `component` prefix already stripped).
fn build_composite_component_arm(
    builder: &mut SqlBuilder,
    components: &[CompositeComponentPredicate],
    comp_segs: &[Vec<String>],
) -> Result<SqlExpr, SqlBuilderError> {
    let col = builder.resource_column().to_string();
    let conditions = components
        .iter()
        .zip(comp_segs.iter())
        .map(|(component, segs)| {
            let json_path =
                build_jsonb_accessor("component_elem", segs, component_text_leaf(component));
            render_composite_component_at_path_expr(builder, component, &json_path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if conditions.is_empty() {
        return Ok(SqlExpr::Bool(true));
    }
    let component_path = format!("{col}->'component'");
    Ok(jsonb_array_exists_expr(
        &jsonb_array_or_singleton(&component_path),
        "component_elem",
        SqlExpr::And(conditions),
    ))
}

fn id_clause_expr(builder: &mut SqlBuilder, clause: &IdClause, id_column: &str) -> SqlExpr {
    let condition = match &clause.predicate {
        IdPredicate::Equals { value } => {
            let p = builder.add_text_param(value);
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(id_column.to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            }
        }
        IdPredicate::Missing { is_missing } => {
            if *is_missing {
                SqlExpr::IsNull(SqlTerm::Ident(id_column.to_string()))
            } else {
                SqlExpr::IsNotNull(SqlTerm::Ident(id_column.to_string()))
            }
        }
    };

    if clause.negated {
        SqlExpr::Compare {
            lhs: SqlTerm::Expr(Box::new(condition)),
            op: SqlOp::Eq,
            rhs: SqlTerm::Bool(false),
        }
    } else {
        condition
    }
}

fn render_composite_component_at_path_expr(
    builder: &mut SqlBuilder,
    component: &CompositeComponentPredicate,
    json_path: &str,
) -> Result<SqlExpr, SqlBuilderError> {
    match component.spec.search_type {
        SearchParameterType::Token => {
            render_composite_token_component_expr(builder, &component.value, json_path)
        }
        SearchParameterType::String => {
            let p = builder.add_text_param(format!("{}%", component.value));
            Ok(SqlExpr::Compare {
                lhs: SqlTerm::Ident(json_path.to_string()),
                op: SqlOp::ILike,
                rhs: SqlTerm::Param(p),
            })
        }
        SearchParameterType::Quantity => {
            render_composite_quantity_component_expr(builder, &component.value, json_path)
        }
        SearchParameterType::Date => {
            let (prefix, date_str) = extract_prefix(&component.value);
            let p = builder.add_text_param(date_str);
            Ok(SqlExpr::Compare {
                lhs: SqlTerm::Raw(format!("{json_path}::timestamp")),
                op: prefix_to_sql_op(prefix),
                rhs: SqlTerm::ParamCast {
                    index: p,
                    cast: "timestamp",
                },
            })
        }
        SearchParameterType::Reference => {
            let base = to_object_path(json_path);
            let p = builder.add_text_param(&component.value);
            Ok(SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("{base}->>'reference'")),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            })
        }
        SearchParameterType::Number => {
            let (prefix, num_str) = extract_prefix(&component.value);
            let p = builder.add_text_param(num_str);
            Ok(SqlExpr::Compare {
                lhs: SqlTerm::Raw(format!("{json_path}::numeric")),
                op: prefix_to_sql_op(prefix),
                rhs: SqlTerm::ParamCast {
                    index: p,
                    cast: "numeric",
                },
            })
        }
        other => Err(SqlBuilderError::NotImplemented(format!(
            "Composite component type '{}' not supported",
            crate::ir::search_type_name(other)
        ))),
    }
}

fn component_text_leaf(component: &CompositeComponentPredicate) -> bool {
    matches!(
        component.spec.search_type,
        SearchParameterType::String
            | SearchParameterType::Date
            | SearchParameterType::Number
            | SearchParameterType::Uri
    )
}

/// jsonpath string-literal escape for a value placed inside `"..."`.
fn jp_quote(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

/// jsonpath member accessor for JSON segments, rooted at the filter variable `@`.
fn jp_member(segments: &[String]) -> String {
    let mut acc = String::from("@");
    for part in segments {
        acc.push_str(&format!(".\"{}\"", jp_quote(part)));
    }
    acc
}

/// Map a FHIR search prefix to a jsonpath comparison operator. None for prefixes
/// jsonpath can't express directly (sa/eb/ap → caller falls back to EXISTS).
fn prefix_to_jsonpath_op(prefix: &str) -> Option<&'static str> {
    match prefix {
        "eq" | "" => Some("=="),
        "ne" => Some("!="),
        "gt" => Some(">"),
        "lt" => Some("<"),
        "ge" => Some(">="),
        "le" => Some("<="),
        _ => None,
    }
}

/// Build one jsonpath boolean clause for a composite component, evaluated against
/// the current array element `@`. Returns None when the component type/value can't
/// be expressed as jsonpath, so the caller keeps the correct EXISTS fallback.
fn component_jsonpath_clause(
    component: &CompositeComponentPredicate,
    in_elem_segments: &[String],
) -> Option<String> {
    let base = jp_member(in_elem_segments);
    match component.spec.search_type {
        SearchParameterType::Token => {
            // "system|code" | "code" | "|code" | "system|". Correlate system+code
            // within a single coding via a nested `exists(... ? (...))`.
            let (system, code) = match component.value.split_once('|') {
                Some((s, c)) => (Some(s), Some(c)),
                None => (None, Some(component.value.as_str())),
            };
            let mut inner = Vec::new();
            if let Some(s) = system.filter(|s| !s.is_empty()) {
                inner.push(format!("@.\"system\" == \"{}\"", jp_quote(s)));
            }
            if let Some(c) = code.filter(|c| !c.is_empty()) {
                inner.push(format!("@.\"code\" == \"{}\"", jp_quote(c)));
            }
            if inner.is_empty() {
                return None;
            }
            Some(format!(
                "exists({base}.\"coding\"[*] ? ({}))",
                inner.join(" && ")
            ))
        }
        SearchParameterType::Quantity => {
            let num_part = component.value.split('|').next().unwrap_or("");
            let (prefix, num) = extract_prefix(num_part);
            let op = prefix_to_jsonpath_op(prefix)?;
            num.parse::<f64>().ok()?; // validate before embedding as a literal
            Some(format!("{base}.\"value\" {op} {num}"))
        }
        SearchParameterType::Number => {
            let (prefix, num) = extract_prefix(&component.value);
            let op = prefix_to_jsonpath_op(prefix)?;
            num.parse::<f64>().ok()?;
            Some(format!("{base} {op} {num}"))
        }
        SearchParameterType::Reference => Some(format!(
            "{base}.\"reference\" == \"{}\"",
            jp_quote(&component.value)
        )),
        // string (regex semantics), date (datetime compare) → EXISTS fallback.
        _ => None,
    }
}

fn jsonb_array_or_singleton(path: &str) -> String {
    format!(
        "CASE \
         WHEN jsonb_typeof({path}) = 'array' THEN {path} \
         WHEN {path} IS NULL THEN '[]'::jsonb \
         ELSE jsonb_build_array({path}) \
         END"
    )
}

fn render_composite_token_component_expr(
    builder: &mut SqlBuilder,
    value: &str,
    json_path: &str,
) -> Result<SqlExpr, SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(
        &crate::parser::ParsedParam {
            name: "composite-token".to_string(),
            modifier: None,
            values: vec![crate::parser::ParsedValue {
                prefix: None,
                raw: value.to_string(),
            }],
        },
        "",
        TokenIndexShape::Coding,
    )?;

    let parts = clauses
        .iter()
        .map(|clause| {
            // Prefer subtree `@>` containment (e.g. `resource->'code' @> '{...}'`) so the
            // composite's code component is served by the dedicated code subtree GIN and
            // BitmapAnd'd with the value btree — the scalar `->>'code'` OR form the path
            // render emits is unindexable. The literal is INLINED (not a bind param) so a
            // targeted partial composite index `WHERE <this expr>` is provably usable.
            // Fall back to the path form for shapes `@>` can't express.
            match token_coding_subtree_containment_expr(builder, clause, json_path, true) {
                Some(expr) => Ok(expr),
                None => token_path_clause_expr(builder, clause, json_path).and_then(|maybe_expr| {
                    maybe_expr.map_or_else(
                        || {
                            render_token_path_raw_clause(builder, clause, json_path)
                                .map(SqlExpr::Raw)
                        },
                        Ok,
                    )
                }),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    match parts.len() {
        0 => Err(SqlBuilderError::InvalidSearchValue(
            "empty token component".to_string(),
        )),
        1 => Ok(parts.into_iter().next().unwrap()),
        _ => Ok(SqlExpr::Or(parts)),
    }
}

fn render_composite_quantity_component_expr(
    builder: &mut SqlBuilder,
    value: &str,
    json_path: &str,
) -> Result<SqlExpr, SqlBuilderError> {
    let base = to_object_path(json_path);
    let parts: Vec<&str> = value.split('|').collect();
    let (prefix, num_str) = extract_prefix(parts[0]);

    let p = builder.add_text_param(num_str);
    let value_cond = SqlExpr::Compare {
        lhs: SqlTerm::Raw(format!("({base}->>'value')::numeric")),
        op: prefix_to_sql_op(prefix),
        rhs: SqlTerm::ParamCast {
            index: p,
            cast: "numeric",
        },
    };

    if parts.len() >= 3 {
        let mut conds = vec![value_cond];
        if !parts[1].is_empty() {
            let ps = builder.add_text_param(parts[1]);
            conds.push(SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("{base}->>'system'")),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(ps),
            });
        }
        if !parts[2].is_empty() {
            let pc = builder.add_text_param(parts[2]);
            conds.push(SqlExpr::Or(vec![
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident(format!("{base}->>'code'")),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(pc),
                },
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident(format!("{base}->>'unit'")),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(pc),
                },
            ]));
        }
        Ok(SqlExpr::And(conds))
    } else {
        Ok(value_cond)
    }
}

fn to_object_path(path: &str) -> String {
    if let Some(idx) = path.rfind("->>") {
        let last_part = path[idx + 3..].trim_matches('\'');
        format!("{}->'{}'", &path[..idx].trim_end_matches("->"), last_part)
    } else {
        path.to_string()
    }
}

fn extract_prefix(value: &str) -> (&str, &str) {
    for prefix in ["ge", "le", "gt", "lt", "ne", "sa", "eb", "ap"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            return (prefix, rest);
        }
    }
    ("eq", value)
}

fn prefix_to_sql_op(prefix: &str) -> SqlOp {
    match prefix {
        "gt" | "sa" => SqlOp::Gt,
        "lt" | "eb" => SqlOp::Lt,
        "ge" => SqlOp::Ge,
        "le" => SqlOp::Le,
        "ne" => SqlOp::Ne,
        _ => SqlOp::Eq,
    }
}

fn timestamp_window_expr(
    builder: &mut SqlBuilder,
    column: &str,
    lo: Option<Bound>,
    hi: Option<Bound>,
) -> SqlExpr {
    let mut parts = Vec::new();
    if let Some(bound) = lo {
        let p = builder.add_timestamp_param(format_rfc3339(&bound.at));
        parts.push(SqlExpr::Compare {
            lhs: SqlTerm::Ident(column.to_string()),
            op: if bound.inclusive {
                SqlOp::Ge
            } else {
                SqlOp::Gt
            },
            rhs: SqlTerm::ParamCast {
                index: p,
                cast: "timestamptz",
            },
        });
    }
    if let Some(bound) = hi {
        let p = builder.add_timestamp_param(format_rfc3339(&bound.at));
        parts.push(SqlExpr::Compare {
            lhs: SqlTerm::Ident(column.to_string()),
            op: if bound.inclusive {
                SqlOp::Le
            } else {
                SqlOp::Lt
            },
            rhs: SqlTerm::ParamCast {
                index: p,
                cast: "timestamptz",
            },
        });
    }

    match parts.len() {
        0 => SqlExpr::Bool(true),
        1 => parts.pop().unwrap(),
        _ => SqlExpr::And(parts),
    }
}

fn string_array_clause_expr(
    builder: &mut SqlBuilder,
    clause: &StringClause,
    array_path: &str,
    field_name: &str,
) -> SqlExpr {
    match &clause.predicate {
        StringPredicate::Prefix { value } => {
            let normalized = normalize_string(value);
            let escaped = escape_like_pattern(&normalized);
            let p = builder.add_text_param(format!("{escaped}%"));
            string_array_field_exists_expr(
                array_path,
                SqlExpr::Or(vec![
                    unaccent_like_expr(&format!("elem->>'{field_name}'"), p),
                    jsonb_nested_text_array_match_expr(
                        &format!("elem->'{field_name}'"),
                        "sub",
                        unaccent_like_expr("sub", p),
                    ),
                ]),
            )
        }
        StringPredicate::Exact { value } => {
            let p = builder.add_text_param(value);
            string_array_field_exists_expr(
                array_path,
                SqlExpr::Or(vec![
                    text_eq_expr(&format!("elem->>'{field_name}'"), p),
                    jsonb_nested_text_array_match_expr(
                        &format!("elem->'{field_name}'"),
                        "sub",
                        text_eq_expr("sub", p),
                    ),
                ]),
            )
        }
        StringPredicate::Contains { value } => {
            let normalized = normalize_string(value);
            let escaped = escape_like_pattern(&normalized);
            let p = builder.add_text_param(format!("%{escaped}%"));
            string_array_field_exists_expr(
                array_path,
                SqlExpr::Or(vec![
                    unaccent_like_expr(&format!("elem->>'{field_name}'"), p),
                    jsonb_nested_text_array_match_expr(
                        &format!("elem->'{field_name}'"),
                        "sub",
                        unaccent_like_expr("sub", p),
                    ),
                ]),
            )
        }
        StringPredicate::Text { value } => {
            let resource_col = builder.resource_column().to_string();
            let p = builder.add_text_param(value);
            SqlExpr::Raw(format!(
                "to_tsvector('english', {resource_col}->>'text') @@ plainto_tsquery('english', ${p})"
            ))
        }
        StringPredicate::Missing { is_missing } => {
            jsonb_array_presence_expr(array_path, *is_missing)
        }
    }
}

fn string_path_clause_expr(
    builder: &mut SqlBuilder,
    clause: &StringClause,
    jsonb_path: &str,
) -> SqlExpr {
    match &clause.predicate {
        StringPredicate::Prefix { value } => {
            let normalized = normalize_string(value);
            let escaped = escape_like_pattern(&normalized);
            let p = builder.add_text_param(format!("{escaped}%"));
            unaccent_like_expr(jsonb_path, p)
        }
        StringPredicate::Exact { value } => {
            let p = builder.add_text_param(value);
            text_eq_expr(jsonb_path, p)
        }
        StringPredicate::Contains { value } => {
            let normalized = normalize_string(value);
            let escaped = escape_like_pattern(&normalized);
            let p = builder.add_text_param(format!("%{escaped}%"));
            unaccent_like_expr(jsonb_path, p)
        }
        StringPredicate::Text { value } => {
            let resource_col = builder.resource_column().to_string();
            let p = builder.add_text_param(value);
            SqlExpr::Raw(format!(
                "to_tsvector('english', {resource_col}->>'text') @@ plainto_tsquery('english', ${p})"
            ))
        }
        StringPredicate::Missing { is_missing } => jsonb_presence_expr(jsonb_path, *is_missing),
    }
}

fn string_array_field_exists_expr(array_path: &str, where_clause: SqlExpr) -> SqlExpr {
    jsonb_array_exists_expr(array_path, "elem", where_clause)
}

fn jsonb_nested_text_array_match_expr(
    array_path: &str,
    alias: &str,
    match_expr: SqlExpr,
) -> SqlExpr {
    SqlExpr::And(vec![
        SqlExpr::Compare {
            lhs: SqlTerm::Raw(format!("jsonb_typeof({array_path})")),
            op: SqlOp::Eq,
            rhs: SqlTerm::Raw("'array'".to_string()),
        },
        jsonb_array_text_exists_expr(array_path, alias, match_expr),
    ])
}

fn unaccent_like_expr(path: &str, param: usize) -> SqlExpr {
    SqlExpr::Compare {
        lhs: SqlTerm::Raw(format!("f_unaccent_lower({path})")),
        op: SqlOp::Like,
        rhs: SqlTerm::Param(param),
    }
}

fn text_eq_expr(path: &str, param: usize) -> SqlExpr {
    SqlExpr::Compare {
        lhs: SqlTerm::Ident(path.to_string()),
        op: SqlOp::Eq,
        rhs: SqlTerm::Param(param),
    }
}

fn token_simple_code_clause_expr(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    path_segments: &[String],
) -> Result<SqlExpr, SqlBuilderError> {
    let resource_col = builder.resource_column().to_string();
    match &clause.predicate {
        TokenPredicate::Missing { is_missing } => {
            let text_path =
                crate::sql_builder::build_jsonb_accessor(&resource_col, path_segments, true);
            if *is_missing {
                Ok(SqlExpr::Or(vec![
                    SqlExpr::IsNull(SqlTerm::Ident(text_path.clone())),
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident(text_path),
                        op: SqlOp::Eq,
                        rhs: SqlTerm::Raw("'null'".to_string()),
                    },
                ]))
            } else {
                Ok(SqlExpr::And(vec![
                    SqlExpr::IsNotNull(SqlTerm::Ident(text_path.clone())),
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident(text_path),
                        op: SqlOp::Ne,
                        rhs: SqlTerm::Raw("'null'".to_string()),
                    },
                ]))
            }
        }
        TokenPredicate::SystemAnyCode { .. } => Ok(SqlExpr::Bool(false)),
        predicate => {
            let code = simple_code_token_value(predicate)?;
            let containment = build_nested_json_containment(path_segments, serde_json::json!(code));
            Ok(jsonb_contains_expr(builder, &resource_col, containment))
        }
    }
}

fn token_apply_negation(clause: &TokenClause, condition: SqlExpr) -> SqlExpr {
    if clause.negated {
        SqlExpr::Compare {
            lhs: SqlTerm::Expr(Box::new(condition)),
            op: SqlOp::Eq,
            rhs: SqlTerm::Bool(false),
        }
    } else {
        condition
    }
}

fn jsonb_contains_expr(builder: &mut SqlBuilder, lhs: &str, value: serde_json::Value) -> SqlExpr {
    let p = builder.add_json_param(value.to_string());
    SqlExpr::Compare {
        lhs: SqlTerm::Ident(lhs.to_string()),
        op: SqlOp::JsonbContains,
        rhs: SqlTerm::ParamCast {
            index: p,
            cast: "jsonb",
        },
    }
}

/// Like [`jsonb_contains_expr`] but with the JSONB operand INLINED as a literal
/// (`lhs @> '{...}'::jsonb`) instead of a bind param. A targeted partial composite
/// index (`WHERE <this exact expression>`) can only be proven usable by the planner
/// when the query's containment is a literal — a `$N::jsonb` param is opaque at plan
/// time. The literal comes from `serde_json` (well-formed) with `'` SQL-escaped; the
/// generic resource/subtree GIN serves it identically to the param form.
fn jsonb_contains_inline_expr(lhs: &str, value: &serde_json::Value) -> SqlExpr {
    let literal = value.to_string().replace('\'', "''");
    SqlExpr::Raw(format!("{lhs} @> '{literal}'::jsonb"))
}

fn token_scalar_code_clause_expr(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    jsonb_path: &str,
) -> Result<SqlExpr, SqlBuilderError> {
    match &clause.predicate {
        TokenPredicate::Missing { is_missing } => {
            if *is_missing {
                Ok(SqlExpr::Or(vec![
                    SqlExpr::IsNull(SqlTerm::Ident(jsonb_path.to_string())),
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident(jsonb_path.to_string()),
                        op: SqlOp::Eq,
                        rhs: SqlTerm::Raw("'null'".to_string()),
                    },
                ]))
            } else {
                Ok(SqlExpr::And(vec![
                    SqlExpr::IsNotNull(SqlTerm::Ident(jsonb_path.to_string())),
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident(jsonb_path.to_string()),
                        op: SqlOp::Ne,
                        rhs: SqlTerm::Raw("'null'".to_string()),
                    },
                ]))
            }
        }
        TokenPredicate::SystemAnyCode { .. } => Ok(SqlExpr::Bool(false)),
        predicate => {
            let code = simple_code_token_value(predicate)?;
            let p = builder.add_text_param(code);
            Ok(SqlExpr::Compare {
                lhs: SqlTerm::Ident(jsonb_path.to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            })
        }
    }
}

fn render_token_path_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    jsonb_path: &str,
) -> Result<String, SqlBuilderError> {
    let condition = match token_path_clause_expr(builder, clause, jsonb_path)? {
        Some(expr) => render_sql_expr(&expr),
        None => return render_token_path_raw_clause(builder, clause, jsonb_path),
    };

    if clause.negated {
        Ok(format!("({condition}) = false"))
    } else {
        Ok(condition)
    }
}

fn token_path_clause_expr(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    jsonb_path: &str,
) -> Result<Option<SqlExpr>, SqlBuilderError> {
    Ok(Some(match &clause.predicate {
        TokenPredicate::AnySystemCode { code } => {
            token_path_any_system_code_expr(builder, jsonb_path, code)
        }
        TokenPredicate::NoSystemCode { code } => {
            token_no_system_code_expr(builder, jsonb_path, code)
        }
        TokenPredicate::SystemAnyCode { system } => {
            token_system_any_code_expr(builder, jsonb_path, system)
        }
        TokenPredicate::SystemCode { system, code } => {
            token_path_system_code_expr(builder, jsonb_path, system, code)
        }
        TokenPredicate::Missing { is_missing } => jsonb_presence_expr(jsonb_path, *is_missing),
        TokenPredicate::IdentifierOfType { .. } | TokenPredicate::DisplayText { .. } => {
            return Ok(None);
        }
        TokenPredicate::TerminologySet { modifier, .. } => {
            return Err(SqlBuilderError::NotImplemented(format!(
                "{} modifier requires terminology provider",
                token_set_modifier_name(*modifier)
            )));
        }
    }))
}

fn render_token_path_raw_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    jsonb_path: &str,
) -> Result<String, SqlBuilderError> {
    match &clause.predicate {
        TokenPredicate::IdentifierOfType {
            system,
            code,
            value,
        } => Ok(render_identifier_of_type(
            builder, jsonb_path, system, code, value,
        )),
        TokenPredicate::DisplayText { text } => {
            let p = builder.add_text_param(format!("%{text}%"));
            Ok(format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c WHERE LOWER(c->>'display') LIKE LOWER(${p}))"
            ))
        }
        TokenPredicate::AnySystemCode { .. }
        | TokenPredicate::NoSystemCode { .. }
        | TokenPredicate::SystemAnyCode { .. }
        | TokenPredicate::SystemCode { .. }
        | TokenPredicate::Missing { .. }
        | TokenPredicate::TerminologySet { .. } => {
            unreachable!("handled by token_path_clause_expr")
        }
    }
}

fn token_path_any_system_code_expr(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    code: &str,
) -> SqlExpr {
    let p = builder.add_text_param(code);
    SqlExpr::Or(vec![
        SqlExpr::Compare {
            lhs: SqlTerm::Ident(format!("{jsonb_path}->>'code'")),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(p),
        },
        SqlExpr::Compare {
            lhs: SqlTerm::Ident(format!("{jsonb_path}->>'value'")),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(p),
        },
        jsonb_array_exists_expr(
            &format!("{jsonb_path}->'coding'"),
            "c",
            SqlExpr::Compare {
                lhs: SqlTerm::Ident("c->>'code'".to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            },
        ),
    ])
}

fn token_path_system_code_expr(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    system: &str,
    code: &str,
) -> SqlExpr {
    let p =
        builder.add_json_param(serde_json::json!([{"system": system, "code": code}]).to_string());
    let p_sys = builder.add_text_param(system);
    let p_code = builder.add_text_param(code);
    SqlExpr::Or(vec![
        SqlExpr::Compare {
            lhs: SqlTerm::Ident(format!("{jsonb_path}->'coding'")),
            op: SqlOp::JsonbContains,
            rhs: SqlTerm::ParamCast {
                index: p,
                cast: "jsonb",
            },
        },
        SqlExpr::And(vec![
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("{jsonb_path}->>'system'")),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p_sys),
            },
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("{jsonb_path}->>'code'")),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p_code),
            },
        ]),
        SqlExpr::And(vec![
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("{jsonb_path}->>'system'")),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p_sys),
            },
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("{jsonb_path}->>'value'")),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p_code),
            },
        ]),
    ])
}

fn render_token_identifier_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    array_path: &str,
) -> Result<String, SqlBuilderError> {
    let condition = match &clause.predicate {
        TokenPredicate::AnySystemCode { code } => {
            render_identifier_value_only(builder, array_path, code)
        }
        TokenPredicate::NoSystemCode { code } => {
            render_identifier_no_system_value(builder, array_path, code)
        }
        TokenPredicate::SystemAnyCode { system } => {
            render_identifier_system_any_value(builder, array_path, system)
        }
        TokenPredicate::SystemCode { system, code } => {
            render_identifier_system_value(builder, array_path, system, code)
        }
        TokenPredicate::IdentifierOfType {
            system,
            code,
            value,
        } => render_identifier_of_type(builder, array_path, system, code, value),
        TokenPredicate::Missing { is_missing } => {
            if *is_missing {
                format!("({array_path} IS NULL OR jsonb_array_length({array_path}) = 0)")
            } else {
                format!("({array_path} IS NOT NULL AND jsonb_array_length({array_path}) > 0)")
            }
        }
        TokenPredicate::DisplayText { .. } | TokenPredicate::TerminologySet { .. } => {
            return Err(SqlBuilderError::InvalidModifier(format!(
                "{:?}",
                clause.predicate
            )));
        }
    };

    if clause.negated {
        Ok(format!("({condition}) = false"))
    } else {
        Ok(condition)
    }
}

fn render_identifier_system_value(
    builder: &mut SqlBuilder,
    array_path: &str,
    system: &str,
    value: &str,
) -> String {
    render_sql_expr(&jsonb_contains_expr(
        builder,
        array_path,
        serde_json::json!([{"system": system, "value": value}]),
    ))
}

fn render_identifier_system_any_value(
    builder: &mut SqlBuilder,
    array_path: &str,
    system: &str,
) -> String {
    let p = builder.add_text_param(system);
    render_sql_expr(&identifier_array_exists_expr(
        array_path,
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("ident->>'system'".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(p),
        },
    ))
}

fn render_identifier_no_system_value(
    builder: &mut SqlBuilder,
    array_path: &str,
    value: &str,
) -> String {
    let p = builder.add_text_param(value);
    render_sql_expr(&identifier_array_exists_expr(
        array_path,
        SqlExpr::And(vec![
            SqlExpr::Or(vec![
                SqlExpr::IsNull(SqlTerm::Ident("ident->>'system'".to_string())),
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident("ident->>'system'".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Raw("''".to_string()),
                },
            ]),
            SqlExpr::Compare {
                lhs: SqlTerm::Ident("ident->>'value'".to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            },
        ]),
    ))
}

fn render_identifier_value_only(builder: &mut SqlBuilder, array_path: &str, value: &str) -> String {
    let p = builder.add_text_param(value);
    render_sql_expr(&identifier_array_exists_expr(
        array_path,
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("ident->>'value'".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(p),
        },
    ))
}

fn render_identifier_of_type(
    builder: &mut SqlBuilder,
    array_path: &str,
    system: &str,
    code: &str,
    value: &str,
) -> String {
    let p_coding =
        builder.add_json_param(serde_json::json!([{"system": system, "code": code}]).to_string());
    let p_val = builder.add_text_param(value);
    render_sql_expr(&identifier_array_exists_expr(
        array_path,
        SqlExpr::And(vec![
            SqlExpr::Compare {
                lhs: SqlTerm::Ident("ident->'type'->'coding'".to_string()),
                op: SqlOp::JsonbContains,
                rhs: SqlTerm::ParamCast {
                    index: p_coding,
                    cast: "jsonb",
                },
            },
            SqlExpr::Compare {
                lhs: SqlTerm::Ident("ident->>'value'".to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p_val),
            },
        ]),
    ))
}

fn identifier_array_exists_expr(array_path: &str, where_clause: SqlExpr) -> SqlExpr {
    jsonb_array_exists_expr(array_path, "ident", where_clause)
}

fn jsonb_array_exists_expr(array_path: &str, alias: &str, where_clause: SqlExpr) -> SqlExpr {
    SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: format!("jsonb_array_elements({array_path})"),
            alias: Some(alias.to_string()),
        },
        where_clause: Some(where_clause),
    }))
}

fn render_token_coding_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    path_segments: &[String],
) -> Result<String, SqlBuilderError> {
    let resource_col = builder.resource_column().to_string();
    let jsonb_path = crate::sql_builder::build_jsonb_accessor(&resource_col, path_segments, false);

    let condition = match &clause.predicate {
        TokenPredicate::AnySystemCode { code } => {
            render_token_any_system_code(builder, &resource_col, path_segments, code)
        }
        TokenPredicate::NoSystemCode { code } => {
            render_token_no_system_code(builder, &jsonb_path, code)
        }
        TokenPredicate::SystemAnyCode { system } => {
            render_token_system_any_code(builder, &jsonb_path, system)
        }
        TokenPredicate::SystemCode { system, code } => {
            render_token_system_code(builder, &resource_col, path_segments, system, code)
        }
        TokenPredicate::DisplayText { text } => {
            let p = builder.add_text_param(format!("%{text}%"));
            format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c WHERE LOWER(c->>'display') LIKE LOWER(${p}))"
            )
        }
        TokenPredicate::Missing { is_missing } => {
            render_sql_expr(&jsonb_presence_expr(&jsonb_path, *is_missing))
        }
        TokenPredicate::TerminologySet { modifier, .. } => {
            return Err(SqlBuilderError::NotImplemented(format!(
                "{} modifier requires terminology provider",
                token_set_modifier_name(*modifier)
            )));
        }
        TokenPredicate::IdentifierOfType { .. } => {
            return Err(SqlBuilderError::InvalidModifier("OfType".to_string()));
        }
    };

    if clause.negated {
        Ok(format!("({condition}) = false"))
    } else {
        Ok(condition)
    }
}

fn render_token_any_system_code(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    code: &str,
) -> String {
    render_sql_expr(&SqlExpr::Or(vec![
        token_coding_containment_expr(builder, resource_col, path_segments, None, code),
        jsonb_contains_expr(
            builder,
            resource_col,
            build_nested_json_containment(path_segments, serde_json::json!(code)),
        ),
        jsonb_contains_expr(
            builder,
            resource_col,
            build_nested_json_containment(path_segments, serde_json::json!([code])),
        ),
    ]))
}

fn render_token_system_code(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    system: &str,
    code: &str,
) -> String {
    render_sql_expr(&token_coding_containment_expr(
        builder,
        resource_col,
        path_segments,
        Some(system),
        code,
    ))
}

fn token_coding_containment_expr(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    system: Option<&str>,
    code: &str,
) -> SqlExpr {
    let coding_obj = match system {
        Some(system) => serde_json::json!({"system": system, "code": code}),
        None => serde_json::json!({"code": code}),
    };
    let cc_value = serde_json::json!({"coding": [coding_obj]});
    jsonb_contains_expr(
        builder,
        resource_col,
        build_nested_json_containment(path_segments, cc_value),
    )
}

fn render_token_no_system_code(builder: &mut SqlBuilder, jsonb_path: &str, code: &str) -> String {
    render_sql_expr(&token_no_system_code_expr(builder, jsonb_path, code))
}

fn token_no_system_code_expr(builder: &mut SqlBuilder, jsonb_path: &str, code: &str) -> SqlExpr {
    let p = builder.add_text_param(code);
    SqlExpr::Or(vec![
        SqlExpr::And(vec![
            absent_or_empty_system_expr(&format!("{jsonb_path}->>'system'")),
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("{jsonb_path}->>'code'")),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            },
        ]),
        SqlExpr::And(vec![
            absent_or_empty_system_expr(&format!("{jsonb_path}->>'system'")),
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(format!("{jsonb_path}->>'value'")),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            },
        ]),
        jsonb_array_exists_expr(
            &format!("{jsonb_path}->'coding'"),
            "c",
            SqlExpr::And(vec![
                absent_or_empty_system_expr("c->>'system'"),
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident("c->>'code'".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(p),
                },
            ]),
        ),
    ])
}

fn render_token_system_any_code(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    system: &str,
) -> String {
    render_sql_expr(&token_system_any_code_expr(builder, jsonb_path, system))
}

fn token_system_any_code_expr(builder: &mut SqlBuilder, jsonb_path: &str, system: &str) -> SqlExpr {
    let p = builder.add_text_param(system);
    SqlExpr::Or(vec![
        SqlExpr::Compare {
            lhs: SqlTerm::Ident(format!("{jsonb_path}->>'system'")),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(p),
        },
        jsonb_array_exists_expr(
            &format!("{jsonb_path}->'coding'"),
            "c",
            SqlExpr::Compare {
                lhs: SqlTerm::Ident("c->>'system'".to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(p),
            },
        ),
    ])
}

fn absent_or_empty_system_expr(path: &str) -> SqlExpr {
    SqlExpr::Or(vec![
        SqlExpr::IsNull(SqlTerm::Ident(path.to_string())),
        SqlExpr::Compare {
            lhs: SqlTerm::Ident(path.to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Raw("''".to_string()),
        },
    ])
}

fn jsonb_presence_expr(jsonb_path: &str, is_missing: bool) -> SqlExpr {
    if is_missing {
        SqlExpr::Or(vec![
            SqlExpr::IsNull(SqlTerm::Ident(jsonb_path.to_string())),
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(jsonb_path.to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Raw("'null'".to_string()),
            },
        ])
    } else {
        SqlExpr::And(vec![
            SqlExpr::IsNotNull(SqlTerm::Ident(jsonb_path.to_string())),
            SqlExpr::Compare {
                lhs: SqlTerm::Ident(jsonb_path.to_string()),
                op: SqlOp::Ne,
                rhs: SqlTerm::Raw("'null'".to_string()),
            },
        ])
    }
}

fn token_set_modifier_name(modifier: crate::ir::TokenSetModifier) -> &'static str {
    match modifier {
        crate::ir::TokenSetModifier::In => "in",
        crate::ir::TokenSetModifier::NotIn => "not-in",
        crate::ir::TokenSetModifier::Below => "below",
        crate::ir::TokenSetModifier::Above => "above",
    }
}

fn format_rfc3339(value: &time::OffsetDateTime) -> String {
    value
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| value.to_string())
}

fn simple_code_token_value(predicate: &TokenPredicate) -> Result<&str, SqlBuilderError> {
    match predicate {
        TokenPredicate::AnySystemCode { code }
        | TokenPredicate::NoSystemCode { code }
        | TokenPredicate::SystemCode { code, .. } => Ok(code),
        TokenPredicate::SystemAnyCode { .. } => Ok(""),
        TokenPredicate::IdentifierOfType { .. }
        | TokenPredicate::TerminologySet { .. }
        | TokenPredicate::DisplayText { .. }
        | TokenPredicate::Missing { .. } => {
            Err(SqlBuilderError::InvalidModifier(format!("{predicate:?}")))
        }
    }
}

fn quantity_clause_expr(
    builder: &mut SqlBuilder,
    clause: &QuantityClause,
    jsonb_path: &str,
    containment_path: Option<&[String]>,
) -> Result<SqlExpr, SqlBuilderError> {
    match &clause.predicate {
        QuantityPredicate::Missing { is_missing } => {
            Ok(jsonb_presence_expr(jsonb_path, *is_missing))
        }
        QuantityPredicate::Comparison {
            prefix,
            value,
            system,
            code,
        } => {
            let number =
                RenderDecimalParts::parse(value).map_err(|_| invalid_quantity_number(value))?;
            let num_condition = numeric_comparison_expr(
                builder,
                &format!("{jsonb_path}->>'value'"),
                *prefix,
                &number,
            );

            if system.is_none() && code.is_none() {
                return Ok(num_condition);
            }

            let mut constraints = vec![num_condition];
            if let Some(path_segments) = containment_path
                && let Some(containment) =
                    render_quantity_system_code_containment(builder, path_segments, system, code)
            {
                constraints.push(SqlExpr::Raw(containment));
            } else {
                if let Some(system) = system {
                    let p = builder.add_text_param(system);
                    constraints.push(SqlExpr::Compare {
                        lhs: SqlTerm::Ident(format!("{jsonb_path}->>'system'")),
                        op: SqlOp::Eq,
                        rhs: SqlTerm::Param(p),
                    });
                }
                if let Some(code) = code {
                    let p = builder.add_text_param(code);
                    constraints.push(SqlExpr::Or(vec![
                        SqlExpr::Compare {
                            lhs: SqlTerm::Ident(format!("{jsonb_path}->>'code'")),
                            op: SqlOp::Eq,
                            rhs: SqlTerm::Param(p),
                        },
                        SqlExpr::Compare {
                            lhs: SqlTerm::Ident(format!("{jsonb_path}->>'unit'")),
                            op: SqlOp::Eq,
                            rhs: SqlTerm::Param(p),
                        },
                    ]));
                }
            }

            Ok(SqlExpr::And(constraints))
        }
    }
}

fn render_quantity_system_code_containment(
    builder: &mut SqlBuilder,
    path_segments: &[String],
    system: &Option<String>,
    code: &Option<String>,
) -> Option<String> {
    let system = system.as_deref();
    let code = code.as_deref();
    let resource_col = builder.resource_column().to_string();

    match (system, code) {
        (None, None) => None,
        (Some(system), None) => Some(render_quantity_containment(
            builder,
            &resource_col,
            path_segments,
            serde_json::json!({"system": system}),
        )),
        (None, Some(code)) => {
            let by_code = render_quantity_containment(
                builder,
                &resource_col,
                path_segments,
                serde_json::json!({"code": code}),
            );
            let by_unit = render_quantity_containment(
                builder,
                &resource_col,
                path_segments,
                serde_json::json!({"unit": code}),
            );
            Some(format!("({by_code} OR {by_unit})"))
        }
        (Some(system), Some(code)) => {
            let by_code = render_quantity_containment(
                builder,
                &resource_col,
                path_segments,
                serde_json::json!({"system": system, "code": code}),
            );
            let by_unit = render_quantity_containment(
                builder,
                &resource_col,
                path_segments,
                serde_json::json!({"system": system, "unit": code}),
            );
            Some(format!("({by_code} OR {by_unit})"))
        }
    }
}

fn render_quantity_containment(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    quantity_value: serde_json::Value,
) -> String {
    let containment = build_nested_json_containment(path_segments, quantity_value);
    let p = builder.add_json_param(containment.to_string());
    format!("{resource_col} @> ${p}::jsonb")
}

fn build_nested_json_containment(
    path_segments: &[String],
    leaf_value: serde_json::Value,
) -> serde_json::Value {
    let mut result = leaf_value;
    for segment in path_segments.iter().rev() {
        result = serde_json::json!({ segment.as_str(): result });
    }
    result
}

/// In-place string predicate over the normalised text blob / raw value array of
/// the resource JSONB (no sidecar). `blob_expr` =
/// `fhir_text_blob(fhir_extract_text(col,paths))` (space-wrapped, matched by the
/// trigram GIN functional index); `arr_expr` = `fhir_extract_text(col,paths)`
/// (raw text[], for `:exact` and `:missing`).
fn indexed_string_clause_expr(
    builder: &mut SqlBuilder,
    clause: &StringClause,
    blob_expr: &str,
    arr_expr: &str,
) -> SqlExpr {
    match &clause.predicate {
        // Default FHIR string search: token starts-with (case/accent-insensitive).
        StringPredicate::Prefix { value } => {
            let pat = format!("% {}%", escape_like_pattern(&normalize_string(value)));
            let p = builder.add_text_param(pat);
            SqlExpr::Compare {
                lhs: SqlTerm::Raw(blob_expr.to_string()),
                op: SqlOp::Like,
                rhs: SqlTerm::Param(p),
            }
        }
        // `:contains` and (approximated) `:text`: substring, case/accent-insensitive.
        StringPredicate::Contains { value } | StringPredicate::Text { value } => {
            let pat = format!("%{}%", escape_like_pattern(&normalize_string(value)));
            let p = builder.add_text_param(pat);
            SqlExpr::Compare {
                lhs: SqlTerm::Raw(blob_expr.to_string()),
                op: SqlOp::Like,
                rhs: SqlTerm::Param(p),
            }
        }
        // `:exact`: case/accent-sensitive full equality against a raw extracted value.
        StringPredicate::Exact { value } => {
            let p = builder.add_text_param(value.clone());
            SqlExpr::Raw(format!("${p} = ANY({arr_expr})"))
        }
        StringPredicate::Missing { is_missing } => {
            if *is_missing {
                SqlExpr::IsNull(SqlTerm::Raw(arr_expr.to_string()))
            } else {
                SqlExpr::IsNotNull(SqlTerm::Raw(arr_expr.to_string()))
            }
        }
    }
}

/// Render in-place string clauses (one OR group) over the blob / array expressions.
pub fn render_indexed_string_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
    blob_expr: &str,
    arr_expr: &str,
) -> Option<SqlExpr> {
    let exprs = clauses
        .iter()
        .map(|c| indexed_string_clause_expr(builder, c, blob_expr, arr_expr))
        .collect::<Vec<_>>();
    or_exprs(exprs)
}

fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderDecimalParts {
    mantissa: i128,
    scale: u32,
}

impl RenderDecimalParts {
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

fn numeric_comparison_expr(
    builder: &mut SqlBuilder,
    path: &str,
    prefix: SearchPrefix,
    number: &RenderDecimalParts,
) -> SqlExpr {
    match prefix {
        SearchPrefix::Eq => {
            let (lower, upper) = number.implicit_eq_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            SqlExpr::And(vec![
                numeric_compare_expr(path, SqlOp::Ge, p1),
                numeric_compare_expr(path, SqlOp::Lt, p2),
            ])
        }
        SearchPrefix::Ne => {
            let (lower, upper) = number.implicit_eq_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            SqlExpr::Or(vec![
                numeric_compare_expr(path, SqlOp::Lt, p1),
                numeric_compare_expr(path, SqlOp::Ge, p2),
            ])
        }
        SearchPrefix::Gt | SearchPrefix::Sa => {
            let p = bind_numeric(builder, number.format());
            numeric_compare_expr(path, SqlOp::Gt, p)
        }
        SearchPrefix::Lt | SearchPrefix::Eb => {
            let p = bind_numeric(builder, number.format());
            numeric_compare_expr(path, SqlOp::Lt, p)
        }
        SearchPrefix::Ge => {
            let p = bind_numeric(builder, number.format());
            numeric_compare_expr(path, SqlOp::Ge, p)
        }
        SearchPrefix::Le => {
            let p = bind_numeric(builder, number.format());
            numeric_compare_expr(path, SqlOp::Le, p)
        }
        SearchPrefix::Ap => {
            let (lower, upper) = number.approximate_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            SqlExpr::And(vec![
                numeric_compare_expr(path, SqlOp::Ge, p1),
                numeric_compare_expr(path, SqlOp::Lt, p2),
            ])
        }
    }
}

fn numeric_compare_expr(path: &str, op: SqlOp, param: usize) -> SqlExpr {
    SqlExpr::Compare {
        lhs: SqlTerm::Raw(format!("({path})::numeric")),
        op,
        rhs: SqlTerm::ParamCast {
            index: param,
            cast: "numeric",
        },
    }
}

/// Render the small SQL AST to parameterized SQL text.
pub fn render_sql_expr(expr: &SqlExpr) -> String {
    match expr {
        SqlExpr::And(parts) => render_joined(parts, " AND "),
        SqlExpr::Or(parts) => render_joined(parts, " OR "),
        SqlExpr::Not(inner) => match inner.as_ref() {
            SqlExpr::Exists(select) => render_select_exists(select, true),
            _ => format!("NOT ({})", render_sql_expr(inner)),
        },
        SqlExpr::Exists(select) => render_select_exists(select, false),
        SqlExpr::Compare { lhs, op, rhs } => {
            format!(
                "{} {} {}",
                render_term(lhs),
                render_sql_op(*op),
                render_term(rhs)
            )
        }
        SqlExpr::IsNull(term) => format!("{} IS NULL", render_term(term)),
        SqlExpr::IsNotNull(term) => format!("{} IS NOT NULL", render_term(term)),
        SqlExpr::RangeOp { lhs, op, rhs } => {
            format!(
                "{} {} {}",
                render_term(lhs),
                render_range_op(*op),
                render_term(rhs)
            )
        }
        SqlExpr::Bool(true) => "TRUE".to_string(),
        SqlExpr::Bool(false) => "FALSE".to_string(),
        SqlExpr::Raw(sql) => sql.clone(),
    }
}

fn render_joined(parts: &[SqlExpr], separator: &str) -> String {
    match parts {
        [] => String::new(),
        [only] => render_sql_expr(only),
        _ => format!(
            "({})",
            parts
                .iter()
                .map(render_sql_expr)
                .collect::<Vec<_>>()
                .join(separator)
        ),
    }
}

fn render_select_exists(select: &SelectStmt, negated: bool) -> String {
    let keyword = if negated { "NOT EXISTS" } else { "EXISTS" };
    format!("{keyword} ({})", render_select_stmt(select))
}

fn render_select_stmt(select: &SelectStmt) -> String {
    let projection = if select.projection.is_empty() {
        "1".to_string()
    } else {
        select
            .projection
            .iter()
            .map(render_term)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let from = match &select.from.alias {
        Some(alias) => format!("{} {}", select.from.table, alias),
        None => select.from.table.clone(),
    };
    let where_clause = select
        .where_clause
        .as_ref()
        .map(|expr| format!(" WHERE {}", render_sql_expr(expr)))
        .unwrap_or_default();

    format!("SELECT {projection} FROM {from}{where_clause}")
}

fn render_term(term: &SqlTerm) -> String {
    match term {
        SqlTerm::Ident(name) => name.clone(),
        SqlTerm::Param(n) => format!("${n}"),
        SqlTerm::ParamCast { index, cast } => format!("${index}::{cast}"),
        SqlTerm::Expr(expr) => format!("({})", render_sql_expr(expr)),
        SqlTerm::TimestampRange { lo, hi, bounds } => {
            format!(
                "tstzrange({}, {}, '{bounds}')",
                render_term(lo),
                render_term(hi)
            )
        }
        SqlTerm::Bool(true) => "true".to_string(),
        SqlTerm::Bool(false) => "false".to_string(),
        SqlTerm::Integer(value) => value.to_string(),
        SqlTerm::Null => "NULL".to_string(),
        SqlTerm::Raw(sql) => sql.clone(),
    }
}

fn render_sql_op(op: SqlOp) -> &'static str {
    match op {
        SqlOp::Eq => "=",
        SqlOp::Ne => "!=",
        SqlOp::Like => "LIKE",
        SqlOp::ILike => "ILIKE",
        SqlOp::JsonbContains => "@>",
        SqlOp::Gt => ">",
        SqlOp::Lt => "<",
        SqlOp::Ge => ">=",
        SqlOp::Le => "<=",
    }
}

fn render_range_op(op: RangeOp) -> &'static str {
    match op {
        RangeOp::ContainsBy => "<@",
        RangeOp::Overlaps => "&&",
        RangeOp::StrictlyAfter => ">>",
        RangeOp::StrictlyBefore => "<<",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_ast_renders_range_operator_without_values() {
        let expr = SqlExpr::RangeOp {
            lhs: SqlTerm::Ident("sid.rng".to_string()),
            op: RangeOp::Overlaps,
            rhs: SqlTerm::TimestampRange {
                lo: Box::new(SqlTerm::Param(1)),
                hi: Box::new(SqlTerm::Null),
                bounds: "[)",
            },
        };

        assert_eq!(
            render_sql_expr(&expr),
            "sid.rng && tstzrange($1, NULL, '[)')"
        );
    }

    #[test]
    fn sql_ast_renders_structured_exists_select() {
        let expr = SqlExpr::Exists(Box::new(SelectStmt {
            projection: vec![SqlTerm::Integer(1)],
            from: SqlFrom {
                table: "date_range_idx".to_string(),
                alias: Some("sid".to_string()),
            },
            where_clause: Some(SqlExpr::And(vec![
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident("sid.resource_id".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Ident("r.id".to_string()),
                },
                SqlExpr::RangeOp {
                    lhs: SqlTerm::Ident("sid.rng".to_string()),
                    op: RangeOp::Overlaps,
                    rhs: SqlTerm::TimestampRange {
                        lo: Box::new(SqlTerm::ParamCast {
                            index: 1,
                            cast: "timestamptz",
                        }),
                        hi: Box::new(SqlTerm::Null),
                        bounds: "[)",
                    },
                },
            ])),
        }));

        assert_eq!(
            render_sql_expr(&expr),
            "EXISTS (SELECT 1 FROM date_range_idx sid WHERE (sid.resource_id = r.id AND sid.rng && tstzrange($1::timestamptz, NULL, '[)')))"
        );
    }

    #[test]
    fn sql_ast_renders_not_exists_without_wrapping_exists_as_boolean_expr() {
        let expr = SqlExpr::Not(Box::new(SqlExpr::Exists(Box::new(SelectStmt {
            projection: vec![SqlTerm::Integer(1)],
            from: SqlFrom {
                table: "date_range_idx".to_string(),
                alias: Some("sid".to_string()),
            },
            where_clause: Some(SqlExpr::Bool(true)),
        }))));

        assert_eq!(
            render_sql_expr(&expr),
            "NOT EXISTS (SELECT 1 FROM date_range_idx sid WHERE TRUE)"
        );
    }

    #[test]
    fn date_column_render_ne_uses_positive_range_split() {
        let mut builder = SqlBuilder::new();
        let clauses = DateClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "_lastUpdated".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: Some(SearchPrefix::Ne),
                    raw: "2024-06-15".to_string(),
                }],
            },
            "Patient",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_date_column_clauses_as_or(&mut builder, &clauses, "r.updated_at").unwrap(),
        );

        assert_eq!(
            sql,
            "(r.updated_at < $1::timestamptz OR r.updated_at >= $2::timestamptz)"
        );
        assert!(!sql.contains("NOT"));
        assert_eq!(builder.params()[0].as_str(), "2024-06-15T00:00:00Z");
        assert_eq!(builder.params()[1].as_str(), "2024-06-16T00:00:00Z");
    }

    #[test]
    fn id_render_not_uses_boolean_negation_without_not_wrapper() {
        let mut builder = SqlBuilder::new();
        let clauses = vec![IdClause {
            resource_type: "Patient".to_string(),
            param_code: "_id".to_string(),
            predicate: IdPredicate::Equals {
                value: "pat-1".to_string(),
            },
            negated: true,
        }];

        let sql =
            render_sql_expr(&render_id_clauses_as_or(&mut builder, &clauses, "r.id").unwrap());

        assert_eq!(sql, "(r.id = $1) = false");
        assert!(!sql.contains("NOT ("));
        assert_eq!(builder.params()[0].as_str(), "pat-1");
    }

    #[test]
    fn string_path_render_uses_normalized_bound_pattern() {
        let mut builder = SqlBuilder::new();
        let clauses = StringClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "name".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "Élodie".to_string(),
                }],
            },
            "Patient",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_string_path_clauses_as_or(&mut builder, &clauses, "resource->>'name'").unwrap(),
        );

        assert_eq!(sql, "f_unaccent_lower(resource->>'name') LIKE $1");
        assert_eq!(builder.params()[0].as_str(), "elodie%");
    }

    #[test]
    fn string_array_render_searches_scalar_and_nested_array_field() {
        let mut builder = SqlBuilder::new();
        let clauses = StringClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "given".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Contains),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "Ann".to_string(),
                }],
            },
            "Patient",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_string_array_clauses_as_or(&mut builder, &clauses, "resource->'name'", "given")
                .unwrap(),
        );

        assert!(sql.contains("jsonb_array_elements(resource->'name')"));
        assert!(sql.contains("elem->>'given'"));
        assert!(sql.contains("jsonb_array_elements_text(elem->'given')"));
        assert_eq!(builder.params()[0].as_str(), "%ann%");
    }

    #[test]
    fn string_human_name_render_searches_family_text_and_given() {
        let mut builder = SqlBuilder::new();
        let clauses = StringClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "name".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "Smíth".to_string(),
                }],
            },
            "Patient",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_string_human_name_clauses_as_or(&mut builder, &clauses, "resource->'name'")
                .unwrap(),
        );

        assert!(sql.contains("jsonb_array_elements(resource->'name')"));
        assert!(sql.contains("name->>'family'"));
        assert!(sql.contains("name->>'text'"));
        assert!(sql.contains("jsonb_array_elements_text"));
        assert!(sql.contains("jsonb_typeof(name->'given') = 'array'"));
        assert!(!sql.contains("COALESCE"));
        assert_eq!(builder.params()[0].as_str(), "smith%");
    }

    #[test]
    fn number_render_uses_half_open_decimal_bounds() {
        let mut builder = SqlBuilder::new();
        let clauses = NumberClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "value".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: Some(SearchPrefix::Eq),
                    raw: "5.50".to_string(),
                }],
            },
            "Observation",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_number_clauses_as_or(&mut builder, &clauses, "resource->>'value'")
                .unwrap()
                .unwrap(),
        );

        assert!(sql.contains(">= $1::numeric"));
        assert!(sql.contains("< $2::numeric"));
        assert!(!sql.contains("BETWEEN"));
        assert!(!sql.contains("5.50"));
        assert_eq!(builder.params()[0].as_str(), "5.495");
        assert_eq!(builder.params()[1].as_str(), "5.505");
    }

    #[test]
    fn quantity_render_uses_numeric_bounds_and_code_constraints() {
        let mut builder = SqlBuilder::new();
        let clauses = QuantityClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "value-quantity".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: Some(SearchPrefix::Eq),
                    raw: "5.5|http://unitsofmeasure.org|mg".to_string(),
                }],
            },
            "Observation",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_quantity_clauses_as_or(&mut builder, &clauses, "resource->'valueQuantity'")
                .unwrap()
                .unwrap(),
        );

        assert!(sql.contains("(resource->'valueQuantity'->>'value')::numeric >= $1::numeric"));
        assert!(sql.contains("(resource->'valueQuantity'->>'value')::numeric < $2::numeric"));
        assert!(sql.contains("resource->'valueQuantity'->>'system' = $3"));
        assert!(sql.contains("resource->'valueQuantity'->>'code' = $4"));
        assert!(sql.contains("resource->'valueQuantity'->>'unit' = $4"));
        assert!(!sql.contains("unitsofmeasure") && !sql.contains("mg"));
        assert_eq!(builder.params()[0].as_str(), "5.45");
        assert_eq!(builder.params()[1].as_str(), "5.55");
        assert_eq!(builder.params()[2].as_str(), "http://unitsofmeasure.org");
        assert_eq!(builder.params()[3].as_str(), "mg");
    }

    #[test]
    fn quantity_containment_render_adds_resource_gin_prefilter() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = QuantityClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "value-quantity".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: Some(SearchPrefix::Ge),
                    raw: "100|http://unitsofmeasure.org|mm[Hg]".to_string(),
                }],
            },
            "Observation",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_quantity_containment_clauses_as_or(
                &mut builder,
                &clauses,
                "r.resource->'valueQuantity'",
                &["valueQuantity".to_string()],
            )
            .unwrap()
            .unwrap(),
        );

        assert!(sql.contains("(r.resource->'valueQuantity'->>'value')::numeric >= $1::numeric"));
        assert!(sql.contains("r.resource @>"));
        assert!(!sql.contains("r.resource->'valueQuantity'->>'system'"));
        assert!(!sql.contains("unitsofmeasure") && !sql.contains("mm[Hg]"));
        assert_eq!(builder.params()[0].as_str(), "100");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&builder.params()[1].as_str()).unwrap(),
            serde_json::json!({
                "valueQuantity": {
                    "system": "http://unitsofmeasure.org",
                    "code": "mm[Hg]"
                }
            })
        );
    }

    #[test]
    fn composite_component_tuple_renders_same_element_jsonpath() {
        let mut builder = SqlBuilder::new();
        let clauses = CompositeClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "code-value-quantity".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://loinc.org|8480-6$ge100|http://unitsofmeasure.org|mm[Hg]"
                        .to_string(),
                }],
            },
            "Observation",
            &[
                crate::ir::CompositeComponentSpec {
                    code: "code".to_string(),
                    search_type: SearchParameterType::Token,
                    expression: "Observation.component.code".to_string(),
                    element_type_hint: crate::parameters::ElementTypeHint::Unknown,
                },
                crate::ir::CompositeComponentSpec {
                    code: "value-quantity".to_string(),
                    search_type: SearchParameterType::Quantity,
                    expression: "Observation.component.valueQuantity".to_string(),
                    element_type_hint: crate::parameters::ElementTypeHint::Unknown,
                },
            ],
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_composite_clauses_as_or(&mut builder, &clauses)
                .unwrap()
                .unwrap(),
        );

        // Same-element correlation via one `@?` jsonpath over $.component[*]; the
        // token sub-match correlates system+code within a coding via nested
        // `exists(...)`. Values are embedded as jsonpath literals so the whole-
        // resource GIN can serve the predicate.
        assert!(sql.contains("@?"));
        assert!(sql.contains(r#"exists(@."component"[*] ?"#));
        assert!(sql.contains(r#"exists(@."code"."coding"[*] ?"#));
        assert!(sql.contains(r#"@."system" == "http://loinc.org""#));
        assert!(sql.contains(r#"@."code" == "8480-6""#));
        assert!(sql.contains(r#"@."valueQuantity"."value" >= 100"#));
        // Correlated within the array element — never top-level navigation.
        assert!(!sql.contains("resource->'component'->'code'"));
    }

    #[test]
    fn simple_code_token_render_uses_jsonb_containment() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "female".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_simple_code_clauses_as_or(
                &mut builder,
                &clauses,
                &["gender".to_string()],
            )
            .unwrap()
            .unwrap(),
        );

        assert_eq!(sql, "r.resource @> $1::jsonb");
        assert!(!sql.contains("female"));
        assert_eq!(builder.params()[0].as_str(), r#"{"gender":"female"}"#);
    }

    #[test]
    fn simple_code_token_render_not_uses_boolean_false_check() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Not),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "female".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_simple_code_clauses_as_or(
                &mut builder,
                &clauses,
                &["gender".to_string()],
            )
            .unwrap()
            .unwrap(),
        );

        assert_eq!(sql, "(r.resource @> $1::jsonb) = false");
        assert_eq!(builder.params()[0].as_str(), r#"{"gender":"female"}"#);
    }

    #[test]
    fn simple_code_token_render_no_system_code_uses_value_containment() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "|female".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_simple_code_clauses_as_or(
                &mut builder,
                &clauses,
                &["gender".to_string()],
            )
            .unwrap()
            .unwrap(),
        );

        assert_eq!(sql, "r.resource @> $1::jsonb");
        assert_eq!(builder.params()[0].as_str(), r#"{"gender":"female"}"#);
    }

    #[test]
    fn simple_code_token_render_system_any_code_matches_nothing() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://example.org|".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_simple_code_clauses_as_or(
                &mut builder,
                &clauses,
                &["gender".to_string()],
            )
            .unwrap()
            .unwrap(),
        );

        assert_eq!(sql, "FALSE");
        assert!(builder.params().is_empty());
    }

    #[test]
    fn scalar_code_token_render_uses_text_path_and_ignores_system() {
        let mut builder = SqlBuilder::new();
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://example.org|female".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_scalar_code_clauses_as_or(&mut builder, &clauses, "resource->>'gender'")
                .unwrap()
                .unwrap(),
        );

        assert_eq!(sql, "resource->>'gender' = $1");
        assert_eq!(builder.params()[0].as_str(), "female");
    }

    #[test]
    fn scalar_code_token_render_system_any_code_matches_nothing() {
        let mut builder = SqlBuilder::new();
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://example.org|".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_scalar_code_clauses_as_or(&mut builder, &clauses, "resource->>'gender'")
                .unwrap()
                .unwrap(),
        );

        assert_eq!(sql, "FALSE");
        assert!(builder.params().is_empty());
    }

    #[test]
    fn scalar_code_token_render_not_uses_boolean_false_check() {
        let mut builder = SqlBuilder::new();
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Not),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "female".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_scalar_code_clauses_as_or(&mut builder, &clauses, "resource->>'gender'")
                .unwrap()
                .unwrap(),
        );

        assert_eq!(sql, "(resource->>'gender' = $1) = false");
        assert!(!sql.contains("NOT ("));
        assert_eq!(builder.params()[0].as_str(), "female");
    }

    #[test]
    fn coding_token_render_preserves_system_code_as_containment() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "code".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://loinc.org|8480-6".to_string(),
                }],
            },
            "Observation",
            crate::ir::TokenIndexShape::Coding,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_coding_clauses_as_or(&mut builder, &clauses, &["code".to_string()])
                .unwrap()
                .unwrap(),
        );

        assert_eq!(sql, "r.resource @> $1::jsonb");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&builder.params()[0].as_str()).unwrap(),
            serde_json::json!({
                "code": {
                    "coding": [{
                        "system": "http://loinc.org",
                        "code": "8480-6"
                    }]
                }
            })
        );
    }

    #[test]
    fn coding_token_render_not_uses_boolean_false_check() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "code".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Not),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://loinc.org|8480-6".to_string(),
                }],
            },
            "Observation",
            crate::ir::TokenIndexShape::Coding,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_coding_clauses_as_or(&mut builder, &clauses, &["code".to_string()])
                .unwrap()
                .unwrap(),
        );

        assert_eq!(sql, "(r.resource @> $1::jsonb) = false");
        assert!(!sql.contains("NOT ("));
    }

    #[test]
    fn identifier_token_render_preserves_system_value_containment() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "identifier".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://test.org|debug-123".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::Identifier,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_identifier_clauses_as_or(
                &mut builder,
                &clauses,
                "r.resource->'identifier'",
            )
            .unwrap()
            .unwrap(),
        );

        assert_eq!(sql, "r.resource->'identifier' @> $1::jsonb");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&builder.params()[0].as_str()).unwrap(),
            serde_json::json!([{
                "system": "http://test.org",
                "value": "debug-123"
            }])
        );
    }

    #[test]
    fn identifier_token_render_not_uses_boolean_false_check() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "identifier".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Not),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "|debug-123".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::Identifier,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_identifier_clauses_as_or(
                &mut builder,
                &clauses,
                "r.resource->'identifier'",
            )
            .unwrap()
            .unwrap(),
        );

        assert!(sql.starts_with("(EXISTS"));
        assert!(sql.ends_with("= false"));
        assert!(!sql.contains("NOT ("));
    }

    #[test]
    fn identifier_token_containment_render_uses_resource_gin_shape() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "identifier".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://test.org|debug-123".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::Identifier,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_identifier_containment_clauses_as_or(
                &mut builder,
                &clauses,
                &["identifier".to_string()],
                "r.resource->'identifier'",
            )
            .unwrap()
            .unwrap(),
        );

        assert_eq!(sql, "r.resource @> $1::jsonb");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&builder.params()[0].as_str()).unwrap(),
            serde_json::json!({
                "identifier": [{
                    "system": "http://test.org",
                    "value": "debug-123"
                }]
            })
        );
    }

    #[test]
    fn uri_render_escapes_like_patterns() {
        let mut builder = SqlBuilder::new();
        let clauses = UriClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "url".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Below),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://example.org/100%".to_string(),
                }],
            },
            "ImplementationGuide",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_uri_clauses_as_or(&mut builder, &clauses, "resource->>'url'").unwrap(),
        );

        assert_eq!(sql, "resource->>'url' LIKE $1");
        assert_eq!(builder.params()[0].as_str(), "http://example.org/100\\%%");
    }

    #[test]
    fn uri_array_render_uses_array_elements_text() {
        let mut builder = SqlBuilder::new();
        let clauses = UriClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "_profile".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://hl7.org/fhir/us/core/Patient".to_string(),
                }],
            },
            "Patient",
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_uri_array_clauses_as_or(&mut builder, &clauses, "resource->'meta'->'profile'")
                .unwrap(),
        );

        // Path is normalized via a CASE so jsonb_array_elements_text works on
        // both array and scalar JSONB shapes.
        assert!(sql.contains("jsonb_array_elements_text(CASE"));
        assert!(sql.contains("jsonb_typeof(resource->'meta'->'profile') = 'array'"));
        assert!(sql.contains("uri = $1"));
    }

    #[test]
    fn token_path_render_not_uses_boolean_false_check() {
        let mut builder = SqlBuilder::new();
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "status".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Not),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "active".to_string(),
                }],
            },
            "Observation",
            crate::ir::TokenIndexShape::Coding,
        )
        .unwrap();

        let sql = render_sql_expr(
            &render_token_path_clauses_as_or(&mut builder, &clauses, "resource->'status'")
                .unwrap()
                .unwrap(),
        );

        assert!(sql.starts_with("("));
        assert!(sql.ends_with("= false"));
        assert!(!sql.contains("NOT ("));
    }
}
