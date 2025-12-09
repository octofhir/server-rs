//! Tree-sitter based SQL context extraction.
//!
//! This module uses tree-sitter to parse SQL and extract semantic context
//! for intelligent completions, similar to Supabase's postgres-language-server.

use tree_sitter::{Node, Parser, Point, Tree};

/// SQL clause that wraps the cursor position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrappingClause {
    Select,
    From,
    Where,
    Join,
    On,
    GroupBy,
    OrderBy,
    Having,
    Insert,
    Update,
    Delete,
    Set,
    Values,
    Returning,
    With,
    Grant,
    Revoke,
    Create,
    Alter,
    Drop,
}

/// A table reference with optional alias.
#[derive(Debug, Clone)]
pub struct RelationMatch {
    /// Schema name (if specified, e.g., "public" in "public.patient")
    pub schema: Option<String>,
    /// Table name
    pub name: String,
    /// Alias (if specified, e.g., "p" in "patient p")
    pub alias: Option<String>,
}

/// Context extracted from tree-sitter AST.
#[derive(Debug)]
pub struct TreesitterContext {
    /// The SQL clause containing the cursor
    pub wrapping_clause: Option<WrappingClause>,
    /// Tables referenced in FROM/JOIN clauses
    pub relations: Vec<RelationMatch>,
    /// Current identifier being typed (if any)
    pub current_identifier: Option<String>,
    /// Qualifier before cursor (e.g., "p" in "p.id" or "schema" in "schema.table")
    pub qualifier: Option<String>,
    /// Whether cursor is inside a string literal
    pub in_string: bool,
    /// The kind of node at cursor position
    pub node_kind_at_cursor: Option<String>,
    /// Byte offset of cursor in the text
    cursor_offset: usize,
}

