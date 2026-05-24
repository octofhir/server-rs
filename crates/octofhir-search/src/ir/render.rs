use crate::ir::ast::{
    CompositeClause, CompositeComponentPredicate, CompositePredicate, CompositeSafety, IdClause,
    IdPredicate, NumberClause, NumberPredicate, QuantityClause, QuantityPredicate, ReferenceClause,
    ReferencePredicate, StringClause, StringPredicate, TokenClause, TokenIndexShape,
    TokenPredicate, UriClause, UriPredicate,
};
use crate::ir::sql::{RangeOp, SelectStmt, SqlExpr, SqlFrom, SqlOp, SqlTerm};
use crate::parameters::SearchParameterType;
use crate::parameters::SearchPrefix;
use crate::parser::{ParsedParam, ParsedValue};
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::types::date_ast::{Bound, DateClause, DatePredicate, PeriodClause, PeriodPredicate};
use octofhir_core::search_index::normalize_string;

/// Render date sidecar clauses as one OR group.
pub fn render_date_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[DateClause],
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_sql_expr(&date_sidecar_clause_expr(builder, clause)))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render date clauses against a single timestamptz column.
pub fn render_date_column_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[DateClause],
    column: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_sql_expr(&date_column_clause_expr(builder, clause, column)))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render date clauses against a JSONB text extraction path cast to timestamptz.
pub fn render_date_text_path_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[DateClause],
    jsonb_path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_date_text_path_clause(builder, clause, jsonb_path))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render Period clauses against a JSONB object with `start` and `end`.
pub fn render_period_path_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[PeriodClause],
    jsonb_path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_period_path_clause(builder, clause, jsonb_path))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render composite tuple clauses as OR of AND-combined component predicates.
pub fn render_composite_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[CompositeClause],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| {
            render_composite_clause_expr(builder, clause).map(|expr| render_sql_expr(&expr))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render composite tuple clauses through JSONB traversal.
///
/// This is intentionally kept out of the native production renderer. It is used
/// only for user-defined SearchParameters that cannot have prebuilt native
/// sidecar rows until the parameter is promoted into package metadata.
pub fn render_composite_clauses_as_jsonb_fallback_or(
    builder: &mut SqlBuilder,
    clauses: &[CompositeClause],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| {
            render_composite_clause_jsonb_fallback_expr(builder, clause)
                .map(|expr| render_sql_expr(&expr))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render logical id clauses as one OR group over a resource id column.
pub fn render_id_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[IdClause],
    id_column: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_sql_expr(&id_clause_expr(builder, clause, id_column)))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render string sidecar clauses as one OR group.
pub fn render_string_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_string_clause(builder, clause))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render scalar JSONB string clauses as one OR group.
pub fn render_string_path_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
    jsonb_path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_string_path_clause(builder, clause, jsonb_path))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render string clauses over an array of FHIR objects.
pub fn render_string_array_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
    array_path: &str,
    field_name: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_string_array_clause(builder, clause, array_path, field_name))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render HumanName string clauses across family, text, and given.
pub fn render_string_human_name_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
    array_path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_string_human_name_clause(builder, clause, array_path))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render reference clauses as one OR group.
pub fn render_reference_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[ReferenceClause],
    jsonb_path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_reference_clause(builder, clause, jsonb_path))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render scalar URI clauses as one OR group.
pub fn render_uri_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[UriClause],
    path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_sql_expr(&uri_clause_expr(builder, clause, path)))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render URI-array clauses as one OR group.
pub fn render_uri_array_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[UriClause],
    array_path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_sql_expr(&uri_array_clause_expr(builder, clause, array_path)))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render number clauses as one OR group over the current JSONB numeric-cast path.
pub fn render_number_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[NumberClause],
    jsonb_path: &str,
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_number_clause(builder, clause, jsonb_path))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render number clauses through `search_idx_number`.
pub fn render_number_index_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[NumberClause],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_number_index_clause(builder, clause))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render quantity clauses as one OR group over the current JSONB numeric-cast path.
pub fn render_quantity_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[QuantityClause],
    jsonb_path: &str,
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_quantity_clause(builder, clause, jsonb_path, None))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render quantity clauses with full-resource containment for system/code
/// constraints where possible, so the generic resource GIN index can prefilter
/// before numeric comparison.
pub fn render_quantity_containment_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[QuantityClause],
    jsonb_path: &str,
    path_segments: &[String],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_quantity_clause(builder, clause, jsonb_path, Some(path_segments)))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render quantity clauses through `search_idx_quantity`.
