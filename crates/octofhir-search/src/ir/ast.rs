use crate::ir::strategy::IndexStrategy;
use crate::parameters::{SearchModifier, SearchParameterType};
use crate::types::date_ast::{DateClause, DatePredicate};

/// Top-level FHIR search expression.
///
/// Repeated query parameters become `And`; comma-separated values become `Or`.
#[derive(Debug, Clone)]
pub enum SearchExpr {
    And(Vec<SearchExpr>),
    Or(Vec<SearchExpr>),
    Not(Box<SearchExpr>),
    Param(SearchParamExpr),
}

/// SearchParameter expression after registry lookup and type parsing.
#[derive(Debug, Clone)]
pub struct SearchParamExpr {
    pub resource_type: String,
    pub code: String,
    pub search_type: SearchParameterType,
    pub modifier: Option<SearchModifier>,
    pub values: Vec<SearchValue>,
    pub expression: Option<String>,
    pub strategy_hint: Option<IndexStrategy>,
}

/// Type-specific predicate payload.
#[derive(Debug, Clone)]
pub enum SearchValue {
    Date(DatePredicate),
}

/// Date SearchParameter occurrence.
///
/// `clauses` are OR-combined because they come from one comma-separated query
/// occurrence. Repeated occurrences are represented by multiple `DateParamExpr`
/// values under a parent `SearchExpr::And`.
#[derive(Debug, Clone)]
pub struct DateParamExpr {
    pub clauses: Vec<DateClause>,
}

impl DateParamExpr {
    pub fn new(clauses: Vec<DateClause>) -> Self {
        Self { clauses }
    }
}
