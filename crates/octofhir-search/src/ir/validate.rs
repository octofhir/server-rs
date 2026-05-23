use crate::ir::ast::{SearchExpr, SearchValue};
use crate::ir::strategy::IndexStrategy;
use crate::parameters::SearchParameterType;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("empty boolean expression")]
    EmptyBooleanExpr,
    #[error("search parameter '{code}' has no values")]
    EmptyParamValues { code: String },
    #[error("search parameter '{code}' value type does not match declared type {search_type:?}")]
    TypeMismatch {
        code: String,
        search_type: SearchParameterType,
    },
    #[error("search parameter '{code}' uses disabled strategy")]
    DisabledStrategy { code: String },
}

pub fn validate_search_expr(expr: &SearchExpr) -> Result<(), ValidationError> {
    match expr {
        SearchExpr::And(children) | SearchExpr::Or(children) => {
            if children.is_empty() {
                return Err(ValidationError::EmptyBooleanExpr);
            }
            for child in children {
                validate_search_expr(child)?;
            }
        }
        SearchExpr::Not(inner) => validate_search_expr(inner)?,
        SearchExpr::Param(param) => {
            if param.values.is_empty() {
                return Err(ValidationError::EmptyParamValues {
                    code: param.code.clone(),
                });
            }
            if matches!(param.strategy_hint, Some(IndexStrategy::Disabled)) {
                return Err(ValidationError::DisabledStrategy {
                    code: param.code.clone(),
                });
            }
            for value in &param.values {
                let matches_type = matches!(
                    (param.search_type, value),
                    (SearchParameterType::Date, SearchValue::Date(_))
                        | (SearchParameterType::String, SearchValue::String(_))
                        | (SearchParameterType::Token, SearchValue::Token(_))
                        | (SearchParameterType::Number, SearchValue::Number(_))
                        | (SearchParameterType::Quantity, SearchValue::Quantity(_))
                        | (SearchParameterType::Uri, SearchValue::Uri(_))
                        | (SearchParameterType::Composite, SearchValue::Composite(_))
                );
                if !matches_type {
                    return Err(ValidationError::TypeMismatch {
                        code: param.code.clone(),
                        search_type: param.search_type,
                    });
                }
            }
        }
    }
    Ok(())
}