pub fn render_quantity_index_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[QuantityClause],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_quantity_index_clause(builder, clause))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render simple-code token clauses as one OR group.
///
/// This covers scalar/array code SearchParameters such as `Patient.gender`.
/// CodeableConcept/Coding and Identifier token renderers remain separate slices.
pub fn render_token_simple_code_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    path_segments: &[String],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_token_simple_code_clause(builder, clause, path_segments))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render scalar text-path code token clauses as one OR group.
pub fn render_token_scalar_code_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    jsonb_path: &str,
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_token_scalar_code_clause(builder, clause, jsonb_path))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render Coding/CodeableConcept token clauses as one OR group.
pub fn render_token_coding_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    path_segments: &[String],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_token_coding_clause(builder, clause, path_segments))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render Identifier token clauses as one OR group.
pub fn render_token_identifier_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    array_path: &str,
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_token_identifier_clause(builder, clause, array_path))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
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
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| {
            render_token_identifier_containment_clause(builder, clause, path_segments, array_path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render generic token clauses over an already-resolved JSONB path.
pub fn render_token_path_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    jsonb_path: &str,
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_token_path_clause(builder, clause, jsonb_path))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
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

fn render_number_clause(
    builder: &mut SqlBuilder,
    clause: &NumberClause,
    jsonb_path: &str,
) -> Result<String, SqlBuilderError> {
    match &clause.predicate {
        NumberPredicate::Missing { is_missing } => Ok(render_sql_expr(&jsonb_presence_expr(
            jsonb_path,
            *is_missing,
        ))),
        NumberPredicate::Comparison { prefix, value } => {
            let number = RenderDecimalParts::parse(value)?;
            Ok(render_numeric_comparison(
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

fn uri_array_clause_expr(
    builder: &mut SqlBuilder,
    clause: &UriClause,
    array_path: &str,
) -> SqlExpr {
    match &clause.predicate {
        UriPredicate::Exact { value } => {
            let p = builder.add_text_param(value);
            jsonb_array_text_exists_expr(
                array_path,
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
                array_path,
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
                array_path,
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
                array_path,
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

fn render_string_human_name_clause(
    builder: &mut SqlBuilder,
    clause: &StringClause,
    array_path: &str,
) -> String {
    render_sql_expr(&string_human_name_clause_expr(builder, clause, array_path))
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

fn render_date_text_path_clause(
    builder: &mut SqlBuilder,
    clause: &DateClause,
    jsonb_path: &str,
) -> String {
    let timestamp_expr = format!("({jsonb_path})::timestamptz");
    render_sql_expr(&date_column_clause_expr(builder, clause, &timestamp_expr))
}

fn render_period_path_clause(
    builder: &mut SqlBuilder,
    clause: &PeriodClause,
    jsonb_path: &str,
) -> String {
    render_sql_expr(&period_path_clause_expr(builder, clause, jsonb_path))
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

fn date_sidecar_clause_expr(builder: &mut SqlBuilder, clause: &DateClause) -> SqlExpr {
    let rt_param = builder.add_text_param(&clause.resource_type);
    let pc_param = builder.add_text_param(&clause.param_code);
    let id_col = builder.id_column().to_string();

    let mut where_parts = vec![
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sid.resource_type".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(rt_param),
        },
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sid.resource_id".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Ident(id_col),
        },
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sid.param_code".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(pc_param),
        },
    ];

    let (negate_exists, predicate) = match &clause.predicate {
        DatePredicate::Contains { q } => (
            false,
            SqlExpr::RangeOp {
                lhs: SqlTerm::Ident("sid.rng".to_string()),
                op: RangeOp::ContainsBy,
                rhs: date_range_term(builder, q),
            },
        ),
        DatePredicate::NotContains { q } => (
            true,
            SqlExpr::RangeOp {
                lhs: SqlTerm::Ident("sid.rng".to_string()),
                op: RangeOp::ContainsBy,
                rhs: date_range_term(builder, q),
            },
        ),
        DatePredicate::Overlap { lo, hi } => (
            false,
            SqlExpr::RangeOp {
                lhs: SqlTerm::Ident("sid.rng".to_string()),
                op: RangeOp::Overlaps,
                rhs: timestamp_range_term(builder, *lo, *hi),
            },
        ),
        DatePredicate::StrictlyAfter { q } => (
            false,
            SqlExpr::RangeOp {
                lhs: SqlTerm::Ident("sid.rng".to_string()),
                op: RangeOp::StrictlyAfter,
                rhs: date_range_term(builder, q),
            },
        ),
        DatePredicate::StrictlyBefore { q } => (
            false,
            SqlExpr::RangeOp {
                lhs: SqlTerm::Ident("sid.rng".to_string()),
                op: RangeOp::StrictlyBefore,
                rhs: date_range_term(builder, q),
            },
        ),
        DatePredicate::Missing { is_missing } => {
            let exists = SqlExpr::Exists(Box::new(SelectStmt {
                projection: vec![SqlTerm::Integer(1)],
                from: SqlFrom {
                    table: "search_idx_date".to_string(),
                    alias: Some("sid".to_string()),
                },
                where_clause: Some(SqlExpr::And(where_parts)),
            }));
            return if *is_missing {
                SqlExpr::Not(Box::new(exists))
            } else {
                exists
            };
        }
    };
    where_parts.push(predicate);

    let exists = SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: "search_idx_date".to_string(),
            alias: Some("sid".to_string()),
        },
        where_clause: Some(SqlExpr::And(where_parts)),
    }));

    if negate_exists {
        SqlExpr::Not(Box::new(exists))
    } else {
        exists
    }
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
    match &clause.predicate {
        CompositePredicate::Tuple { components, safety } => {
            if matches!(safety, CompositeSafety::RequiresSameElement) {
                return Err(SqlBuilderError::NotImplemented(
                    "same-element composite search requires a materialized native composite strategy"
                        .to_string(),
                ));
            }

            let conditions = components
                .iter()
                .map(|component| {
                    render_composite_component_native_expr(
                        builder,
                        &clause.resource_type,
                        component,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if conditions.is_empty() {
                Ok(SqlExpr::Bool(true))
            } else {
                Ok(SqlExpr::And(conditions))
            }
        }
        CompositePredicate::Missing { .. } => Err(SqlBuilderError::NotImplemented(
            "composite :missing requires a materialized composite strategy".to_string(),
        )),
    }
}

fn render_composite_clause_jsonb_fallback_expr(
    builder: &mut SqlBuilder,
    clause: &CompositeClause,
) -> Result<SqlExpr, SqlBuilderError> {
    match &clause.predicate {
        CompositePredicate::Tuple { components, safety } => {
            if matches!(safety, CompositeSafety::RequiresSameElement) {
                return render_composite_same_component_element_expr(builder, components);
            }

            let conditions = components
                .iter()
                .map(|component| render_composite_component_expr(builder, component))
                .collect::<Result<Vec<_>, _>>()?;
            if conditions.is_empty() {
                Ok(SqlExpr::Bool(true))
            } else {
                Ok(SqlExpr::And(conditions))
            }
        }
        CompositePredicate::Missing { .. } => Err(SqlBuilderError::NotImplemented(
            "custom composite :missing JSONB fallback is not supported".to_string(),
        )),
    }
}

fn render_composite_component_native_expr(
    builder: &mut SqlBuilder,
    resource_type: &str,
    component: &CompositeComponentPredicate,
) -> Result<SqlExpr, SqlBuilderError> {
    let parsed = parsed_component_param(component);
    let sql = match component.spec.search_type {
        SearchParameterType::String => {
            let clauses = StringClause::from_parsed_param(&parsed, resource_type)?;
            render_string_clauses_as_or(builder, &clauses)
        }
        SearchParameterType::Date => {
            let clauses = DateClause::from_parsed_param(&parsed, resource_type)?;
            render_date_clauses_as_or(builder, &clauses)
        }
        SearchParameterType::Number => {
            let clauses = NumberClause::from_parsed_param(&parsed, resource_type)?;
            render_number_index_clauses_as_or(builder, &clauses)?
        }
        SearchParameterType::Quantity => {
            let clauses = QuantityClause::from_parsed_param(&parsed, resource_type)?;
            render_quantity_index_clauses_as_or(builder, &clauses)?
        }
        SearchParameterType::Reference => {
            let clauses = ReferenceClause::from_parsed_param(&parsed, resource_type, &[])?;
            render_reference_clauses_as_or(builder, &clauses, "")
        }
        SearchParameterType::Token => {
            let index_shape = token_index_shape_for_component(component);
            let clauses = TokenClause::from_parsed_param(&parsed, resource_type, index_shape)?;
            let path_segments =
                crate::sql_builder::fhirpath_to_jsonb_path(&component.spec.expression, "");
            match index_shape {
                TokenIndexShape::Identifier => {
                    let resource_col = builder.resource_column().to_string();
                    let array_path = crate::sql_builder::build_jsonb_accessor(
                        &resource_col,
                        &path_segments,
                        false,
                    );
                    render_token_identifier_containment_clauses_as_or(
                        builder,
                        &clauses,
                        &path_segments,
                        &array_path,
                    )?
                }
                TokenIndexShape::SimpleCode => {
                    render_token_simple_code_clauses_as_or(builder, &clauses, &path_segments)?
                }
                TokenIndexShape::Coding => {
                    render_token_coding_clauses_as_or(builder, &clauses, &path_segments)?
                }
            }
        }
        SearchParameterType::Uri => {
            let clauses = UriClause::from_parsed_param(&parsed, resource_type)?;
            let path_segments =
                crate::sql_builder::fhirpath_to_jsonb_path(&component.spec.expression, "");
            let path = crate::sql_builder::build_jsonb_accessor(
                builder.resource_column(),
                &path_segments,
                true,
            );
            render_uri_clauses_as_or(builder, &clauses, &path)
        }
        SearchParameterType::Composite | SearchParameterType::Special => {
            return Err(SqlBuilderError::NotImplemented(format!(
                "Composite component type '{}' not supported",
                crate::ir::search_type_name(component.spec.search_type)
            )));
        }
    };

    sql.map(SqlExpr::Raw).ok_or_else(|| {
        SqlBuilderError::InvalidSearchValue(format!(
            "Composite component '{}' produced no native predicate",
            component.spec.code
        ))
    })
}

fn parsed_component_param(component: &CompositeComponentPredicate) -> ParsedParam {
    let (prefix, raw) = extract_component_prefix(&component.value);
    ParsedParam {
        name: component.spec.code.clone(),
        modifier: None,
        values: vec![ParsedValue {
            prefix,
            raw: raw.to_string(),
        }],
    }
}

fn extract_component_prefix(value: &str) -> (Option<SearchPrefix>, &str) {
    let mut chars = value.chars();
    if let Some(c1) = chars.next()
        && let Some(c2) = chars.next()
        && c1.is_ascii_lowercase()
        && c2.is_ascii_lowercase()
    {
        let prefix_str: String = [c1, c2].iter().collect();
        if let Some(prefix) = SearchPrefix::parse(&prefix_str) {
            return (Some(prefix), &value[2..]);
        }
    }
    (None, value)
}

fn token_index_shape_for_component(component: &CompositeComponentPredicate) -> TokenIndexShape {
    if component.spec.element_type_hint.is_identifier() {
        TokenIndexShape::Identifier
    } else if matches!(
        &component.spec.element_type_hint,
        crate::parameters::ElementTypeHint::SimpleCode
    ) {
        TokenIndexShape::SimpleCode
    } else {
        TokenIndexShape::Coding
    }
}

fn render_composite_same_component_element_expr(
    builder: &mut SqlBuilder,
    components: &[CompositeComponentPredicate],
) -> Result<SqlExpr, SqlBuilderError> {
    let Some(suffixes) = components
        .iter()
        .map(|component| strip_component_suffix(&component.spec.expression))
        .collect::<Option<Vec<_>>>()
    else {
        let conditions = components
            .iter()
            .map(|component| render_composite_component_expr(builder, component))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(SqlExpr::And(conditions));
    };

    let conditions = components
        .iter()
        .zip(suffixes.iter())
        .map(|(component, suffix)| {
            let json_path =
                suffix_jsonb_path("component_elem", suffix, component_text_leaf(component));
            render_composite_component_at_path_expr(builder, component, &json_path)
        })
        .collect::<Result<Vec<_>, _>>()?;

    if conditions.is_empty() {
        return Ok(SqlExpr::Bool(true));
    }

    let component_path = format!("{}->'component'", builder.resource_column());
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

fn render_composite_component_expr(
    builder: &mut SqlBuilder,
    component: &CompositeComponentPredicate,
) -> Result<SqlExpr, SqlBuilderError> {
    let json_path = expression_to_jsonb_path(&component.spec.expression);
    render_composite_component_at_path_expr(builder, component, json_path.as_str())
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

fn strip_component_suffix(expression: &str) -> Option<String> {
    let path = expression
        .split_once('.')
        .map_or(expression, |(head, tail)| {
            if head
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
            {
                tail
            } else {
                expression
            }
        });
    path.strip_prefix("component.")
        .map(str::to_string)
        .or_else(|| {
            path.strip_prefix("Observation.component.")
                .map(str::to_string)
        })
}

fn suffix_jsonb_path(base: &str, suffix: &str, text_leaf: bool) -> String {
    let parts = suffix
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return base.to_string();
    }

    let mut acc = base.to_string();
    for (index, part) in parts.iter().enumerate() {
        let is_leaf = index == parts.len() - 1;
        let op = if is_leaf && text_leaf { "->>" } else { "->" };
        acc.push_str(&format!("{op}'{part}'"));
    }
    acc
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
            token_path_clause_expr(builder, clause, json_path).and_then(|maybe_expr| {
                maybe_expr.map_or_else(
                    || render_token_path_raw_clause(builder, clause, json_path).map(SqlExpr::Raw),
                    Ok,
                )
            })
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

fn expression_to_jsonb_path(expression: &str) -> String {
    let path = expression
        .find('.')
        .map_or(expression, |i| &expression[i + 1..]);
    let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();

    if parts.is_empty() {
        return "resource".to_string();
    }

    let mut acc = "resource".to_string();
    for (i, part) in parts.iter().enumerate() {
        let op = if i == parts.len() - 1 { "->>" } else { "->" };
        acc.push_str(&format!("{op}'{part}'"));
    }
    acc
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

fn render_string_array_clause(
    builder: &mut SqlBuilder,
    clause: &StringClause,
    array_path: &str,
    field_name: &str,
) -> String {
    render_sql_expr(&string_array_clause_expr(
        builder, clause, array_path, field_name,
    ))
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

fn render_string_path_clause(
    builder: &mut SqlBuilder,
    clause: &StringClause,
    jsonb_path: &str,
) -> String {
    render_sql_expr(&string_path_clause_expr(builder, clause, jsonb_path))
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

fn render_token_simple_code_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    path_segments: &[String],
) -> Result<String, SqlBuilderError> {
    let condition = token_simple_code_clause_expr(builder, clause, path_segments)?;
    Ok(render_sql_expr(&token_apply_negation(clause, condition)))
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

fn render_token_scalar_code_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    jsonb_path: &str,
) -> Result<String, SqlBuilderError> {
    let condition = token_scalar_code_clause_expr(builder, clause, jsonb_path)?;
    Ok(render_sql_expr(&token_apply_negation(clause, condition)))
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

fn render_number_index_clause(
    builder: &mut SqlBuilder,
    clause: &NumberClause,
) -> Result<String, SqlBuilderError> {
    match &clause.predicate {
        NumberPredicate::Missing { is_missing } => Ok(render_sql_expr(&number_index_exists_expr(
            builder,
            clause,
            Vec::new(),
            *is_missing,
        ))),
        NumberPredicate::Comparison { prefix, value } => {
            let number = RenderDecimalParts::parse(value)?;
            if matches!(prefix, SearchPrefix::Ne) {
                let predicates = numeric_eq_range_predicates(builder, "sin.value_num", &number);
                return Ok(render_sql_expr(&number_index_exists_expr(
                    builder, clause, predicates, true,
                )));
            }
            let predicates = numeric_sidecar_predicates(builder, "sin.value_num", *prefix, &number);
            Ok(render_sql_expr(&number_index_exists_expr(
                builder, clause, predicates, false,
            )))
        }
    }
}

fn number_index_exists_expr(
    builder: &mut SqlBuilder,
    clause: &NumberClause,
    mut predicates: Vec<SqlExpr>,
    invert: bool,
) -> SqlExpr {
    let rt_param = builder.add_text_param(&clause.resource_type);
    let pc_param = builder.add_text_param(&clause.param_code);
    predicates.insert(
        0,
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sin.param_code".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(pc_param),
        },
    );
    predicates.insert(
        0,
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sin.resource_id".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Ident(builder.id_column()),
        },
    );
    predicates.insert(
        0,
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sin.resource_type".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(rt_param),
        },
    );

    let exists = SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: "search_idx_number".to_string(),
            alias: Some("sin".to_string()),
        },
        where_clause: Some(SqlExpr::And(predicates)),
    }));

    if invert {
        SqlExpr::Not(Box::new(exists))
    } else {
        exists
    }
}

fn render_quantity_index_clause(
    builder: &mut SqlBuilder,
    clause: &QuantityClause,
) -> Result<String, SqlBuilderError> {
    match &clause.predicate {
        QuantityPredicate::Missing { is_missing } => Ok(render_sql_expr(
            &quantity_index_exists_expr(builder, clause, Vec::new(), *is_missing),
        )),
        QuantityPredicate::Comparison {
            prefix,
            value,
            system,
            code,
        } => {
            let number =
                RenderDecimalParts::parse(value).map_err(|_| invalid_quantity_number(value))?;
            let mut predicates = if matches!(prefix, SearchPrefix::Ne) {
                numeric_eq_range_predicates(builder, "siq.value_num", &number)
            } else {
                numeric_sidecar_predicates(builder, "siq.value_num", *prefix, &number)
            };
            if let Some(system) = system {
                let p = builder.add_text_param(system);
                predicates.push(SqlExpr::Compare {
                    lhs: SqlTerm::Ident("siq.system".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(p),
                });
            }
            if let Some(code) = code {
                let p = builder.add_text_param(code);
                predicates.push(SqlExpr::Or(vec![
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident("siq.code".to_string()),
                        op: SqlOp::Eq,
                        rhs: SqlTerm::Param(p),
                    },
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident("siq.unit".to_string()),
                        op: SqlOp::Eq,
                        rhs: SqlTerm::Param(p),
                    },
                ]));
            }
            Ok(render_sql_expr(&quantity_index_exists_expr(
                builder,
                clause,
                predicates,
                matches!(prefix, SearchPrefix::Ne),
            )))
        }
    }
}

fn quantity_index_exists_expr(
    builder: &mut SqlBuilder,
    clause: &QuantityClause,
    mut predicates: Vec<SqlExpr>,
    invert: bool,
) -> SqlExpr {
    let rt_param = builder.add_text_param(&clause.resource_type);
    let pc_param = builder.add_text_param(&clause.param_code);
    predicates.insert(
        0,
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("siq.param_code".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(pc_param),
        },
    );
    predicates.insert(
        0,
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("siq.resource_id".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Ident(builder.id_column()),
        },
    );
    predicates.insert(
        0,
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("siq.resource_type".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(rt_param),
        },
    );

    let exists = SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: "search_idx_quantity".to_string(),
            alias: Some("siq".to_string()),
        },
        where_clause: Some(SqlExpr::And(predicates)),
    }));

    if invert {
        SqlExpr::Not(Box::new(exists))
    } else {
        exists
    }
}

fn render_quantity_clause(
    builder: &mut SqlBuilder,
    clause: &QuantityClause,
    jsonb_path: &str,
    containment_path: Option<&[String]>,
) -> Result<String, SqlBuilderError> {
    match &clause.predicate {
        QuantityPredicate::Missing { is_missing } => Ok(render_sql_expr(&jsonb_presence_expr(
            jsonb_path,
            *is_missing,
        ))),
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
                return Ok(render_sql_expr(&num_condition));
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

            Ok(render_sql_expr(&SqlExpr::And(constraints)))
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

fn render_reference_clause(
    builder: &mut SqlBuilder,
    clause: &ReferenceClause,
    _jsonb_path: &str,
) -> String {
    match &clause.predicate {
        ReferencePredicate::Missing { is_missing } => {
            render_sql_expr(&reference_index_missing_expr(builder, clause, *is_missing))
        }
        predicate => render_reference_index_condition(builder, clause, predicate),
    }
}

fn reference_index_missing_expr(
    builder: &mut SqlBuilder,
    clause: &ReferenceClause,
    is_missing: bool,
) -> SqlExpr {
    let exists = reference_index_exists_expr(reference_base_predicates(builder, clause));
    if is_missing {
        SqlExpr::Not(Box::new(exists))
    } else {
        exists
    }
}

fn render_reference_index_condition(
    builder: &mut SqlBuilder,
    clause: &ReferenceClause,
    predicate: &ReferencePredicate,
) -> String {
    render_sql_expr(&reference_index_condition_expr(builder, clause, predicate))
}

fn reference_index_condition_expr(
    builder: &mut SqlBuilder,
    clause: &ReferenceClause,
    predicate: &ReferencePredicate,
) -> SqlExpr {
    let mut predicates = reference_base_predicates(builder, clause);

    match predicate {
        ReferencePredicate::Local {
            target_type,
            target_id,
        } => {
            predicates.push(SqlExpr::Compare {
                lhs: SqlTerm::Ident("sir.ref_kind".to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Integer(1),
            });
            if let Some(target_type) = target_type {
                let tt_param = builder.add_text_param(target_type);
                predicates.push(SqlExpr::Compare {
                    lhs: SqlTerm::Ident("sir.target_type".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(tt_param),
                });
            }
            let tid_param = builder.add_text_param(target_id);
            predicates.push(SqlExpr::Compare {
                lhs: SqlTerm::Ident("sir.target_id".to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(tid_param),
            });
            reference_index_exists_expr(predicates)
        }
        ReferencePredicate::External { url } => {
            let url_param = builder.add_text_param(url);
            predicates.push(SqlExpr::Or(vec![
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident("sir.external_url".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(url_param),
                },
                SqlExpr::Compare {
                    lhs: SqlTerm::Ident("sir.raw_reference".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(url_param),
                },
            ]));
            reference_index_exists_expr(predicates)
        }
        ReferencePredicate::Identifier {
            system,
            require_no_system,
            value,
        } => {
            predicates.push(SqlExpr::Compare {
                lhs: SqlTerm::Ident("sir.ref_kind".to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Integer(4),
            });
            let val_param = builder.add_text_param(value);
            if let Some(system) = system {
                let sys_param = builder.add_text_param(system);
                predicates.push(SqlExpr::Compare {
                    lhs: SqlTerm::Ident("sir.identifier_system".to_string()),
                    op: SqlOp::Eq,
                    rhs: SqlTerm::Param(sys_param),
                });
            } else if *require_no_system {
                predicates.push(SqlExpr::IsNull(SqlTerm::Ident(
                    "sir.identifier_system".to_string(),
                )));
            }
            predicates.push(SqlExpr::Compare {
                lhs: SqlTerm::Ident("sir.identifier_value".to_string()),
                op: SqlOp::Eq,
                rhs: SqlTerm::Param(val_param),
            });
            reference_index_exists_expr(predicates)
        }
        ReferencePredicate::Missing { .. } => unreachable!("handled by caller"),
    }
}

fn reference_base_predicates(builder: &mut SqlBuilder, clause: &ReferenceClause) -> Vec<SqlExpr> {
    let rt_param = builder.add_text_param(&clause.resource_type);
    let pc_param = builder.add_text_param(&clause.param_code);
    let id_col = builder.id_column();
    vec![
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sir.resource_type".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(rt_param),
        },
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sir.resource_id".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Ident(id_col),
        },
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sir.param_code".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(pc_param),
        },
    ]
}

fn reference_index_exists_expr(predicates: Vec<SqlExpr>) -> SqlExpr {
    SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: "search_idx_reference".to_string(),
            alias: Some("sir".to_string()),
        },
        where_clause: Some(SqlExpr::And(predicates)),
    }))
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

fn render_string_clause(builder: &mut SqlBuilder, clause: &StringClause) -> String {
    if let StringPredicate::Text { value } = &clause.predicate {
        let resource_col = builder.resource_column().to_string();
        let p = builder.add_text_param(value);
        return format!(
            "to_tsvector('english', {resource_col}->>'text') @@ plainto_tsquery('english', ${p})"
        );
    }

    render_sql_expr(&string_sidecar_clause_expr(builder, clause))
}

fn string_sidecar_clause_expr(builder: &mut SqlBuilder, clause: &StringClause) -> SqlExpr {
    let rt_param = builder.add_text_param(&clause.resource_type);
    let pc_param = builder.add_text_param(&clause.param_code);
    let id_col = builder.id_column();
    let mut predicates = vec![
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sid.resource_type".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(rt_param),
        },
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sid.resource_id".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Ident(id_col),
        },
        SqlExpr::Compare {
            lhs: SqlTerm::Ident("sid.param_code".to_string()),
            op: SqlOp::Eq,
            rhs: SqlTerm::Param(pc_param),
        },
    ];

    match &clause.predicate {
        StringPredicate::Missing { is_missing } => {
            let exists = string_sidecar_exists_expr(predicates);
            if *is_missing {
                SqlExpr::Not(Box::new(exists))
            } else {
                exists
            }
        }
        predicate => {
            predicates.push(match predicate {
                StringPredicate::Prefix { value } => {
                    let normalized = normalize_string(value);
                    let pattern = format!("{}%", escape_like_pattern(&normalized));
                    let p = builder.add_text_param(pattern);
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident("sid.value_norm".to_string()),
                        op: SqlOp::Like,
                        rhs: SqlTerm::Param(p),
                    }
                }
                StringPredicate::Contains { value } => {
                    let normalized = normalize_string(value);
                    let pattern = format!("%{}%", escape_like_pattern(&normalized));
                    let p = builder.add_text_param(pattern);
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident("sid.value_norm".to_string()),
                        op: SqlOp::Like,
                        rhs: SqlTerm::Param(p),
                    }
                }
                StringPredicate::Exact { value } => {
                    let p = builder.add_text_param(value);
                    SqlExpr::Compare {
                        lhs: SqlTerm::Ident("sid.value_exact".to_string()),
                        op: SqlOp::Eq,
                        rhs: SqlTerm::Param(p),
                    }
                }
                StringPredicate::Text { .. } => unreachable!("handled above"),
                StringPredicate::Missing { .. } => unreachable!("handled above"),
            });
            string_sidecar_exists_expr(predicates)
        }
    }
}

