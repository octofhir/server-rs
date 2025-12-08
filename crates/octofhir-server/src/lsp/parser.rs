//! SQL parser using pg_query for cursor context detection.
//!
//! This module wraps libpg_query to parse SQL and determine the cursor context
//! for providing intelligent completions.

use tower_lsp::lsp_types::Position;

/// JSONB operators that can trigger path completions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonbOperator {
    /// `->` - Get JSON object field by key (returns jsonb)
    Arrow,
    /// `->>` - Get JSON object field as text
    DoubleArrow,
    /// `#>` - Get JSON object at path (returns jsonb)
    HashArrow,
    /// `#>>` - Get JSON object at path as text
    HashDoubleArrow,
    /// `@>` - Does left contain right?
    Contains,
    /// `<@` - Is left contained in right?
    ContainedBy,
    /// `?` - Does key exist?
    Exists,
    /// `?|` - Do any keys exist?
    ExistsAny,
    /// `?&` - Do all keys exist?
    ExistsAll,
}

impl JsonbOperator {
    /// Parse a JSONB operator from a string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "->" => Some(Self::Arrow),
            "->>" => Some(Self::DoubleArrow),
            "#>" => Some(Self::HashArrow),
            "#>>" => Some(Self::HashDoubleArrow),
            "@>" => Some(Self::Contains),
            "<@" => Some(Self::ContainedBy),
            "?" => Some(Self::Exists),
            "?|" => Some(Self::ExistsAny),
            "?&" => Some(Self::ExistsAll),
            _ => None,
        }
    }

    /// Returns true if this operator expects a path string argument.
    pub fn expects_path(&self) -> bool {
        matches!(
            self,
            Self::Arrow | Self::DoubleArrow | Self::HashArrow | Self::HashDoubleArrow
        )
    }
}

/// Context information about where the cursor is in a SQL query.
#[derive(Debug, Clone)]
pub enum CursorContext {
    /// Cursor is in SELECT column list
    SelectColumns {
        /// Table alias if known
        table_alias: Option<String>,
        /// Partial text being typed
        partial: String,
    },
    /// Cursor is in FROM clause
    FromClause {
        /// Partial table name being typed
        partial: String,
    },
    /// Cursor is in WHERE clause
    WhereClause {
        /// Table alias if known
        table_alias: Option<String>,
        /// Partial text being typed
        partial: String,
    },
    /// Cursor is after a JSONB operator, expecting a path
    JsonbPath {
        /// Source table name
        table: String,
        /// Source column name
        column: String,
        /// Path segments already typed (e.g., ["name", "given"])
        path: Vec<String>,
        /// The JSONB operator being used
        operator: JsonbOperator,
    },
    /// Cursor is in function arguments
    FunctionArgs {
        /// Function name
        function: String,
        /// Argument index (0-based)
        arg_index: usize,
    },
    /// Cursor is at keyword position (start of statement or after comma)
    Keyword {
        /// Partial keyword being typed
        partial: String,
    },
    /// Unknown context
    Unknown {
        /// Any partial text at cursor
        partial: String,
    },
}

/// SQL parser for cursor context detection.
pub struct SqlParser;

impl SqlParser {
    /// Parse SQL and determine the cursor context at the given position.
    pub fn get_context(sql: &str, position: Position) -> CursorContext {
        let offset = Self::position_to_offset(sql, position);
        Self::detect_context(sql, offset)
    }

    /// Convert LSP position (line, column) to byte offset.
    fn position_to_offset(sql: &str, position: Position) -> usize {
        let mut offset = 0;
        for (line_num, line) in sql.lines().enumerate() {
            if line_num == position.line as usize {
                // Found the target line
                offset += position.character as usize;
                break;
            }
            // +1 for the newline character
            offset += line.len() + 1;
        }
        offset.min(sql.len())
    }

    /// Detect the cursor context by analyzing the SQL text before the cursor.
    fn detect_context(sql: &str, offset: usize) -> CursorContext {
        let before_cursor = &sql[..offset.min(sql.len())];

        // Try to detect JSONB path context first (highest priority)
        if let Some(ctx) = Self::detect_jsonb_context(before_cursor) {
            return ctx;
        }

        // Try to detect clause context
        if let Some(ctx) = Self::detect_clause_context(before_cursor) {
            return ctx;
        }

        // Extract partial word at cursor
        let partial = Self::extract_partial_word(before_cursor);

        CursorContext::Unknown { partial }
    }

