//! FHIR _filter parameter parser and SQL generator.
//!
//! Implements the FHIR R5 _filter parameter syntax for advanced search filtering.
//! See: https://www.hl7.org/fhir/search_filter.html
//!
//! # Syntax
//!
//! ```text
//! filter        = paramExp / logExp / ("(" filter ")")
//! logExp        = filter ("and" / "or") filter
//! paramExp      = paramPath SP compareOp SP compValue
//! paramPath     = paramName ["." paramPath]
//! compareOp     = "eq" / "ne" / "co" / "sw" / "ew" / "gt" / "lt" / "ge" / "le" / "sa" / "eb" / "ap"
//! compValue     = string / token / number / date
//! ```
//!
//! # Examples
//!
//! ```text
//! name eq "Smith"
//! birthdate ge 1990-01-01
//! status ne "cancelled" or priority eq "urgent"
//! not (status eq "draft")
//! subject.name co "john"
//! ```

use crate::ir::search_type_name;
use crate::ir::sql::SqlExpr;
use crate::parameters::{SearchModifier, SearchParameterType, SearchPrefix};
use crate::parser::{ParsedParam, ParsedValue};
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::types::dispatch_search_with_registry;

/// Comparison operators supported in _filter expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    /// Equal
    Eq,
    /// Not equal
    Ne,
    /// Contains
    Co,
    /// Starts with
    Sw,
    /// Ends with
    Ew,
    /// Greater than
    Gt,
    /// Less than
    Lt,
    /// Greater than or equal
    Ge,
    /// Less than or equal
    Le,
    /// Starts after (for date ranges)
    Sa,
    /// Ends before (for date ranges)
    Eb,
    /// Approximately (for numeric/date values)
    Ap,
}

impl FilterOp {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "eq" => Some(Self::Eq),
            "ne" => Some(Self::Ne),
            "co" => Some(Self::Co),
            "sw" => Some(Self::Sw),
            "ew" => Some(Self::Ew),
            "gt" => Some(Self::Gt),
            "lt" => Some(Self::Lt),
            "ge" => Some(Self::Ge),
            "le" => Some(Self::Le),
            "sa" => Some(Self::Sa),
            "eb" => Some(Self::Eb),
            "ap" => Some(Self::Ap),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::Co => "co",
            Self::Sw => "sw",
            Self::Ew => "ew",
            Self::Gt => "gt",
            Self::Lt => "lt",
            Self::Ge => "ge",
            Self::Le => "le",
            Self::Sa => "sa",
            Self::Eb => "eb",
            Self::Ap => "ap",
        }
    }
}

/// Logical operators for combining filter expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
}

/// A parsed filter expression.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterExpression {
    /// A comparison: path op value
    Comparison {
        path: String,
        op: FilterOp,
        value: String,
    },
    /// Logical AND or OR of two expressions
    Logical {
        op: LogicalOp,
        left: Box<FilterExpression>,
        right: Box<FilterExpression>,
    },
    /// Negation of an expression
    Not(Box<FilterExpression>),
}

/// Tokenizer for filter expressions.
struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Identifier(String),
    QuotedString(String),
    Number(String),
    OpenParen,
    CloseParen,
    And,
    Or,
    Not,
    Operator(FilterOp),
    Eof,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            let ch = self.input.as_bytes()[self.pos] as char;
            if ch.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        if self.pos < self.input.len() {
            Some(self.input.as_bytes()[self.pos] as char)
        } else {
            None
        }
    }

    fn next_token(&mut self) -> Result<Token, SqlBuilderError> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Ok(Token::Eof);
        }

        let ch = self.peek_char().unwrap();

        // Handle parentheses
        if ch == '(' {
            self.pos += 1;
            return Ok(Token::OpenParen);
        }
        if ch == ')' {
            self.pos += 1;
            return Ok(Token::CloseParen);
        }

        // Handle quoted strings
        if ch == '"' || ch == '\'' {
            let quote = ch;
            self.pos += 1;
            let start = self.pos;
            while self.pos < self.input.len() {
                let c = self.input.as_bytes()[self.pos] as char;
                if c == quote {
                    let value = &self.input[start..self.pos];
                    self.pos += 1;
                    return Ok(Token::QuotedString(value.to_string()));
                }
                self.pos += 1;
            }
            return Err(SqlBuilderError::InvalidSearchValue(
                "Unterminated quoted string in _filter".to_string(),
            ));
        }

        // Handle identifiers, operators, and keywords
        let start = self.pos;
        while self.pos < self.input.len() {
            let c = self.input.as_bytes()[self.pos] as char;
            if c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || c == ':' {
                self.pos += 1;
            } else {
                break;
            }
        }

        if self.pos == start {
            return Err(SqlBuilderError::InvalidSearchValue(format!(
                "Unexpected character '{}' in _filter",
                ch
            )));
        }

        let word = &self.input[start..self.pos];

        // Check for keywords
        match word.to_lowercase().as_str() {
            "and" => Ok(Token::And),
            "or" => Ok(Token::Or),
            "not" => Ok(Token::Not),
            _ => {
                // Check if it's an operator
                if let Some(op) = FilterOp::from_str(word) {
                    Ok(Token::Operator(op))
                } else if word
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
                {
                    Ok(Token::Number(word.to_string()))
                } else {
                    Ok(Token::Identifier(word.to_string()))
                }
            }
        }
    }
}

