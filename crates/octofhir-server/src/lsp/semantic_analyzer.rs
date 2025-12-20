use std::collections::{HashMap, HashSet};

/// Semantic analyzer using pg_query for 100% PostgreSQL compatibility
pub struct SemanticAnalyzer;

impl SemanticAnalyzer {
    /// Parse SQL and extract semantic context
    pub fn analyze(text: &str) -> Option<SemanticContext> {
        // Use pg_query to parse SQL with actual PostgreSQL parser
        let parse_result = pg_query::parse(text).ok()?;

        Some(SemanticContext {
            jsonb_operator: Self::find_jsonb_operator(text, &parse_result),
            existing_clauses: Self::extract_clauses(text, &parse_result),
            table_aliases: Self::extract_aliases(&parse_result),
        })
    }

    /// Find JSONB operators and functions in the SQL text
    /// Note: For now, we use simple text-based detection since pg_query's protobuf AST
    /// is complex to navigate. This will be improved in Phase 3 with proper AST walking.
    fn find_jsonb_operator(
        text: &str,
        _parse_result: &pg_query::ParseResult,
    ) -> Option<JsonbOperatorInfo> {
        // Simple heuristic: look for JSONB operators in the text
        // This is a temporary solution until we implement proper AST walking

        // Check for arrow operators: ->, ->>, #>, #>>
        if text.contains("->") || text.contains("#>") {
            return Self::detect_jsonb_operator_heuristic(text);
        }

        // Check for JSONB functions
        if Self::contains_jsonb_function(text) {
            return Self::detect_jsonb_function_heuristic(text);
        }

        None
    }

    /// Heuristic detection of JSONB operators from SQL text
    fn detect_jsonb_operator_heuristic(text: &str) -> Option<JsonbOperatorInfo> {
        // Find arrow operators
        let operators = ["#>>", "#>", "->>", "->"];

        for op in &operators {
            if let Some(pos) = text.find(op) {
                // Extract context around operator
                let before = &text[..pos].trim_end();
                let after = &text[pos + op.len()..].trim_start();

                // Extract left side (column/table reference)
                let left_expr = before
                    .split_whitespace()
                    .last()
                    .unwrap_or("")
                    .to_string();

                // Extract right side (path)
                let right_expr = after
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c| c == '\'' || c == '"' || c == ',' || c == ';')
                    .to_string();

                return Some(JsonbOperatorInfo {
                    operator_str: Some(op.to_string()),
                    function_name: None,
                    left_expr,
                    right_expr: Some(right_expr.clone()),
                    path_expr: Some(right_expr),
                });
            }
        }

