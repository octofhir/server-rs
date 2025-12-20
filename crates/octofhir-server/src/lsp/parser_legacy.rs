//! SQL parser for cursor context detection.
//!
//! This module provides regex-based SQL parsing for cursor context detection.
//! For more robust AST-based parsing, see the tree-sitter integration in server.rs.

use async_lsp::lsp_types::Position;

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

/// Tracks quote context for JSONB path completions.
#[derive(Debug, Clone)]
pub struct JsonbQuoteContext {
    /// User typed opening quote: resource->'
    pub has_opening_quote: bool,
    /// Cursor is between quotes: resource->'nam|'
    pub cursor_inside_quotes: bool,
    /// We need to add quotes to completion
    pub needs_quotes: bool,
}

/// A table reference from the FROM clause with optional alias.
#[derive(Debug, Clone)]
pub struct TableRef {
    /// Schema name (if specified)
    pub schema: Option<String>,
    /// Table name
    pub table: String,
    /// Alias (if specified)
    pub alias: Option<String>,
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
        /// Tables from FROM clause (for column completions)
        tables: Vec<TableRef>,
    },
    /// Cursor is in FROM clause
    FromClause {
        /// Partial table name being typed
        partial: String,
    },
    /// Cursor is after a schema prefix (e.g., `FROM public.`)
    SchemaTableAccess {
        /// Schema name
        schema: String,
        /// Partial table name being typed
        partial: String,
    },
    /// Cursor is after a table alias (e.g., `SELECT p.` where p is alias for patient)
    AliasColumnAccess {
        /// The alias being used
        alias: String,
        /// The actual table name
        table: String,
        /// Schema if known
        schema: Option<String>,
        /// Partial column name being typed
        partial: String,
    },
    /// Cursor is in WHERE clause
    WhereClause {
        /// Table alias if known
        table_alias: Option<String>,
        /// Partial text being typed
        partial: String,
        /// Tables from FROM clause
        tables: Vec<TableRef>,
    },
    /// Cursor is after a JSONB operator, expecting a path
    JsonbPath {
        /// Source table name (may be empty if not explicitly specified)
        table: String,
        /// Source column name
        column: String,
        /// Path segments already typed (e.g., ["name", "given"])
        path: Vec<String>,
        /// The JSONB operator being used
        operator: JsonbOperator,
        /// Tables from FROM clause for column resolution
        tables: Vec<TableRef>,
        /// Quote context for intelligent completion
        quote_context: JsonbQuoteContext,
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
    /// Cursor is in GRANT/REVOKE statement expecting role names
    GrantRole {
        /// Partial role name being typed
        partial: String,
    },
    /// Cursor is in GRANT/REVOKE ON expecting table name
    GrantTable {
        /// Partial table name being typed
        partial: String,
    },
    /// Cursor is in CREATE/ALTER POLICY statement
    PolicyDefinition {
        /// Table the policy is on
        table: Option<String>,
        /// Partial text being typed
        partial: String,
    },
    /// Cursor is in CAST expression expecting type name
    CastType {
        /// Partial type name being typed
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
    ///
    /// Note: JSONB path detection has been migrated to AST-based methods.
    /// For JSONB completions, use `PostgresLspServer::find_jsonb_expression_robust` instead.
    pub fn get_context(sql: &str, position: Position) -> CursorContext {
        let offset = Self::position_to_offset(sql, position);
        Self::detect_context(sql, offset)
    }

    /// Convert LSP position (line, column) to byte offset.
    /// Properly handles multi-byte UTF-8 characters to prevent panics.
    fn position_to_offset(sql: &str, position: Position) -> usize {
        let mut offset = 0;
        for (line_num, line) in sql.lines().enumerate() {
            if line_num == position.line as usize {
                // Found the target line
                // Convert character position to byte offset
                // LSP uses UTF-16 code units, but we use char boundaries for safety
                let char_offset = position.character as usize;
                let line_chars: Vec<char> = line.chars().collect();
                let byte_offset: usize = line_chars
                    .iter()
                    .take(char_offset.min(line_chars.len()))
                    .map(|c| c.len_utf8())
                    .sum();
                return offset + byte_offset;
            }
            // +1 for the newline character
            offset += line.len() + 1;
        }
        offset.min(sql.len())
    }

    /// Detect the cursor context by analyzing the SQL text before the cursor.
    fn detect_context(sql: &str, offset: usize) -> CursorContext {
        // Ensure we're on a char boundary to prevent UTF-8 panics
        let safe_offset = offset.min(sql.len());
        let safe_offset = if sql.is_char_boundary(safe_offset) {
            safe_offset
        } else {
            // Find the previous char boundary
            (0..safe_offset)
                .rev()
                .find(|&i| sql.is_char_boundary(i))
                .unwrap_or(0)
        };
        let before_cursor = &sql[..safe_offset];

        // Parse table references from the entire SQL for alias resolution
        let tables = Self::parse_table_references(sql);

        // Try to detect alias.column pattern first (e.g., `p.` where p is alias)
        if let Some(ctx) = Self::detect_alias_column_access(before_cursor, &tables) {
            return ctx;
        }

        // JSONB path detection has been migrated to AST-based methods
        // Use PostgresLspServer::find_jsonb_expression_robust() instead
        // (regex-based detection removed - full AST migration)

        // Try to detect clause context
        if let Some(ctx) = Self::detect_clause_context(before_cursor, &tables) {
            return ctx;
        }

        // Extract partial word at cursor
        let partial = Self::extract_partial_word(before_cursor);

        CursorContext::Unknown { partial }
    }

    /// Parse table references from SQL (FROM and JOIN clauses).
    fn parse_table_references(sql: &str) -> Vec<TableRef> {
        let mut tables = Vec::new();

        // Regex pattern for: [schema.]table [AS] alias
        // Matches patterns like:
        // - patient
        // - patient p
        // - patient AS p
        // - public.patient
        // - public.patient p
        // - public.patient AS p
        let upper = sql.to_uppercase();

        // Find FROM clause
        if let Some(from_pos) = upper.find("FROM") {
            let after_from = &sql[from_pos + 4..];

            // Find the end of FROM clause (WHERE, ORDER BY, GROUP BY, HAVING, LIMIT, ;, or end)
            let end_keywords = [
                "WHERE",
                "ORDER",
                "GROUP",
                "HAVING",
                "LIMIT",
                "UNION",
                "EXCEPT",
                "INTERSECT",
            ];
            let mut end_pos = after_from.len();

            for kw in end_keywords {
                if let Some(pos) = after_from.to_uppercase().find(kw) {
                    end_pos = end_pos.min(pos);
                }
            }
            if let Some(pos) = after_from.find(';') {
                end_pos = end_pos.min(pos);
            }

            let from_clause = &after_from[..end_pos];

            // Split by JOIN keywords and commas to get individual table references
            let join_pattern = regex::Regex::new(r"(?i)\b(LEFT\s+)?(?:OUTER\s+)?(?:INNER\s+)?(?:RIGHT\s+)?(?:CROSS\s+)?(?:FULL\s+)?JOIN\b|,").ok();

            let table_parts: Vec<&str> = if let Some(ref pat) = join_pattern {
                pat.split(from_clause).collect()
            } else {
                vec![from_clause]
            };

            for part in table_parts {
                if let Some(table_ref) = Self::parse_single_table_ref(part.trim()) {
                    tables.push(table_ref);
                }
            }
        }

        tables
    }

    /// Parse a single table reference like "patient p" or "public.patient AS p".
    fn parse_single_table_ref(s: &str) -> Option<TableRef> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        // Remove ON clause if present
        let s = if let Some(on_pos) = s.to_uppercase().find(" ON ") {
            &s[..on_pos]
        } else {
            s
        };

        let tokens: Vec<&str> = s.split_whitespace().collect();
        if tokens.is_empty() {
            return None;
        }

        // First token is [schema.]table
        let first = tokens[0];
        let (schema, table) = if let Some(dot_pos) = first.find('.') {
            (
                Some(first[..dot_pos].trim_matches('"').to_string()),
                first[dot_pos + 1..].trim_matches('"').to_string(),
            )
        } else {
            (None, first.trim_matches('"').to_string())
        };

        // Look for alias
        let alias = if tokens.len() >= 3 && tokens[1].eq_ignore_ascii_case("AS") {
            // pattern: table AS alias
            Some(tokens[2].trim_matches('"').to_string())
        } else if tokens.len() >= 2 && !tokens[1].eq_ignore_ascii_case("ON") {
            // pattern: table alias (no AS keyword)
            Some(tokens[1].trim_matches('"').to_string())
        } else {
            None
        };

        Some(TableRef {
            schema,
            table,
            alias,
        })
    }

    /// Detect alias.column access pattern (e.g., `p.` or `p.res`).
    fn detect_alias_column_access(
        before_cursor: &str,
        tables: &[TableRef],
    ) -> Option<CursorContext> {
        let trimmed = before_cursor.trim();

        // Get the last token (what the user is typing)
        let last_token = trimmed.split_whitespace().last()?;

        // Check for alias.partial pattern (but not schema.table which has tables in FROM)
        if let Some(dot_pos) = last_token.rfind('.') {
            let potential_alias = &last_token[..dot_pos];
            let partial = &last_token[dot_pos + 1..];

            // Check if this matches a known table alias
            for table_ref in tables {
                // Match against alias
                if let Some(ref alias) = table_ref.alias {
                    if alias.eq_ignore_ascii_case(potential_alias) {
                        return Some(CursorContext::AliasColumnAccess {
                            alias: alias.clone(),
                            table: table_ref.table.clone(),
                            schema: table_ref.schema.clone(),
                            partial: partial.to_string(),
                        });
                    }
                }

                // Also match against table name directly (e.g., `patient.id`)
                if table_ref.table.eq_ignore_ascii_case(potential_alias) {
                    return Some(CursorContext::AliasColumnAccess {
                        alias: potential_alias.to_string(),
                        table: table_ref.table.clone(),
                        schema: table_ref.schema.clone(),
                        partial: partial.to_string(),
                    });
                }
            }
        }

        None
    }

    /// Detect the SQL clause context (SELECT, FROM, WHERE, etc.).
    fn detect_clause_context(before_cursor: &str, tables: &[TableRef]) -> Option<CursorContext> {
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
            // GRANT/REVOKE keywords
            ("GRANT", "grant"),
            ("REVOKE", "revoke"),
            ("TO", "to"),
            ("ON", "on"),
            // Policy keywords
            ("CREATE POLICY", "create policy"),
            ("ALTER POLICY", "alter policy"),
            // Type/cast keywords
            ("CAST", "cast"),
            ("AS", "as"),
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

        // Check for schema.table pattern in FROM/JOIN clauses
        if matches!(
            last_keyword,
            Some("FROM")
                | Some("JOIN")
                | Some("LEFT JOIN")
                | Some("RIGHT JOIN")
                | Some("INNER JOIN")
        ) {
            if let Some(schema_ctx) = Self::detect_schema_table_access(trimmed) {
                return Some(schema_ctx);
            }
        }

        let partial = Self::extract_partial_word(trimmed);

        match last_keyword {
            Some("SELECT") => Some(CursorContext::SelectColumns {
                table_alias: None,
                partial,
                tables: tables.to_vec(),
            }),
            Some("FROM") | Some("JOIN") | Some("LEFT JOIN") | Some("RIGHT JOIN")
            | Some("INNER JOIN") => Some(CursorContext::FromClause { partial }),
            Some("WHERE") | Some("HAVING") => Some(CursorContext::WhereClause {
                table_alias: None,
                partial,
                tables: tables.to_vec(),
            }),
            Some("ORDER BY") | Some("GROUP BY") => Some(CursorContext::SelectColumns {
                table_alias: None,
                partial,
                tables: tables.to_vec(),
            }),
            // GRANT/REVOKE TO expects role names
            Some("TO") => {
                // Check if this is part of a GRANT/REVOKE statement
                if upper.contains("GRANT") || upper.contains("REVOKE") {
                    Some(CursorContext::GrantRole { partial })
                } else {
                    None
                }
            }
            // GRANT/REVOKE ON expects table names
            Some("ON") => {
                if upper.contains("GRANT") || upper.contains("REVOKE") || upper.contains("POLICY") {
                    Some(CursorContext::GrantTable { partial })
                } else {
                    None
                }
            }
            // GRANT without TO yet - could be privileges or ON
            Some("GRANT") | Some("REVOKE") => {
                // If we see ON after GRANT, it's table context
                // Otherwise, show both keywords and tables
                Some(CursorContext::Keyword { partial })
            }
            // CREATE/ALTER POLICY contexts
            Some("CREATE POLICY") | Some("ALTER POLICY") => {
                // Extract table name if it appears after ON
                let table = Self::extract_policy_table(&upper);
                Some(CursorContext::PolicyDefinition { table, partial })
            }
            // CAST AS expects type names
            Some("AS") => {
                // Check if this is part of a CAST expression
                if upper.contains("CAST") {
                    Some(CursorContext::CastType { partial })
                } else {
                    // Regular AS for aliases
                    None
                }
            }
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

    /// Extract table name from a POLICY statement (after ON keyword).
    fn extract_policy_table(sql_upper: &str) -> Option<String> {
        // Look for "ON table_name" pattern
        if let Some(on_pos) = sql_upper.find(" ON ") {
            let after_on = &sql_upper[on_pos + 4..];
            // Get the first word after ON
            if let Some(table) = after_on.split_whitespace().next() {
                return Some(table.to_lowercase());
            }
        }
        None
    }

    /// Detect schema.table access pattern (e.g., `FROM public.` or `FROM auth.pat`).
    fn detect_schema_table_access(before_cursor: &str) -> Option<CursorContext> {
        // Get the last token (what the user is typing)
        let last_token = before_cursor.split_whitespace().last()?;

        // Check for schema.partial pattern
        if let Some(dot_pos) = last_token.rfind('.') {
            let schema = &last_token[..dot_pos];
            let partial = &last_token[dot_pos + 1..];

            // Only if schema part is a valid identifier (not empty, alphanumeric + underscore)
            if !schema.is_empty()
                && schema
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '"')
            {
                // Strip quotes if present
                let schema = schema.trim_matches('"');

                return Some(CursorContext::SchemaTableAccess {
                    schema: schema.to_string(),
                    partial: partial.to_string(),
                });
            }
        }

        None
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonb_operator_parsing() {
        assert_eq!(JsonbOperator::from_str("->"), Some(JsonbOperator::Arrow));
        assert_eq!(
            JsonbOperator::from_str("->>"),
            Some(JsonbOperator::DoubleArrow)
        );
        assert_eq!(
            JsonbOperator::from_str("#>"),
            Some(JsonbOperator::HashArrow)
        );
        assert_eq!(JsonbOperator::from_str("invalid"), None);
    }

    #[test]
    fn test_position_to_offset() {
        let sql = "SELECT *\nFROM patient";

        // Line 0, column 0 = offset 0
        assert_eq!(
            SqlParser::position_to_offset(
                sql,
                Position {
                    line: 0,
                    character: 0
                }
            ),
            0
        );

        // Line 0, column 7 = offset 7 (the '*')
        assert_eq!(
            SqlParser::position_to_offset(
                sql,
                Position {
                    line: 0,
                    character: 7
                }
            ),
            7
        );

        // Line 1, column 0 = offset 9 (start of "FROM")
        assert_eq!(
            SqlParser::position_to_offset(
                sql,
                Position {
                    line: 1,
                    character: 0
                }
            ),
            9
        );
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
    fn test_extract_partial_word() {
        assert_eq!(
            SqlParser::extract_partial_word("SELECT col"),
            "col".to_string()
        );
        assert_eq!(SqlParser::extract_partial_word("SELECT "), "".to_string());
        assert_eq!(
            SqlParser::extract_partial_word("table_name"),
            "table_name".to_string()
        );
    }

    #[test]
    fn test_detect_schema_table_access() {
        // Test schema.partial pattern
        let sql = "SELECT * FROM public.";
        let ctx = SqlParser::detect_context(sql, sql.len());

        if let CursorContext::SchemaTableAccess { schema, partial } = ctx {
            assert_eq!(schema, "public");
            assert_eq!(partial, "");
        } else {
            panic!("Expected SchemaTableAccess context, got {:?}", ctx);
        }

        // Test schema.partial with partial table name
        let sql = "SELECT * FROM public.pat";
        let ctx = SqlParser::detect_context(sql, sql.len());

        if let CursorContext::SchemaTableAccess { schema, partial } = ctx {
            assert_eq!(schema, "public");
            assert_eq!(partial, "pat");
        } else {
            panic!("Expected SchemaTableAccess context, got {:?}", ctx);
        }

        // Test with quoted schema
        let sql = r#"SELECT * FROM "auth"."#;
        let ctx = SqlParser::detect_context(sql, sql.len());

        if let CursorContext::SchemaTableAccess { schema, partial } = ctx {
            assert_eq!(schema, "auth");
            assert_eq!(partial, "");
        } else {
            panic!("Expected SchemaTableAccess context, got {:?}", ctx);
        }
    }
}
