//! FHIR-native search intermediate representation.
//!
//! This module is intentionally small. It models FHIR search semantics and
//! sidecar strategy choices before SQL text is rendered, without becoming a
//! generic SQL DSL.

pub mod ast;
pub mod debug;
pub mod registry;
pub mod render;
pub mod rewrite;
pub mod sql;
pub mod strategy;
pub mod validate;

pub use ast::{
    CompositeClause, CompositeComponentPredicate, CompositeComponentSpec, CompositePredicate,
    CompositeSafety, DateParamExpr, NumberClause, NumberPredicate, QuantityClause,
    QuantityPredicate, ReferenceClause, ReferencePredicate, SearchExpr, SearchParamExpr,
    SearchValue, StringClause, StringPredicate, TokenClause, TokenIndexShape, TokenPredicate,
    TokenSetModifier,
};
pub use debug::{
    DebugPredicate, SearchDebugPlan, build_composite_debug_plan, build_date_debug_plan,
    build_number_debug_plan, build_quantity_debug_plan, build_reference_debug_plan,
    build_string_debug_plan, build_string_text_debug_predicate, build_token_debug_plan,
};
pub use registry::{
    resolve_component_definition, resolve_composite_component_specs, search_type_name,
};
pub use render::{render_date_clauses_as_or, render_sql_expr, render_string_clauses_as_or};
pub use rewrite::{rewrite_date_clauses, rewrite_search_expr};
pub use strategy::{IndexStrategy, StrategyDecision};
pub use validate::{ValidationError, validate_search_expr};