        None
    }

    /// Check if text contains JSONB functions
    fn contains_jsonb_function(text: &str) -> bool {
        let lower = text.to_lowercase();
        matches!(
            true,
            _ if lower.contains("jsonb_path_exists")
                || lower.contains("jsonb_path_query")
                || lower.contains("jsonb_extract_path")
                || lower.contains("jsonb_array_elements")
                || lower.contains("jsonb_each")
                || lower.contains("jsonb_object_keys")
        )
    }

    /// Heuristic detection of JSONB functions from SQL text
    fn detect_jsonb_function_heuristic(text: &str) -> Option<JsonbOperatorInfo> {
        let functions = [
            "jsonb_path_exists",
            "jsonb_path_query",
            "jsonb_path_query_array",
            "jsonb_path_query_first",
            "jsonb_extract_path",
            "jsonb_extract_path_text",
            "jsonb_array_elements",
            "jsonb_array_elements_text",
            "jsonb_each",
            "jsonb_each_text",
            "jsonb_object_keys",
        ];

        let lower = text.to_lowercase();

        for func in &functions {
            if let Some(pos) = lower.find(func) {
                // Extract function arguments (simple heuristic)
                let after = &text[pos + func.len()..];
                if let Some(open_paren) = after.find('(') {
                    if let Some(close_paren) = after.find(')') {
                        let args = &after[open_paren + 1..close_paren];
                        let parts: Vec<&str> = args.split(',').collect();

                        let target = parts.get(0).map(|s| s.trim().to_string()).unwrap_or_default();
                        let path = parts.get(1).map(|s| {
                            s.trim()
                                .trim_matches(|c| c == '\'' || c == '"')
                                .to_string()
                        });

                        return Some(JsonbOperatorInfo {
                            operator_str: None,
                            function_name: Some(func.to_string()),
                            left_expr: target,
                            right_expr: path.clone(),
                            path_expr: path,
                        });
                    }
                }
            }
        }

        None
    }

    /// Extract existing SQL clauses using simple text-based detection
    /// Note: Proper AST walking will be implemented in Phase 3
    fn extract_clauses(text: &str, parse_result: &pg_query::ParseResult) -> HashSet<String> {
        // For Phase 1, use simple text-based detection
        // In Phase 3, we'll walk the protobuf AST properly
        if parse_result.protobuf.stmts.is_empty() {
            return HashSet::new();
        }

        // Use simple keyword detection as a temporary solution
        // This is intentionally simple - Phase 3 will add proper AST walking
        Self::extract_clauses_from_text(text)
    }

    /// Extract clauses from SQL text using simple keyword matching
    /// This is a temporary heuristic until Phase 3 implements proper AST walking
    fn extract_clauses_from_text(text: &str) -> HashSet<String> {
        let mut clauses = HashSet::new();
        let upper = text.to_uppercase();

        // Check for DISTINCT (must come after SELECT)
        if upper.contains("SELECT DISTINCT") || upper.contains("SELECT\nDISTINCT") {
            clauses.insert("DISTINCT".to_string());
        }

        // Check for WHERE clause
        if Self::contains_clause_keyword(&upper, "WHERE") {
            clauses.insert("WHERE".to_string());
        }

        // Check for GROUP BY clause
        if Self::contains_clause_keyword(&upper, "GROUP BY") {
            clauses.insert("GROUP BY".to_string());
        }

        // Check for HAVING clause
        if Self::contains_clause_keyword(&upper, "HAVING") {
            clauses.insert("HAVING".to_string());
        }

        // Check for ORDER BY clause
        if Self::contains_clause_keyword(&upper, "ORDER BY") {
            clauses.insert("ORDER BY".to_string());
        }

        // Check for LIMIT clause
        if Self::contains_clause_keyword(&upper, "LIMIT") {
            clauses.insert("LIMIT".to_string());
        }

        // Check for OFFSET clause
        if Self::contains_clause_keyword(&upper, "OFFSET") {
            clauses.insert("OFFSET".to_string());
        }

        clauses
    }

    /// Check if a clause keyword exists in the SQL text
    /// Simple heuristic: keyword must be preceded by whitespace or start of string
    fn contains_clause_keyword(upper_text: &str, keyword: &str) -> bool {
        if let Some(pos) = upper_text.find(keyword) {
            // Check if it's a standalone keyword (not part of another word)
            if pos == 0 {
                return true;
            }
            let before = &upper_text[..pos];
            let last_char = before.chars().last();
            // Keyword should be preceded by whitespace, newline, or opening paren
            matches!(last_char, Some(' ') | Some('\n') | Some('\t') | Some('('))
        } else {
            false
        }
    }

    /// Extract table aliases (placeholder for Phase 3)
    /// Full implementation will be done in Phase 3 with TableResolver
    fn extract_aliases(_parse_result: &pg_query::ParseResult) -> HashMap<String, String> {
        // Placeholder - will be implemented with proper AST walking in Phase 3
        // when we build the enhanced TableResolver
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
    /// JSONB operator string (if using operator syntax like ->, ->>, #>, #>>)
    pub operator_str: Option<String>,
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
        self.operator_str.is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_select() {
        let sql = "SELECT * FROM patient";
        let result = SemanticAnalyzer::analyze(sql);
        assert!(result.is_some());

        let ctx = result.unwrap();
        // Simple SELECT should have no clauses (not counting SELECT itself)
        assert!(ctx.existing_clauses.is_empty());
    }

    #[test]
    fn test_detect_jsonb_arrow_operator() {
        let sql = "SELECT resource->'name' FROM patient";
        let result = SemanticAnalyzer::analyze(sql);
        assert!(result.is_some());

        let ctx = result.unwrap();
        assert!(ctx.jsonb_operator.is_some());

        let jsonb = ctx.jsonb_operator.unwrap();
        assert!(jsonb.is_operator());
        assert_eq!(jsonb.operator_str.as_deref(), Some("->"));
    }

    #[test]
    fn test_detect_jsonb_long_arrow_operator() {
        let sql = "SELECT resource->>'status' FROM patient";
        let result = SemanticAnalyzer::analyze(sql);
        assert!(result.is_some());

        let ctx = result.unwrap();
        assert!(ctx.jsonb_operator.is_some());

        let jsonb = ctx.jsonb_operator.unwrap();
        assert!(jsonb.is_operator());
        assert_eq!(jsonb.operator_str.as_deref(), Some("->>"));
    }

    #[test]
    fn test_detect_jsonb_function() {
        let sql = "SELECT jsonb_path_exists(resource, '$.name') FROM patient";
        let result = SemanticAnalyzer::analyze(sql);
        assert!(result.is_some());

        let ctx = result.unwrap();
        assert!(ctx.jsonb_operator.is_some());

        let jsonb = ctx.jsonb_operator.unwrap();
        assert!(jsonb.is_function());
        assert_eq!(jsonb.function_name.as_deref(), Some("jsonb_path_exists"));
    }

    #[test]
    fn test_invalid_sql() {
        let sql = "SELECT FROM";
        let result = SemanticAnalyzer::analyze(sql);
        // pg_query will fail to parse invalid SQL
        assert!(result.is_none());
    }
}