fn string_sidecar_exists_expr(predicates: Vec<SqlExpr>) -> SqlExpr {
    SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: "search_idx_string".to_string(),
            alias: Some("sid".to_string()),
        },
        where_clause: Some(SqlExpr::And(predicates)),
    }))
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

fn render_numeric_comparison(
    builder: &mut SqlBuilder,
    path: &str,
    prefix: SearchPrefix,
    number: &RenderDecimalParts,
) -> String {
    render_sql_expr(&numeric_comparison_expr(builder, path, prefix, number))
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

fn numeric_sidecar_predicates(
    builder: &mut SqlBuilder,
    path: &str,
    prefix: SearchPrefix,
    number: &RenderDecimalParts,
) -> Vec<SqlExpr> {
    match prefix {
        SearchPrefix::Eq | SearchPrefix::Ne => numeric_eq_range_predicates(builder, path, number),
        SearchPrefix::Gt | SearchPrefix::Sa => {
            let p = bind_numeric(builder, number.format());
            vec![numeric_compare_expr(path, SqlOp::Gt, p)]
        }
        SearchPrefix::Lt | SearchPrefix::Eb => {
            let p = bind_numeric(builder, number.format());
            vec![numeric_compare_expr(path, SqlOp::Lt, p)]
        }
        SearchPrefix::Ge => {
            let p = bind_numeric(builder, number.format());
            vec![numeric_compare_expr(path, SqlOp::Ge, p)]
        }
        SearchPrefix::Le => {
            let p = bind_numeric(builder, number.format());
            vec![numeric_compare_expr(path, SqlOp::Le, p)]
        }
        SearchPrefix::Ap => {
            let (lower, upper) = number.approximate_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            vec![
                numeric_compare_expr(path, SqlOp::Ge, p1),
                numeric_compare_expr(path, SqlOp::Lt, p2),
            ]
        }
    }
}

fn numeric_eq_range_predicates(
    builder: &mut SqlBuilder,
    path: &str,
    number: &RenderDecimalParts,
) -> Vec<SqlExpr> {
    let (lower, upper) = number.implicit_eq_bounds();
    let p1 = bind_numeric(builder, lower);
    let p2 = bind_numeric(builder, upper);
    vec![
        numeric_compare_expr(path, SqlOp::Ge, p1),
        numeric_compare_expr(path, SqlOp::Lt, p2),
    ]
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
                table: "search_idx_date".to_string(),
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
            "EXISTS (SELECT 1 FROM search_idx_date sid WHERE (sid.resource_id = r.id AND sid.rng && tstzrange($1::timestamptz, NULL, '[)')))"
        );
    }

    #[test]
    fn sql_ast_renders_not_exists_without_wrapping_exists_as_boolean_expr() {
        let expr = SqlExpr::Not(Box::new(SqlExpr::Exists(Box::new(SelectStmt {
            projection: vec![SqlTerm::Integer(1)],
            from: SqlFrom {
                table: "search_idx_date".to_string(),
                alias: Some("sid".to_string()),
            },
            where_clause: Some(SqlExpr::Bool(true)),
        }))));

        assert_eq!(
            render_sql_expr(&expr),
            "NOT EXISTS (SELECT 1 FROM search_idx_date sid WHERE TRUE)"
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

        let sql = render_date_column_clauses_as_or(&mut builder, &clauses, "r.updated_at").unwrap();

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

        let sql = render_id_clauses_as_or(&mut builder, &clauses, "r.id").unwrap();

        assert_eq!(sql, "(r.id = $1) = false");
        assert!(!sql.contains("NOT ("));
        assert_eq!(builder.params()[0].as_str(), "pat-1");
    }

    #[test]
    fn string_sidecar_render_redacts_values_into_params() {
        let mut builder = SqlBuilder::new();
        let clauses = vec![StringClause {
            resource_type: "Patient".to_string(),
            param_code: "family".to_string(),
            predicate: StringPredicate::Contains {
                value: "Sm_th%".to_string(),
            },
        }];

        let sql = render_string_clauses_as_or(&mut builder, &clauses).unwrap();

        assert!(sql.contains("search_idx_string"));
        assert!(sql.contains("sid.value_norm LIKE $3"));
        assert!(!sql.contains("Sm_th"));
        assert_eq!(builder.params()[2].as_str(), "%sm\\_th\\%%");
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

        let sql =
            render_string_path_clauses_as_or(&mut builder, &clauses, "resource->>'name'").unwrap();

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

        let sql =
            render_string_array_clauses_as_or(&mut builder, &clauses, "resource->'name'", "given")
                .unwrap();

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

        let sql =
            render_string_human_name_clauses_as_or(&mut builder, &clauses, "resource->'name'")
                .unwrap();

        assert!(sql.contains("jsonb_array_elements(resource->'name')"));
        assert!(sql.contains("name->>'family'"));
        assert!(sql.contains("name->>'text'"));
        assert!(sql.contains("jsonb_array_elements_text"));
        assert!(sql.contains("jsonb_typeof(name->'given') = 'array'"));
        assert!(!sql.contains("COALESCE"));
        assert_eq!(builder.params()[0].as_str(), "smith%");
    }

    #[test]
    fn reference_render_uses_sidecar_without_jsonb_fallback() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = ReferenceClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "subject".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "Patient/pat-123".to_string(),
                }],
            },
            "Observation",
            &["Patient".to_string(), "Group".to_string()],
        )
        .unwrap();

        let sql = render_reference_clauses_as_or(&mut builder, &clauses, "r.resource->'subject'")
            .unwrap();

        assert!(sql.contains("search_idx_reference"));
        assert!(sql.contains("sir.ref_kind = 1"));
        assert!(sql.contains("sir.target_type = $3"));
        assert!(sql.contains("sir.target_id = $4"));
        assert!(!sql.contains(" OR "));
        assert!(!sql.contains("r.resource->'subject'->>'reference'"));
        assert!(!sql.contains("pat-123") && !sql.contains("Patient/pat-123"));
        assert_eq!(builder.params()[2].as_str(), "Patient");
        assert_eq!(builder.params()[3].as_str(), "pat-123");
        assert_eq!(builder.params().len(), 4);
    }

    #[test]
    fn reference_render_preserves_identifier_no_system_semantics() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = ReferenceClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "subject".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Identifier),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "|abc".to_string(),
                }],
            },
            "Observation",
            &["Patient".to_string()],
        )
        .unwrap();

        let sql = render_reference_clauses_as_or(&mut builder, &clauses, "r.resource->'subject'")
            .unwrap();

        assert!(sql.contains("sir.ref_kind = 4"));
        assert!(sql.contains("sir.identifier_system IS NULL"));
        assert!(sql.contains("sir.identifier_value = $3"));
        assert!(!sql.contains(" OR "));
        assert!(!sql.contains("abc"));
        assert_eq!(builder.params()[2].as_str(), "abc");
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

        let sql = render_number_clauses_as_or(&mut builder, &clauses, "resource->>'value'")
            .unwrap()
            .unwrap();

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

        let sql =
            render_quantity_clauses_as_or(&mut builder, &clauses, "resource->'valueQuantity'")
                .unwrap()
                .unwrap();

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

        let sql = render_quantity_containment_clauses_as_or(
            &mut builder,
            &clauses,
            "r.resource->'valueQuantity'",
            &["valueQuantity".to_string()],
        )
        .unwrap()
        .unwrap();

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
    fn composite_component_tuple_renders_same_element_exists() {
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

        let sql = render_composite_clauses_as_jsonb_fallback_or(&mut builder, &clauses)
            .unwrap()
            .unwrap();

        assert!(sql.contains("jsonb_array_elements"));
        assert!(sql.contains("component_elem"));
        assert!(sql.contains("component_elem->'code'->'coding' @>"));
        assert!(sql.contains("component_elem->'valueQuantity'->>'value'"));
        assert!(!sql.contains("resource->'component'->'code'"));
        assert!(!sql.contains("loinc") && !sql.contains("8480-6") && !sql.contains("mm[Hg]"));
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

        let sql =
            render_token_simple_code_clauses_as_or(&mut builder, &clauses, &["gender".to_string()])
                .unwrap()
                .unwrap();

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

        let sql =
            render_token_simple_code_clauses_as_or(&mut builder, &clauses, &["gender".to_string()])
                .unwrap()
                .unwrap();

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

        let sql =
            render_token_simple_code_clauses_as_or(&mut builder, &clauses, &["gender".to_string()])
                .unwrap()
                .unwrap();

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

        let sql =
            render_token_simple_code_clauses_as_or(&mut builder, &clauses, &["gender".to_string()])
                .unwrap()
                .unwrap();

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

        let sql =
            render_token_scalar_code_clauses_as_or(&mut builder, &clauses, "resource->>'gender'")
                .unwrap()
                .unwrap();

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

        let sql =
            render_token_scalar_code_clauses_as_or(&mut builder, &clauses, "resource->>'gender'")
                .unwrap()
                .unwrap();

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

        let sql =
            render_token_scalar_code_clauses_as_or(&mut builder, &clauses, "resource->>'gender'")
                .unwrap()
                .unwrap();

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

        let sql = render_token_coding_clauses_as_or(&mut builder, &clauses, &["code".to_string()])
            .unwrap()
            .unwrap();

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

        let sql = render_token_coding_clauses_as_or(&mut builder, &clauses, &["code".to_string()])
            .unwrap()
            .unwrap();

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

        let sql = render_token_identifier_clauses_as_or(
            &mut builder,
            &clauses,
            "r.resource->'identifier'",
        )
        .unwrap()
        .unwrap();

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

        let sql = render_token_identifier_clauses_as_or(
            &mut builder,
            &clauses,
            "r.resource->'identifier'",
        )
        .unwrap()
        .unwrap();

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

        let sql = render_token_identifier_containment_clauses_as_or(
            &mut builder,
            &clauses,
            &["identifier".to_string()],
            "r.resource->'identifier'",
        )
        .unwrap()
        .unwrap();

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

        let sql = render_uri_clauses_as_or(&mut builder, &clauses, "resource->>'url'").unwrap();

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

        let sql =
            render_uri_array_clauses_as_or(&mut builder, &clauses, "resource->'meta'->'profile'")
                .unwrap();

        assert!(sql.contains("jsonb_array_elements_text(resource->'meta'->'profile')"));
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

        let sql = render_token_path_clauses_as_or(&mut builder, &clauses, "resource->'status'")
            .unwrap()
            .unwrap();

        assert!(sql.starts_with("("));
        assert!(sql.ends_with("= false"));
        assert!(!sql.contains("NOT ("));
    }
}
