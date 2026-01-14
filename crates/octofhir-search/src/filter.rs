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

use crate::sql_builder::{SqlBuilder, SqlBuilderError};

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

/// Convert a path (potentially chained) to a JSONB accessor.
fn path_to_jsonb(path: &str) -> String {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() == 1 {
        format!("resource->>'{}'", parts[0])
    } else {
        // For chained paths like subject.name, we need to access nested JSON
        let mut accessor = "resource".to_string();
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Last part - extract as text
                accessor = format!("{}->>'{}'", accessor, part);
            } else {
                // Intermediate parts - traverse JSON
                accessor = format!("{}->'{}'", accessor, part);
            }
        }
        accessor
    }
}

/// Build SQL condition from a filter expression.
fn build_condition(
    builder: &mut SqlBuilder,
    expr: &FilterExpression,
) -> Result<String, SqlBuilderError> {
    match expr {
        FilterExpression::Comparison { path, op, value } => {
            let json_path = path_to_jsonb(path);
            let p = builder.add_text_param(value);

            let condition = match op {
                FilterOp::Eq => format!("{json_path} = ${p}"),
                FilterOp::Ne => format!("{json_path} != ${p}"),
                FilterOp::Gt => format!("{json_path} > ${p}"),
                FilterOp::Lt => format!("{json_path} < ${p}"),
                FilterOp::Ge => format!("{json_path} >= ${p}"),
                FilterOp::Le => format!("{json_path} <= ${p}"),
                FilterOp::Co => {
                    let like_p = builder.add_text_param(format!("%{}%", value));
                    format!("{json_path} ILIKE ${like_p}")
                }
                FilterOp::Sw => {
                    let like_p = builder.add_text_param(format!("{}%", value));
                    format!("{json_path} ILIKE ${like_p}")
                }
                FilterOp::Ew => {
                    let like_p = builder.add_text_param(format!("%{}", value));
                    format!("{json_path} ILIKE ${like_p}")
                }
                FilterOp::Sa => {
                    // Starts after - for date/time ranges
                    format!("{json_path} > ${p}")
                }
                FilterOp::Eb => {
                    // Ends before - for date/time ranges
                    format!("{json_path} < ${p}")
                }
                FilterOp::Ap => {
                    // Approximately - within 10% for numbers, same day for dates
                    // For simplicity, treat as equal for now
                    // TODO: Implement proper approximate matching
                    format!("{json_path} = ${p}")
                }
            };

            Ok(condition)
        }
        FilterExpression::Logical { op, left, right } => {
            let left_cond = build_condition(builder, left)?;
            let right_cond = build_condition(builder, right)?;
            let op_str = match op {
                LogicalOp::And => "AND",
                LogicalOp::Or => "OR",
            };
            Ok(format!("({} {} {})", left_cond, op_str, right_cond))
        }
        FilterExpression::Not(inner) => {
            let inner_cond = build_condition(builder, inner)?;
            Ok(format!("NOT ({})", inner_cond))
        }
    }
}

/// Build SQL for a _filter parameter using the full expression parser.
///
/// This function parses the filter expression and generates the corresponding
/// SQL WHERE clause condition.
///
/// # Arguments
///
/// * `builder` - SQL builder to add conditions to
/// * `value` - The _filter parameter value
/// * `_resource_type` - The resource type being searched (for future enhancements)
///
/// # Examples
///
/// ```ignore
/// // Simple comparison
/// build_filter_sql(builder, "name eq \"Smith\"", "Patient")?;
///
/// // Logical operators
/// build_filter_sql(builder, "status ne \"cancelled\" or priority eq \"urgent\"", "Task")?;
///
/// // Nested with parentheses
/// build_filter_sql(builder, "not (status eq \"draft\")", "DocumentReference")?;
///
/// // Chained paths
/// build_filter_sql(builder, "subject.name co \"john\"", "Observation")?;
/// ```
pub fn build_filter_sql(
    builder: &mut SqlBuilder,
    value: &str,
    _resource_type: &str,
) -> Result<(), SqlBuilderError> {
    let expr = parse_filter(value)?;
    let condition = build_condition(builder, &expr)?;
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

    #[test]
    fn test_build_simple_condition() {
        let mut builder = SqlBuilder::new();
        build_filter_sql(&mut builder, "name eq \"Smith\"", "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource->>'name'"));
        assert!(clause.contains("="));
    }

    #[test]
    fn test_build_contains_condition() {
        let mut builder = SqlBuilder::new();
        build_filter_sql(&mut builder, "name co john", "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("ILIKE"));
    }

    #[test]
    fn test_build_and_condition() {
        let mut builder = SqlBuilder::new();
        build_filter_sql(
            &mut builder,
            "name eq Smith and status eq active",
            "Patient",
        )
        .unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("AND"));
    }

    #[test]
    fn test_build_or_condition() {
        let mut builder = SqlBuilder::new();
        build_filter_sql(
            &mut builder,
            "status eq cancelled or status eq draft",
            "Task",
        )
        .unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("OR"));
    }

    #[test]
    fn test_build_not_condition() {
        let mut builder = SqlBuilder::new();
        build_filter_sql(&mut builder, "not (status eq draft)", "Task").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("NOT"));
    }

    #[test]
    fn test_build_chained_path_condition() {
        let mut builder = SqlBuilder::new();
        build_filter_sql(&mut builder, "subject.name co john", "Observation").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource->'subject'->>'name'"));
    }

    #[test]
    fn test_all_operators() {
        let ops = [
            "eq", "ne", "co", "sw", "ew", "gt", "lt", "ge", "le", "sa", "eb", "ap",
        ];
        for op in ops {
            let mut builder = SqlBuilder::new();
            let filter = format!("field {} value", op);
            build_filter_sql(&mut builder, &filter, "Test").unwrap();
            let clause = builder.build_where_clause();
            assert!(
                clause.is_some(),
                "Operator '{}' should produce a valid clause",
                op
            );
        }
    }
}
