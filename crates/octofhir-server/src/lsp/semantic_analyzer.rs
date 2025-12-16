use sqlparser::ast::{
    BinaryOperator, Expr, Function, FunctionArg, FunctionArgExpr, LimitClause, Query, SelectItem,
    SetExpr, Statement,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::collections::{HashMap, HashSet};

/// Semantic analyzer using sqlparser-rs for better PostgreSQL coverage
pub struct SemanticAnalyzer;

impl SemanticAnalyzer {
    /// Parse SQL and extract semantic context
    pub fn analyze(text: &str) -> Option<SemanticContext> {
        let ast = Parser::parse_sql(&PostgreSqlDialect {}, text).ok()?;

        Some(SemanticContext {
            jsonb_operator: Self::find_jsonb_operator(&ast),
            existing_clauses: Self::extract_clauses(&ast),
            table_aliases: Self::extract_aliases(&ast),
        })
    }

    /// Find JSONB operators and functions in the AST
    pub fn find_jsonb_operator(statements: &[Statement]) -> Option<JsonbOperatorInfo> {
        for stmt in statements {
            if let Statement::Query(query) = stmt {
                if let Some(info) = Self::find_jsonb_in_query(query) {
                    return Some(info);
                }
            }
        }
        None
    }

    fn find_jsonb_in_query(query: &Query) -> Option<JsonbOperatorInfo> {
        if let SetExpr::Select(select) = &*query.body {
            // Check SELECT projection
            for proj in &select.projection {
                match proj {
                    SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                        if let Some(info) = Self::find_jsonb_in_expr(expr) {
                            return Some(info);
                        }
                    }
                    _ => {}
                }
            }

            // Check WHERE clause
            if let Some(selection) = &select.selection {
                if let Some(info) = Self::find_jsonb_in_expr(selection) {
                    return Some(info);
                }
            }
        }
        None
    }

    fn find_jsonb_in_expr(expr: &Expr) -> Option<JsonbOperatorInfo> {
        match expr {
            // JSONB operator detection (e.g., resource -> 'name')
            Expr::BinaryOp { left, op, right } if Self::is_jsonb_operator(op) => {
                Some(JsonbOperatorInfo {
                    operator: Some(op.clone()),
                    function_name: None,
                    left_expr: format!("{}", left),
                    right_expr: Some(format!("{}", right)),
                    path_expr: Some(format!("{}", right)),
                })
            }
            // JSONB function detection (e.g., jsonb_path_exists(resource, '$.name'))
            Expr::Function(func) => Self::find_jsonb_function(func),
            // Recursively search nested expressions
            Expr::BinaryOp { left, right, .. } => {
                Self::find_jsonb_in_expr(left).or_else(|| Self::find_jsonb_in_expr(right))
            }
            Expr::Nested(inner) => Self::find_jsonb_in_expr(inner),
            Expr::Cast { expr, .. } => Self::find_jsonb_in_expr(expr),
            Expr::InList { expr, .. } => Self::find_jsonb_in_expr(expr),
            Expr::Between {
                expr, low, high, ..
            } => Self::find_jsonb_in_expr(expr)
                .or_else(|| Self::find_jsonb_in_expr(low))
                .or_else(|| Self::find_jsonb_in_expr(high)),
            Expr::Case {
                operand,
                conditions,
                else_result,
                ..
            } => {
                if let Some(op) = operand {
                    if let Some(info) = Self::find_jsonb_in_expr(op) {
                        return Some(info);
                    }
                }
                for case_when in conditions {
                    if let Some(info) = Self::find_jsonb_in_expr(&case_when.condition) {
                        return Some(info);
                    }
                    if let Some(info) = Self::find_jsonb_in_expr(&case_when.result) {
                        return Some(info);
                    }
                }
                if let Some(else_res) = else_result {
                    return Self::find_jsonb_in_expr(else_res);
                }
                None
            }
            _ => None,
        }
    }

    fn find_jsonb_function(func: &Function) -> Option<JsonbOperatorInfo> {
        let func_name = func.name.to_string().to_lowercase();

        // Check if it's a JSONB function
        if Self::is_jsonb_function(&func_name) {
            let mut target_expr = None;
            let mut path_expr = None;

            // Extract arguments from FunctionArguments - CORRECT API for v0.60
            if let sqlparser::ast::FunctionArguments::List(arg_list) = &func.args {
                for (idx, arg) in arg_list.args.iter().enumerate() {
                    if let FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) = arg {
                        let arg_str = format!("{}", expr);

                        // First arg is typically the JSONB column
                        if idx == 0 {
                            target_expr = Some(arg_str);
                        }
                        // Second arg is typically the path
                        else if idx == 1 {
                            path_expr = Some(arg_str);
                        }
                    }
                }
            }

            return Some(JsonbOperatorInfo {
                operator: None,
                function_name: Some(func_name),
                left_expr: target_expr.unwrap_or_default(),
                right_expr: path_expr.clone(),
                path_expr,
            });
        }

        None
    }

    fn is_jsonb_operator(op: &BinaryOperator) -> bool {
        matches!(
            op,
            BinaryOperator::Arrow
                | BinaryOperator::LongArrow
                | BinaryOperator::HashArrow
                | BinaryOperator::HashLongArrow
        )
    }

    fn is_jsonb_function(name: &str) -> bool {
        matches!(
            name,
            "jsonb_path_exists"
                | "jsonb_path_query"
                | "jsonb_path_query_array"
                | "jsonb_path_query_first"
                | "jsonb_path_match"
                | "jsonb_extract_path"
                | "jsonb_extract_path_text"
                | "jsonb_array_elements"
                | "jsonb_array_elements_text"
                | "jsonb_each"
                | "jsonb_each_text"
                | "jsonb_object_keys"
                | "jsonb_typeof"
                | "jsonb_array_length"
        )
    }

    /// Extract existing SQL clauses (WHERE, GROUP BY, ORDER BY, etc.)
    pub fn extract_clauses(statements: &[Statement]) -> HashSet<String> {
        let mut clauses = HashSet::new();

        for stmt in statements {
            if let Statement::Query(query) = stmt {
                Self::extract_clauses_from_query(query, &mut clauses);
            }
        }

        clauses
    }

    fn extract_clauses_from_query(query: &Query, clauses: &mut HashSet<String>) {
        if let SetExpr::Select(select) = &*query.body {
            // Check for WHERE clause
            if select.selection.is_some() {
                clauses.insert("WHERE".to_string());
            }

            // Check for GROUP BY clause - CORRECT API for v0.60
            match &select.group_by {
                sqlparser::ast::GroupByExpr::All(_) => {
                    clauses.insert("GROUP BY".to_string());
                }
                sqlparser::ast::GroupByExpr::Expressions(exprs, _) => {
                    if !exprs.is_empty() {
                        clauses.insert("GROUP BY".to_string());
                    }
                }
            }

            // Check for HAVING clause
            if select.having.is_some() {
                clauses.insert("HAVING".to_string());
            }

            // Check for DISTINCT - CORRECT API for v0.60
            if select.distinct.is_some() {
                clauses.insert("DISTINCT".to_string());
            }
        }

        // Check for ORDER BY clause
        if query.order_by.is_some() {
            clauses.insert("ORDER BY".to_string());
        }

        // Check for LIMIT/OFFSET - CORRECT API for v0.60
        // LimitClause is an enum with two variants:
        // 1. LimitOffset { limit: Option<Expr>, offset: Option<Offset>, limit_by: Vec<Expr> }
        // 2. OffsetCommaLimit { offset: Expr, limit: Expr } - MySQL syntax
        if let Some(limit_clause) = &query.limit_clause {
            match limit_clause {
                LimitClause::LimitOffset {
                    limit,
                    offset,
                    limit_by: _,
                } => {
                    // Standard SQL: LIMIT and OFFSET are independent
                    if limit.is_some() {
                        clauses.insert("LIMIT".to_string());
                    }
                    if offset.is_some() {
                        clauses.insert("OFFSET".to_string());
                    }
                }
                LimitClause::OffsetCommaLimit { .. } => {
                    // MySQL syntax: both are always present
                    clauses.insert("LIMIT".to_string());
                    clauses.insert("OFFSET".to_string());
                }
            }
        }
    }

    /// Extract table aliases (placeholder for future)
    pub fn extract_aliases(_statements: &[Statement]) -> HashMap<String, String> {
        HashMap::new()
    }
}

#[derive(Debug, Clone)]
pub struct SemanticContext {
    pub jsonb_operator: Option<JsonbOperatorInfo>,
    pub existing_clauses: HashSet<String>,
    pub table_aliases: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct JsonbOperatorInfo {
    /// JSONB operator (if using operator syntax like ->, ->>, #>, #>>)
    pub operator: Option<BinaryOperator>,
    /// JSONB function name (if using function syntax like jsonb_path_exists)
    pub function_name: Option<String>,
    /// Left expression (column name or JSONB value)
    pub left_expr: String,
    /// Right expression (path or argument)
    pub right_expr: Option<String>,
    /// Path expression for field suggestion
    pub path_expr: Option<String>,
}

impl JsonbOperatorInfo {
    /// Check if this is a JSONB operator usage (not function)
    pub fn is_operator(&self) -> bool {
        self.operator.is_some()
    }

    /// Check if this is a JSONB function usage
    pub fn is_function(&self) -> bool {
        self.function_name.is_some()
    }

    /// Get the JSONB column/target being accessed
    pub fn target(&self) -> &str {
        &self.left_expr
    }

    /// Get the path being accessed (if available)
    pub fn path(&self) -> Option<&str> {
        self.path_expr.as_deref()
    }
}