    /// Detect if cursor is in a JSONB path context.
    fn detect_jsonb_context(before_cursor: &str) -> Option<CursorContext> {
        // Look backwards for JSONB operators
        let trimmed = before_cursor.trim_end();

        // Check for patterns like: column->'path' or column->>'path' or column->'
        // We need to find the operator and extract context

        // Find the last JSONB operator
        let operators = ["#>>", "#>", "->>", "->", "@>", "<@", "?|", "?&", "?"];

        for op in operators {
            if let Some(op_pos) = trimmed.rfind(op) {
                // Found an operator, extract what comes before and after
                let before_op = &trimmed[..op_pos];
                let after_op = &trimmed[op_pos + op.len()..];

                // Parse the JSONB operator
                let jsonb_op = JsonbOperator::from_str(op)?;

                if !jsonb_op.expects_path() {
                    continue;
                }

                // Extract table.column or just column before the first operator in the chain
                let (table, column) = Self::extract_jsonb_source(before_op);

                // Extract existing path segments
                let path = Self::extract_path_segments(after_op);

                // Check if we're currently inside a string literal (typing a path element)
                let in_string = after_op.chars().filter(|c| *c == '\'').count() % 2 == 1;

                if in_string || after_op.trim().is_empty() || after_op.ends_with("->") || after_op.ends_with("->>") {
                    return Some(CursorContext::JsonbPath {
                        table,
                        column,
                        path,
                        operator: jsonb_op,
                    });
                }
            }
        }

        None
    }

    /// Extract table and column from the source of a JSONB expression.
    fn extract_jsonb_source(before_op: &str) -> (String, String) {
        let trimmed = before_op.trim();

        // Find the identifier before the operator chain
        // Could be: resource, t.resource, patient.resource, etc.
        let words: Vec<&str> = trimmed.split_whitespace().collect();

        if let Some(last_word) = words.last() {
            // Check for table.column pattern
            if let Some(dot_pos) = last_word.rfind('.') {
                let table = last_word[..dot_pos].to_string();
                let column = &last_word[dot_pos + 1..];
                // Strip any preceding operators/path
                let column = Self::strip_jsonb_chain(column);
                return (table, column.to_string());
            } else {
                // Just column name, try to infer table from FROM clause
                let column = Self::strip_jsonb_chain(last_word);
                return (String::new(), column.to_string());
            }
        }

        (String::new(), String::new())
    }

    /// Strip any JSONB operator chain from a column reference.
    fn strip_jsonb_chain(s: &str) -> &str {
        // Find first occurrence of any JSONB operator
        let operators = ["#>>", "#>", "->>", "->"];
        let mut end = s.len();

        for op in operators {
            if let Some(pos) = s.find(op) {
                end = end.min(pos);
            }
        }

        &s[..end]
    }

    /// Extract path segments from the part after a JSONB operator.
    fn extract_path_segments(after_op: &str) -> Vec<String> {
        let mut segments = Vec::new();
        let mut current = String::new();
        let mut in_string = false;

        for c in after_op.chars() {
            match c {
                '\'' => {
                    if in_string {
                        // End of string, save segment
                        if !current.is_empty() {
                            segments.push(current.clone());
                            current.clear();
                        }
                    }
                    in_string = !in_string;
                }
                _ if in_string => {
                    current.push(c);
                }
                _ => {
                    // Outside string, look for next operator
                }
            }
        }

        segments
    }

    /// Detect the SQL clause context (SELECT, FROM, WHERE, etc.).
    fn detect_clause_context(before_cursor: &str) -> Option<CursorContext> {
        let upper = before_cursor.to_uppercase();
        let trimmed = before_cursor.trim();

        // Find the last keyword to determine context
        let keywords = [
            ("SELECT", "select"),
            ("FROM", "from"),
            ("WHERE", "where"),
            ("JOIN", "join"),
            ("LEFT JOIN", "left join"),
            ("RIGHT JOIN", "right join"),
            ("INNER JOIN", "inner join"),
            ("ORDER BY", "order by"),
            ("GROUP BY", "group by"),
            ("HAVING", "having"),
        ];

        let mut last_keyword = None;
        let mut last_pos = 0;

        for (kw_upper, _kw_lower) in &keywords {
            if let Some(pos) = upper.rfind(kw_upper) {
                if pos >= last_pos {
                    last_pos = pos;
                    last_keyword = Some(*kw_upper);
                }
            }
        }

        let partial = Self::extract_partial_word(trimmed);

        match last_keyword {
            Some("SELECT") => Some(CursorContext::SelectColumns {
                table_alias: None,
                partial,
            }),
            Some("FROM") | Some("JOIN") | Some("LEFT JOIN") | Some("RIGHT JOIN") | Some("INNER JOIN") => {
                Some(CursorContext::FromClause { partial })
            }
            Some("WHERE") | Some("HAVING") => Some(CursorContext::WhereClause {
                table_alias: None,
                partial,
            }),
            Some("ORDER BY") | Some("GROUP BY") => Some(CursorContext::SelectColumns {
                table_alias: None,
                partial,
            }),
            _ => {
                // Check if we're at the start or after a statement separator
                if trimmed.is_empty() || trimmed.ends_with(';') {
                    Some(CursorContext::Keyword { partial })
                } else {
                    None
                }
            }
        }
    }