/// Parser for filter expressions.
struct Parser<'a> {
    tokenizer: Tokenizer<'a>,
    current: Token,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Result<Self, SqlBuilderError> {
        let mut tokenizer = Tokenizer::new(input);
        let current = tokenizer.next_token()?;
        Ok(Self { tokenizer, current })
    }

    fn advance(&mut self) -> Result<(), SqlBuilderError> {
        self.current = self.tokenizer.next_token()?;
        Ok(())
    }

    /// Parse a full filter expression.
    fn parse(&mut self) -> Result<FilterExpression, SqlBuilderError> {
        self.parse_or()
    }

    /// Parse OR expressions (lowest precedence).
    fn parse_or(&mut self) -> Result<FilterExpression, SqlBuilderError> {
        let mut left = self.parse_and()?;

        while self.current == Token::Or {
            self.advance()?;
            let right = self.parse_and()?;
            left = FilterExpression::Logical {
                op: LogicalOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parse AND expressions.
    fn parse_and(&mut self) -> Result<FilterExpression, SqlBuilderError> {
        let mut left = self.parse_not()?;

        while self.current == Token::And {
            self.advance()?;
            let right = self.parse_not()?;
            left = FilterExpression::Logical {
                op: LogicalOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parse NOT expressions.
    fn parse_not(&mut self) -> Result<FilterExpression, SqlBuilderError> {
        if self.current == Token::Not {
            self.advance()?;
            let expr = self.parse_primary()?;
            return Ok(FilterExpression::Not(Box::new(expr)));
        }
        self.parse_primary()
    }

    /// Parse primary expressions (comparisons and parenthesized groups).
    fn parse_primary(&mut self) -> Result<FilterExpression, SqlBuilderError> {
        // Handle parenthesized expressions
        if self.current == Token::OpenParen {
            self.advance()?;
            let expr = self.parse()?;
            if self.current != Token::CloseParen {
                return Err(SqlBuilderError::InvalidSearchValue(
                    "Expected ')' in _filter expression".to_string(),
                ));
            }
            self.advance()?;
            return Ok(expr);
        }

        // Parse comparison: path op value
        let path = match &self.current {
            Token::Identifier(s) => s.clone(),
            _ => {
                return Err(SqlBuilderError::InvalidSearchValue(
                    "Expected identifier in _filter expression".to_string(),
                ));
            }
        };
        self.advance()?;

        let op = match &self.current {
            Token::Operator(op) => *op,
            _ => {
                return Err(SqlBuilderError::InvalidSearchValue(
                    "Expected comparison operator in _filter expression".to_string(),
                ));
            }
        };
        self.advance()?;

        let value = match &self.current {
            Token::QuotedString(s) => s.clone(),
            Token::Identifier(s) => s.clone(),
            Token::Number(s) => s.clone(),
            _ => {
                return Err(SqlBuilderError::InvalidSearchValue(
                    "Expected value in _filter expression".to_string(),
                ));
            }
        };
        self.advance()?;

        Ok(FilterExpression::Comparison { path, op, value })
    }
}

/// Parse a _filter expression string.
pub fn parse_filter(input: &str) -> Result<FilterExpression, SqlBuilderError> {
    let mut parser = Parser::new(input)?;
    let expr = parser.parse()?;

    if parser.current != Token::Eof {
        return Err(SqlBuilderError::InvalidSearchValue(
            "Unexpected tokens after _filter expression".to_string(),
        ));
    }

    Ok(expr)
}

/// The `(modifier, prefix)` that make the ordinary search parameter behave like
/// `op`, or None when `op` has no counterpart for that parameter type.
fn comparison_to_search(
    param_type: SearchParameterType,
    op: FilterOp,
) -> Option<(Option<SearchModifier>, Option<SearchPrefix>)> {
    use SearchParameterType as T;
    let prefix = |p| Some((None, Some(p)));
    match (param_type, op) {
        (T::String, FilterOp::Eq) => Some((Some(SearchModifier::Exact), None)),
        (T::String, FilterOp::Co) => Some((Some(SearchModifier::Contains), None)),
        (T::String, FilterOp::Sw) => Some((None, None)),
        (T::Token | T::Uri | T::Reference, FilterOp::Eq) => Some((None, None)),
        (T::Token, FilterOp::Ne) => Some((Some(SearchModifier::Not), None)),
        (T::Date | T::Number | T::Quantity, op) => match op {
            FilterOp::Eq => prefix(SearchPrefix::Eq),
            FilterOp::Ne => prefix(SearchPrefix::Ne),
            FilterOp::Gt => prefix(SearchPrefix::Gt),
            FilterOp::Lt => prefix(SearchPrefix::Lt),
            FilterOp::Ge => prefix(SearchPrefix::Ge),
            FilterOp::Le => prefix(SearchPrefix::Le),
            FilterOp::Sa => prefix(SearchPrefix::Sa),
            FilterOp::Eb => prefix(SearchPrefix::Eb),
            FilterOp::Ap => prefix(SearchPrefix::Ap),
            FilterOp::Co | FilterOp::Sw | FilterOp::Ew => None,
        },
        _ => None,
    }
}

/// Escape the FHIR search value delimiters, so a `_filter` string value is
/// matched literally rather than split into an OR list or a token pair.
fn escape_search_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\\' | ',' | '$' | '|') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Render one `_filter` expression as a SQL condition. Each comparison is the
/// ordinary search parameter it names, with the operator mapped onto that
/// parameter's modifier or prefix, so it shares the search's semantics and
/// indexes; `and`/`or`/`not` combine the rendered conditions.
fn build_condition(
    builder: &mut SqlBuilder,
    expr: &FilterExpression,
    resource_type: &str,
    registry: &SearchParameterRegistry,
) -> Result<SqlExpr, SqlBuilderError> {
    match expr {
        FilterExpression::Comparison { path, op, value } => {
            if path.contains('.') {
                return Err(SqlBuilderError::NotImplemented(format!(
                    "_filter: chained parameter '{path}' is not supported"
                )));
            }
            let definition = registry.get(resource_type, path).ok_or_else(|| {
                SqlBuilderError::InvalidSearchValue(format!(
                    "_filter: unknown search parameter '{path}' for {resource_type}"
                ))
            })?;
            let (modifier, prefix) =
                comparison_to_search(definition.param_type, *op).ok_or_else(|| {
                    SqlBuilderError::NotImplemented(format!(
                        "_filter: operator '{}' is not supported for {} parameter '{path}'",
                        op.as_str(),
                        search_type_name(definition.param_type),
                    ))
                })?;
            let raw = match definition.param_type {
                SearchParameterType::String => escape_search_value(value),
                _ => value.clone(),
            };
            let param = ParsedParam {
                name: path.clone(),
                modifier,
                values: vec![ParsedValue { prefix, raw }],
            };
            let first = builder.condition_count();
            dispatch_search_with_registry(builder, &param, &definition, resource_type, registry)?;
            let rendered = builder.take_conditions_from(first);
            Ok(match rendered.len() {
                1 => rendered.into_iter().next().unwrap(),
                _ => SqlExpr::And(rendered),
            })
        }
        FilterExpression::Logical { op, left, right } => {
            let left = build_condition(builder, left, resource_type, registry)?;
            let right = build_condition(builder, right, resource_type, registry)?;
            Ok(match op {
                LogicalOp::And => SqlExpr::And(vec![left, right]),
                LogicalOp::Or => SqlExpr::Or(vec![left, right]),
            })
        }
        FilterExpression::Not(inner) => Ok(SqlExpr::Not(Box::new(build_condition(
            builder,
            inner,
            resource_type,
            registry,
        )?))),
    }
}

/// Add the SQL condition of a `_filter` value to `builder`. Comparisons name
/// search parameters of `resource_type` (see [`build_condition`]); an unknown
/// parameter, a chained path or an operator the parameter type cannot express
/// is an error rather than a filter that silently matches nothing.
///
/// ```ignore
/// build_filter_sql(builder, "name co \"smi\" and birthdate ge 1990", "Patient", registry)?;
/// build_filter_sql(builder, "not (status eq draft)", "Task", registry)?;
/// ```
pub fn build_filter_sql(
    builder: &mut SqlBuilder,
    value: &str,
    resource_type: &str,
    registry: &SearchParameterRegistry,
) -> Result<(), SqlBuilderError> {
    let expr = parse_filter(value)?;
    let condition = build_condition(builder, &expr, resource_type, registry)?;
    builder.add_condition(condition);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_comparison() {
        let expr = parse_filter("name eq \"Smith\"").unwrap();
        assert!(matches!(
            expr,
            FilterExpression::Comparison {
                path,
                op: FilterOp::Eq,
                value
            } if path == "name" && value == "Smith"
        ));
    }

    #[test]
    fn test_parse_unquoted_value() {
        let expr = parse_filter("status eq active").unwrap();
        assert!(matches!(
            expr,
            FilterExpression::Comparison {
                path,
                op: FilterOp::Eq,
                value
            } if path == "status" && value == "active"
        ));
    }

    #[test]
    fn test_parse_date_comparison() {
        let expr = parse_filter("birthdate ge 1990-01-01").unwrap();
        assert!(matches!(
            expr,
            FilterExpression::Comparison {
                path,
                op: FilterOp::Ge,
                value
            } if path == "birthdate" && value == "1990-01-01"
        ));
    }

    #[test]
    fn test_parse_and_expression() {
        let expr = parse_filter("name eq \"Smith\" and status eq active").unwrap();
        assert!(matches!(
            expr,
            FilterExpression::Logical {
                op: LogicalOp::And,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_or_expression() {
        let expr = parse_filter("status ne cancelled or priority eq urgent").unwrap();
        assert!(matches!(
            expr,
            FilterExpression::Logical {
                op: LogicalOp::Or,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_not_expression() {
        let expr = parse_filter("not (status eq draft)").unwrap();
        assert!(matches!(expr, FilterExpression::Not(_)));
    }

    #[test]
    fn test_parse_chained_path() {
        let expr = parse_filter("subject.name co john").unwrap();
        assert!(matches!(
            expr,
            FilterExpression::Comparison {
                path,
                op: FilterOp::Co,
                value
            } if path == "subject.name" && value == "john"
        ));
    }

    #[test]
    fn test_parse_complex_expression() {
        // (status eq active or status eq pending) and priority eq urgent
        let expr =
            parse_filter("(status eq active or status eq pending) and priority eq urgent").unwrap();
        assert!(matches!(
            expr,
            FilterExpression::Logical {
                op: LogicalOp::And,
                ..
            }
        ));
    }

    fn registry() -> SearchParameterRegistry {
        let registry = SearchParameterRegistry::new();
        for (code, param_type, expression) in [
            ("name", SearchParameterType::String, "Patient.name"),
            ("gender", SearchParameterType::Token, "Patient.gender"),
            ("birthdate", SearchParameterType::Date, "Patient.birthDate"),
        ] {
            registry.register(
                crate::parameters::SearchParameter::new(
                    code,
                    format!("http://example.org/{code}"),
                    param_type,
                    vec!["Patient".to_string()],
                )
                .with_expression(expression),
            );
        }
        registry
    }

    fn filter_sql(filter: &str) -> Result<String, SqlBuilderError> {
        let mut builder = SqlBuilder::new();
        build_filter_sql(&mut builder, filter, "Patient", &registry())?;
        Ok(builder.build_where_clause().unwrap())
    }

    #[test]
    fn comparisons_render_their_search_parameter() {
        // `co` is the string `:contains` search, not a raw JSON key lookup.
        let sql = filter_sql("name co \"smi\"").unwrap();
        assert!(!sql.contains("resource->>'name'"), "SQL={sql}");
        let mut direct = SqlBuilder::new();
        let definition = registry().get("Patient", "name").unwrap();
        dispatch_search_with_registry(
            &mut direct,
            &ParsedParam {
                name: "name".into(),
                modifier: Some(SearchModifier::Contains),
                values: vec![ParsedValue {
                    prefix: None,
                    raw: "smi".into(),
                }],
            },
            &definition,
            "Patient",
            &registry(),
        )
        .unwrap();
        assert_eq!(sql, direct.build_where_clause().unwrap());
    }

    #[test]
    fn logical_operators_combine_conditions() {
        let sql = filter_sql("gender eq female and (birthdate ge 1990-01-01 or not (name sw jo))")
            .unwrap();
        assert!(
            sql.contains(" AND ") && sql.contains(" OR ") && sql.contains("NOT"),
            "SQL={sql}"
        );
    }

    #[test]
    fn unsupported_filters_are_errors_not_empty_results() {
        assert!(matches!(
            filter_sql("agent-name co adm"),
            Err(SqlBuilderError::InvalidSearchValue(_))
        ));
        assert!(matches!(
            filter_sql("general-practitioner.name co x"),
            Err(SqlBuilderError::NotImplemented(_))
        ));
        assert!(matches!(
            filter_sql("name ew son"),
            Err(SqlBuilderError::NotImplemented(_))
        ));
        assert!(matches!(
            filter_sql("birthdate co 1990"),
            Err(SqlBuilderError::NotImplemented(_))
        ));
    }

    #[test]
    fn string_values_are_matched_literally() {
        assert_eq!(escape_search_value(r"a,b$c|d\e"), r"a\,b\$c\|d\\e");
    }
}
