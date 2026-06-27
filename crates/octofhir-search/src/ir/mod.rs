//! FHIR-native search intermediate representation.
//!
//! This module is intentionally small. It models FHIR search semantics and
//! sidecar strategy choices before SQL text is rendered, without becoming a
//! generic SQL DSL.

pub mod ast;
pub mod chain;
pub mod debug;
pub mod registry;
pub mod render;
pub mod rewrite;
pub mod sql;
pub mod strategy;
pub mod validate;

pub use ast::{
    CompositeClause, CompositeComponentPredicate, CompositeComponentSpec, CompositePredicate,
    CompositeSafety, DateParamExpr, IdClause, IdPredicate, NumberClause, NumberPredicate,
    QuantityClause, QuantityPredicate, ReferenceClause, ReferencePredicate, SearchExpr,
    SearchParamExpr, SearchValue, StringClause, StringPredicate, TokenClause, TokenIndexShape,
    TokenPredicate, TokenSetModifier, UriClause, UriPredicate,
};
pub use chain::{
    ChainClause, ChainError, ChainLink, HasClause, HasTail, is_chained_parameter,
    is_reverse_chain_parameter, render_chain_clause, render_has_clause,
};
pub use debug::{
    DebugPredicate, SearchDebugPlan, build_composite_debug_plan, build_date_debug_plan,
    build_number_debug_plan, build_quantity_debug_plan, build_reference_debug_plan,
    build_string_debug_plan, build_string_text_debug_predicate, build_token_debug_plan,
};
pub use registry::{
    ResourceColumnParam, resolve_component_definition, resolve_composite_component_specs,
    resolve_resource_column_param, search_type_name,
};
pub use render::{
    render_composite_clauses_as_or, render_date_column_clauses_as_or,
    render_date_inplace_clauses_as_or, render_date_text_path_clauses_as_or,
    render_id_clauses_as_or, render_indexed_string_clauses_as_or, render_number_clauses_as_or,
    render_period_path_clauses_as_or, render_quantity_array_clauses_as_or,
    render_quantity_clauses_as_or, render_quantity_containment_clauses_as_or,
    render_quantity_union_clauses_as_or, render_sql_expr, render_string_array_clauses_as_or,
    render_string_human_name_clauses_as_or, render_string_path_clauses_as_or,
    render_token_coding_array_clauses_as_or, render_token_coding_clauses_as_or,
    render_token_coding_subtree_clauses_as_or, render_token_identifier_clauses_as_or,
    render_token_identifier_containment_clauses_as_or, render_token_path_clauses_as_or,
    render_token_scalar_code_clauses_as_or, render_token_simple_code_clauses_as_or,
    render_uri_array_clauses_as_or, render_uri_clauses_as_or,
};
pub use rewrite::{rewrite_date_clauses, rewrite_search_expr};
pub use strategy::{IndexStrategy, StrategyDecision};
pub use validate::{ValidationError, validate_search_expr};