    /// Extract the partial word being typed at the cursor.
    fn extract_partial_word(before_cursor: &str) -> String {
        let mut partial = String::new();

        for c in before_cursor.chars().rev() {
            if c.is_alphanumeric() || c == '_' {
                partial.insert(0, c);
            } else {
                break;
            }
        }

        partial
    }

    /// Try to parse SQL with pg_query and extract table information.
    #[allow(dead_code)]
    pub fn parse_tables(sql: &str) -> Vec<String> {
        match pg_query::parse(sql) {
            Ok(result) => {
                // Extract table names from the parse result
                Self::extract_tables_from_ast(&result)
            }
            Err(_) => Vec::new(),
        }
    }

    /// Extract table names from a parsed AST.
    fn extract_tables_from_ast(result: &pg_query::ParseResult) -> Vec<String> {
        let mut tables = Vec::new();

        // pg_query returns a protobuf-based AST
        // We need to traverse it to find RangeVar nodes (table references)
        for stmt in result.protobuf.stmts.iter() {
            if let Some(ref node) = stmt.stmt {
                Self::find_tables_in_node(node, &mut tables);
            }
        }

        tables
    }

    /// Recursively find table references in an AST node.
    fn find_tables_in_node(node: &pg_query::protobuf::Node, tables: &mut Vec<String>) {
        use pg_query::protobuf::node::Node as NodeEnum;

        if let Some(ref inner) = node.node {
            match inner {
                NodeEnum::RangeVar(range_var) => {
                    // Found a table reference
                    if !range_var.relname.is_empty() {
                        tables.push(range_var.relname.clone());
                    }
                }
                NodeEnum::SelectStmt(select) => {
                    // Traverse FROM clause
                    for from_item in &select.from_clause {
                        Self::find_tables_in_node(from_item, tables);
                    }
                }
                NodeEnum::JoinExpr(join) => {
                    // Traverse join arguments
                    if let Some(ref larg) = join.larg {
                        Self::find_tables_in_node(larg, tables);
                    }
                    if let Some(ref rarg) = join.rarg {
                        Self::find_tables_in_node(rarg, tables);
                    }
                }
                _ => {
                    // For other node types, we could add more traversal logic
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonb_operator_parsing() {
        assert_eq!(JsonbOperator::from_str("->"), Some(JsonbOperator::Arrow));
        assert_eq!(JsonbOperator::from_str("->>"), Some(JsonbOperator::DoubleArrow));
        assert_eq!(JsonbOperator::from_str("#>"), Some(JsonbOperator::HashArrow));
        assert_eq!(JsonbOperator::from_str("invalid"), None);
    }

    #[test]
    fn test_position_to_offset() {
        let sql = "SELECT *\nFROM patient";

        // Line 0, column 0 = offset 0
        assert_eq!(SqlParser::position_to_offset(sql, Position { line: 0, character: 0 }), 0);

        // Line 0, column 7 = offset 7 (the '*')
        assert_eq!(SqlParser::position_to_offset(sql, Position { line: 0, character: 7 }), 7);

        // Line 1, column 0 = offset 9 (start of "FROM")
        assert_eq!(SqlParser::position_to_offset(sql, Position { line: 1, character: 0 }), 9);
    }

    #[test]
    fn test_detect_from_context() {
        let sql = "SELECT * FROM ";
        let ctx = SqlParser::detect_context(sql, sql.len());

        assert!(matches!(ctx, CursorContext::FromClause { .. }));
    }

    #[test]
    fn test_detect_select_context() {
        let sql = "SELECT ";
        let ctx = SqlParser::detect_context(sql, sql.len());

        assert!(matches!(ctx, CursorContext::SelectColumns { .. }));
    }

    #[test]
    fn test_detect_jsonb_context() {
        let sql = "SELECT resource->'";
        let ctx = SqlParser::detect_context(sql, sql.len());

        if let CursorContext::JsonbPath { column, operator, .. } = ctx {
            assert_eq!(column, "resource");
            assert_eq!(operator, JsonbOperator::Arrow);
        } else {
            panic!("Expected JsonbPath context");
        }
    }

    #[test]
    fn test_extract_partial_word() {
        assert_eq!(SqlParser::extract_partial_word("SELECT col"), "col".to_string());
        assert_eq!(SqlParser::extract_partial_word("SELECT "), "".to_string());
        assert_eq!(SqlParser::extract_partial_word("table_name"), "table_name".to_string());
    }
}