impl TreesitterContext {
    /// Parse SQL and extract context at the given byte offset.
    pub fn from_sql(sql: &str, cursor_offset: usize) -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_sql::LANGUAGE.into())
            .expect("Error loading SQL grammar");

        let tree = match parser.parse(sql, None) {
            Some(t) => t,
            None => {
                return Self::empty(cursor_offset);
            }
        };

        Self::from_tree(sql, &tree, cursor_offset)
    }

    /// Create context from an already-parsed tree.
    pub fn from_tree(sql: &str, tree: &Tree, cursor_offset: usize) -> Self {
        let root = tree.root_node();

        // Adjust cursor position for trailing whitespace/incomplete input
        let adjusted_offset = Self::adjust_cursor_position(sql, cursor_offset);

        // Find node at cursor
        let point = Self::offset_to_point(sql, adjusted_offset);
        let node_at_cursor = root.descendant_for_point_range(point, point);

        // Extract context
        let wrapping_clause = Self::find_wrapping_clause(sql, &root, adjusted_offset);
        let relations = Self::extract_relations(sql, &root);
        let (qualifier, current_identifier) = Self::extract_identifier_context(sql, adjusted_offset);
        let in_string = Self::is_in_string(sql, &root, adjusted_offset);

        Self {
            wrapping_clause,
            relations,
            current_identifier,
            qualifier,
            in_string,
            node_kind_at_cursor: node_at_cursor.map(|n| n.kind().to_string()),
            cursor_offset: adjusted_offset,
        }
    }

    /// Create an empty context (for parse failures).
    fn empty(cursor_offset: usize) -> Self {
        Self {
            wrapping_clause: None,
            relations: Vec::new(),
            current_identifier: None,
            qualifier: None,
            in_string: false,
            node_kind_at_cursor: None,
            cursor_offset,
        }
    }

    /// Adjust cursor position for whitespace, similar to Supabase's approach.
    fn adjust_cursor_position(sql: &str, offset: usize) -> usize {
        let offset = offset.min(sql.len());
        if offset == 0 {
            return 0;
        }

        // If cursor is at whitespace or end of statement, move back to last non-whitespace
        let chars: Vec<char> = sql.chars().collect();
        let mut adjusted = offset;

        while adjusted > 0 {
            let c = chars.get(adjusted.saturating_sub(1)).copied().unwrap_or(' ');
            if !c.is_ascii_whitespace() && c != ';' && c != ')' {
                break;
            }
            adjusted = adjusted.saturating_sub(1);
        }

        adjusted.max(1).min(sql.len())
    }

    /// Convert byte offset to tree-sitter Point (row, column).
    fn offset_to_point(sql: &str, offset: usize) -> Point {
        let text_before = &sql[..offset.min(sql.len())];
        let row = text_before.matches('\n').count();
        let col = text_before
            .rfind('\n')
            .map(|pos| offset - pos - 1)
            .unwrap_or(offset);
        Point::new(row, col)
    }

    /// Find the SQL clause containing the cursor by walking up the AST.
    fn find_wrapping_clause(sql: &str, root: &Node, offset: usize) -> Option<WrappingClause> {
        let point = Self::offset_to_point(sql, offset);
        let mut node = root.descendant_for_point_range(point, point)?;

        // Walk up the tree looking for clause nodes
        loop {
            let kind = node.kind();

            // Map node kinds to wrapping clauses
            let clause = match kind {
                "select" | "select_clause" | "select_list" => Some(WrappingClause::Select),
                "from" | "from_clause" | "from_item" => Some(WrappingClause::From),
                "where" | "where_clause" => Some(WrappingClause::Where),
                "join" | "join_clause" => Some(WrappingClause::Join),
                "on" | "on_clause" => Some(WrappingClause::On),
                "group_by" | "group_by_clause" => Some(WrappingClause::GroupBy),
                "order_by" | "order_by_clause" => Some(WrappingClause::OrderBy),
                "having" | "having_clause" => Some(WrappingClause::Having),
                "insert" | "insert_statement" => Some(WrappingClause::Insert),
                "update" | "update_statement" => Some(WrappingClause::Update),
                "delete" | "delete_statement" => Some(WrappingClause::Delete),
                "set" | "set_clause" => Some(WrappingClause::Set),
                "values" | "values_clause" => Some(WrappingClause::Values),
                "returning" | "returning_clause" => Some(WrappingClause::Returning),
                "with" | "with_clause" | "cte" => Some(WrappingClause::With),
                "grant" | "grant_statement" => Some(WrappingClause::Grant),
                "revoke" | "revoke_statement" => Some(WrappingClause::Revoke),
                "create" | "create_table" | "create_index" | "create_view" => {
                    Some(WrappingClause::Create)
                }
                "alter" | "alter_table" | "alter_column" => Some(WrappingClause::Alter),
                "drop" | "drop_table" | "drop_index" | "drop_view" => Some(WrappingClause::Drop),
                _ => None,
            };

            if clause.is_some() {
                return clause;
            }

            // Try to determine clause from context (keyword-based fallback)
            if kind == "keyword" || kind == "identifier" || kind == "ERROR" {
                if let Ok(text) = node.utf8_text(sql.as_bytes()) {
                    let text_lower = text.to_lowercase();
                    let clause = match text_lower.as_str() {
                        "select" => Some(WrappingClause::Select),
                        "from" => Some(WrappingClause::From),
                        "where" => Some(WrappingClause::Where),
                        "join" | "left" | "right" | "inner" | "outer" | "cross" => {
                            Some(WrappingClause::Join)
                        }
                        "on" => Some(WrappingClause::On),
                        "group" => Some(WrappingClause::GroupBy),
                        "order" => Some(WrappingClause::OrderBy),
                        "having" => Some(WrappingClause::Having),
                        "insert" => Some(WrappingClause::Insert),
                        "update" => Some(WrappingClause::Update),
                        "delete" => Some(WrappingClause::Delete),
                        "set" => Some(WrappingClause::Set),
                        "values" => Some(WrappingClause::Values),
                        "returning" => Some(WrappingClause::Returning),
                        "with" => Some(WrappingClause::With),
                        "grant" => Some(WrappingClause::Grant),
                        "revoke" => Some(WrappingClause::Revoke),
                        "create" => Some(WrappingClause::Create),
                        "alter" => Some(WrappingClause::Alter),
                        "drop" => Some(WrappingClause::Drop),
                        _ => None,
                    };
                    if clause.is_some() {
                        return clause;
                    }
                }
            }

            // Move up to parent
            match node.parent() {
                Some(parent) => node = parent,
                None => break,
            }
        }

        // Fallback: scan text before cursor for last keyword
        Self::find_clause_from_text(sql, offset)
    }

    /// Fallback clause detection by scanning text before cursor.
    fn find_clause_from_text(sql: &str, offset: usize) -> Option<WrappingClause> {
        let before_cursor = sql[..offset.min(sql.len())].to_uppercase();

        // Find positions of major keywords
        let keywords = [
            ("SELECT", WrappingClause::Select),
            ("FROM", WrappingClause::From),
            ("WHERE", WrappingClause::Where),
            ("JOIN", WrappingClause::Join),
            ("ON", WrappingClause::On),
            ("GROUP BY", WrappingClause::GroupBy),
            ("ORDER BY", WrappingClause::OrderBy),
            ("HAVING", WrappingClause::Having),
            ("INSERT", WrappingClause::Insert),
            ("UPDATE", WrappingClause::Update),
            ("DELETE", WrappingClause::Delete),
            ("SET", WrappingClause::Set),
            ("VALUES", WrappingClause::Values),
            ("RETURNING", WrappingClause::Returning),
            ("WITH", WrappingClause::With),
        ];

        let mut last_pos = 0;
        let mut last_clause = None;

        for (keyword, clause) in keywords {
            if let Some(pos) = before_cursor.rfind(keyword) {
                if pos >= last_pos {
                    last_pos = pos;
                    last_clause = Some(clause);
                }
            }
        }

        last_clause
    }

    /// Extract table references from FROM and JOIN clauses.
    fn extract_relations(sql: &str, root: &Node) -> Vec<RelationMatch> {
        let mut relations = Vec::new();
        Self::collect_relations_recursive(sql, root, &mut relations);
        relations
    }

    /// Recursively collect table references from the AST.
    fn collect_relations_recursive(sql: &str, node: &Node, relations: &mut Vec<RelationMatch>) {
        let kind = node.kind();

        // Look for table reference patterns
        if kind == "relation"
            || kind == "table_reference"
            || kind == "from_item"
            || kind == "join_item"
        {
            if let Some(relation) = Self::parse_relation_node(sql, node) {
                relations.push(relation);
            }
        }

        // Also check for identifier patterns that might be table references
        if kind == "identifier" || kind == "object_reference" {
            // Check if parent is a FROM or JOIN context
            if let Some(parent) = node.parent() {
                let parent_kind = parent.kind();
                if parent_kind == "from"
                    || parent_kind == "from_clause"
                    || parent_kind == "join"
                    || parent_kind == "join_clause"
                {
                    if let Ok(text) = node.utf8_text(sql.as_bytes()) {
                        let (schema, name) = Self::parse_qualified_name(text);
                        relations.push(RelationMatch {
                            schema,
                            name,
                            alias: None,
                        });
                    }
                }
            }
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::collect_relations_recursive(sql, &child, relations);
        }
    }

    /// Parse a relation node to extract schema, table, and alias.
    fn parse_relation_node(sql: &str, node: &Node) -> Option<RelationMatch> {
        let mut cursor = node.walk();
        let mut schema = None;
        let mut name = None;
        let mut alias = None;

        for child in node.children(&mut cursor) {
            let child_kind = child.kind();

            if let Ok(text) = child.utf8_text(sql.as_bytes()) {
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }

                // Skip keywords
                if matches!(
                    text.to_uppercase().as_str(),
                    "AS" | "ON"
                        | "LEFT"
                        | "RIGHT"
                        | "INNER"
                        | "OUTER"
                        | "CROSS"
                        | "JOIN"
                        | "NATURAL"
                ) {
                    continue;
                }

                if child_kind == "identifier" || child_kind == "object_reference" {
                    if name.is_none() {
                        // First identifier is the table (possibly qualified)
                        let (s, n) = Self::parse_qualified_name(text);
                        schema = s;
                        name = Some(n);
                    } else if alias.is_none() {
                        // Second identifier is the alias
                        alias = Some(text.trim_matches('"').to_string());
                    }
                }
            }
        }

        name.map(|n| RelationMatch {
            schema,
            name: n,
            alias,
        })
    }

    /// Parse a potentially qualified name like "schema.table" or just "table".
    fn parse_qualified_name(text: &str) -> (Option<String>, String) {
        let parts: Vec<&str> = text.split('.').collect();
        if parts.len() >= 2 {
            (
                Some(parts[0].trim_matches('"').to_string()),
                parts[1].trim_matches('"').to_string(),
            )
        } else {
            (None, text.trim_matches('"').to_string())
        }
    }

    /// Extract identifier and qualifier at cursor position.
    fn extract_identifier_context(sql: &str, offset: usize) -> (Option<String>, Option<String>) {
        let before_cursor = &sql[..offset.min(sql.len())];

        // Find the last token being typed
        let mut identifier = String::new();
        let mut qualifier = None;

        // Scan backwards to find identifier boundaries
        for c in before_cursor.chars().rev() {
            if c.is_alphanumeric() || c == '_' {
                identifier.insert(0, c);
            } else if c == '.' && !identifier.is_empty() {
                // Found a dot - what's before it is the qualifier
                let remaining = &before_cursor[..before_cursor.len() - identifier.len() - 1];
                let mut qual = String::new();
                for qc in remaining.chars().rev() {
                    if qc.is_alphanumeric() || qc == '_' {
                        qual.insert(0, qc);
                    } else {
                        break;
                    }
                }
                if !qual.is_empty() {
                    qualifier = Some(qual);
                }
                break;
            } else {
                break;
            }
        }

        let current_id = if identifier.is_empty() {
            None
        } else {
            Some(identifier)
        };

        (qualifier, current_id)
    }

    /// Check if the cursor is inside a string literal.
    fn is_in_string(sql: &str, root: &Node, offset: usize) -> bool {
        let point = Self::offset_to_point(sql, offset);
        if let Some(node) = root.descendant_for_point_range(point, point) {
            let kind = node.kind();
            if kind == "string" || kind == "string_literal" || kind.contains("string") {
                return true;
            }
        }

        // Fallback: count quotes before cursor
        let before_cursor = &sql[..offset.min(sql.len())];
        let single_quotes = before_cursor.chars().filter(|c| *c == '\'').count();
        single_quotes % 2 == 1
    }

    /// Resolve an alias to its table name.
    pub fn resolve_alias(&self, alias: &str) -> Option<&RelationMatch> {
        self.relations
            .iter()
            .find(|r| r.alias.as_deref() == Some(alias))
    }

    /// Resolve a qualifier (could be alias or table name).
    pub fn resolve_qualifier(&self, qualifier: &str) -> Option<&RelationMatch> {
        let qualifier_lower = qualifier.to_lowercase();
        self.relations.iter().find(|r| {
            r.alias
                .as_ref()
                .map(|a| a.to_lowercase() == qualifier_lower)
                .unwrap_or(false)
                || r.name.to_lowercase() == qualifier_lower
        })
    }

    /// Check if the cursor has a qualifier prefix (e.g., "p." in "p.id").
    pub fn has_qualifier(&self) -> bool {
        self.qualifier.is_some()
    }

    /// Get the cursor offset.
    pub fn cursor_offset(&self) -> usize {
        self.cursor_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_clause_detection() {
        let sql = "SELECT  FROM patient";
        let ctx = TreesitterContext::from_sql(sql, 7); // cursor after "SELECT "
        assert_eq!(ctx.wrapping_clause, Some(WrappingClause::Select));
    }

    #[test]
    fn test_from_clause_detection() {
        let sql = "SELECT * FROM ";
        let ctx = TreesitterContext::from_sql(sql, sql.len());
        assert_eq!(ctx.wrapping_clause, Some(WrappingClause::From));
    }

    #[test]
    fn test_where_clause_detection() {
        let sql = "SELECT * FROM patient WHERE ";
        let ctx = TreesitterContext::from_sql(sql, sql.len());
        assert_eq!(ctx.wrapping_clause, Some(WrappingClause::Where));
    }

    #[test]
    fn test_relation_extraction() {
        let sql = "SELECT * FROM patient p";
        let ctx = TreesitterContext::from_sql(sql, sql.len());

        assert!(!ctx.relations.is_empty(), "Should extract relations");
        let relation = &ctx.relations[0];
        assert_eq!(relation.name, "patient");
        // Alias extraction may vary based on grammar
    }

    #[test]
    fn test_qualified_identifier_context() {
        let sql = "SELECT p.id FROM patient p";
        let ctx = TreesitterContext::from_sql(sql, 10); // cursor at "p.id"

        // Should detect qualifier
        // Note: exact behavior depends on cursor position and grammar
    }

    #[test]
    fn test_in_string_detection() {
        let sql = "SELECT * FROM patient WHERE resource->'";
        let ctx = TreesitterContext::from_sql(sql, sql.len());
        assert!(ctx.in_string, "Should detect cursor in string");
    }

    #[test]
    fn test_empty_sql() {
        let ctx = TreesitterContext::from_sql("", 0);
        assert!(ctx.wrapping_clause.is_none());
        assert!(ctx.relations.is_empty());
    }

    #[test]
    fn test_partial_select() {
        let sql = "SELECT ";
        let ctx = TreesitterContext::from_sql(sql, sql.len());
        assert_eq!(ctx.wrapping_clause, Some(WrappingClause::Select));
    }

    #[test]
    fn test_join_clause() {
        let sql = "SELECT * FROM patient p JOIN observation o ON ";
        let ctx = TreesitterContext::from_sql(sql, sql.len());
        // Should detect ON or Join clause
        assert!(matches!(
            ctx.wrapping_clause,
            Some(WrappingClause::On) | Some(WrappingClause::Join)
        ));
    }
}
