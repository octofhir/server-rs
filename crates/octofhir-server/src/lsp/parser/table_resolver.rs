//! Table alias resolution for SQL queries.
//!
//! This module resolves table aliases, CTEs, and subqueries to support accurate
//! JSONB path completions when using table aliases.
//!
//! ## Supported Patterns
//!
//! - **Basic aliases**: `FROM patient p` → `p` maps to `patient`
//! - **Multiple JOINs**: `FROM patient p1 JOIN patient p2` → `p1`, `p2` both map to `patient`
//! - **CTEs**: `WITH recent AS (SELECT ...) SELECT * FROM recent` → `recent` maps to CTE
//! - **Subqueries**: `FROM (SELECT * FROM patient) p` → `p` maps to subquery

use std::collections::HashMap;
use tree_sitter::Node;

/// Table alias resolver for SQL queries.
///
/// Tracks all table references (direct tables, CTEs, subqueries) and their aliases
/// to enable accurate column and JSONB path resolution.
#[derive(Debug, Default)]
pub struct TableResolver {
    /// Map from alias to resolved table name
    /// Example: `p` → `patient`, `p1` → `patient`, `recent` → CTE name
    alias_map: HashMap<String, ResolvedTable>,
}

/// Represents a resolved table reference.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedTable {
    /// Direct table reference with optional schema
    Table {
        schema: Option<String>,
        name: String,
    },
    /// Common Table Expression (CTE)
    Cte {
        name: String,
        /// Column names if known from CTE definition
        columns: Vec<String>,
    },
    /// Subquery with optional known columns
    Subquery {
        /// Columns available from the subquery (if determinable)
        columns: Vec<String>,
    },
}

impl TableResolver {
    /// Create a new table resolver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve tables and aliases from a SQL query tree-sitter AST.
    ///
    /// Returns a resolver with all table references mapped to their sources.
    pub fn resolve(text: &str, tree: &tree_sitter::Tree) -> Self {
        let mut resolver = Self::new();
        let root = tree.root_node();

        // First pass: Extract CTEs (WITH clauses)
        resolver.extract_ctes(&root, text);

        // Second pass: Extract FROM/JOIN tables
        resolver.extract_from_clause_tables(&root, text);

        resolver
    }

    /// Get the resolved table for an alias or table name.
    ///
    /// Returns the table name if the alias exists, otherwise tries to use
    /// the input as a direct table name.
    pub fn get_table_name(&self, alias_or_table: &str) -> Option<&str> {
        self.alias_map.get(alias_or_table).and_then(|resolved| {
            match resolved {
                ResolvedTable::Table { name, .. } => Some(name.as_str()),
                ResolvedTable::Cte { name, .. } => Some(name.as_str()),
                ResolvedTable::Subquery { .. } => None, // Subqueries don't have table names
            }
        })
    }

    /// Get all known aliases.
    pub fn get_aliases(&self) -> Vec<&str> {
        self.alias_map.keys().map(String::as_str).collect()
    }

    /// Check if an identifier is a known alias.
    pub fn is_alias(&self, name: &str) -> bool {
        self.alias_map.contains_key(name)
    }

    // ============================================================================
    // Private Extraction Methods
    // ============================================================================

    /// Extract CTEs (Common Table Expressions) from WITH clauses.
    fn extract_ctes(&mut self, node: &Node, text: &str) {
        let mut cursor = node.walk();

        // Walk the tree looking for WITH clauses
        self.walk_for_ctes(&mut cursor, text);
    }

    /// Recursively walk tree looking for CTE definitions.
    fn walk_for_ctes(&mut self, cursor: &mut tree_sitter::TreeCursor, text: &str) {
        let node = cursor.node();

        // Check if this is a WITH clause
        if node.kind() == "with_query" {
            self.extract_cte_from_with_query(&node, text);
        }

        // Recurse into children
        if cursor.goto_first_child() {
            loop {
                self.walk_for_ctes(cursor, text);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
    }

    /// Extract a single CTE from a with_query node.
    fn extract_cte_from_with_query(&mut self, node: &Node, text: &str) {
        // Look for CTE name and definition
        // Tree-sitter structure: with_query has children with CTE definitions
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "cte" || child.kind() == "with_query_item" {
                    if let Some(cte_name) = self.extract_cte_name(&child, text) {
                        // Store CTE reference
                        self.alias_map.insert(
                            cte_name.clone(),
                            ResolvedTable::Cte {
                                name: cte_name,
                                columns: Vec::new(), // TODO: Extract column list if available
                            },
                        );
                    }
                }
            }
        }
    }

    /// Extract CTE name from a CTE definition node.
    fn extract_cte_name(&self, node: &Node, text: &str) -> Option<String> {
        // Look for identifier in CTE definition
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if matches!(child.kind(), "identifier" | "any_identifier") {
                    if let Ok(name) = child.utf8_text(text.as_bytes()) {
                        return Some(name.to_string());
                    }
                }
            }
        }
        None
    }

    /// Extract tables from FROM clause and JOINs.
    fn extract_from_clause_tables(&mut self, node: &Node, text: &str) {
        let mut cursor = node.walk();
        self.walk_for_from_clauses(&mut cursor, text);
    }

    /// Recursively walk tree looking for FROM clauses.
    fn walk_for_from_clauses(&mut self, cursor: &mut tree_sitter::TreeCursor, text: &str) {
        let node = cursor.node();

        // Check if this is a FROM clause
        if node.kind() == "from" {
            self.extract_from_clause(&node, text);
        }

        // Check for JOIN clauses
        if node.kind() == "join" || node.kind() == "join_clause" {
            self.extract_join_clause(&node, text);
        }

        // Recurse into children
        if cursor.goto_first_child() {
            loop {
                self.walk_for_from_clauses(cursor, text);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
    }

    /// Extract table reference from FROM clause.
    fn extract_from_clause(&mut self, node: &Node, text: &str) {
        // FROM clause structure: FROM table_name [AS] alias
        // Or: FROM (subquery) [AS] alias
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                match child.kind() {
                    "relation" => {
                        // Relation can contain either table_reference or subquery
                        // Check if it has a subquery child
                        let has_subquery = (0..child.child_count()).any(|j| {
                            child
                                .child(j as u32)
                                .map(|c| c.kind() == "subquery")
                                .unwrap_or(false)
                        });

                        if has_subquery {
                            // Handle subquery with alias
                            self.extract_subquery_from_relation(&child, text);
                        } else {
                            // Handle regular table reference
                            self.extract_table_reference(&child, text);
                        }
                    }
                    "table_reference" => {
                        self.extract_table_reference(&child, text);
                    }
                    _ => {
                        // Recurse to find nested table references
                        self.extract_from_clause(&child, text);
                    }
                }
            }
        }
    }

    /// Extract table reference from JOIN clause.
    fn extract_join_clause(&mut self, node: &Node, text: &str) {
        // JOIN structure similar to FROM
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                match child.kind() {
                    "relation" => {
                        // Check if it has a subquery child
                        let has_subquery = (0..child.child_count()).any(|j| {
                            child
                                .child(j as u32)
                                .map(|c| c.kind() == "subquery")
                                .unwrap_or(false)
                        });

                        if has_subquery {
                            self.extract_subquery_from_relation(&child, text);
                        } else {
                            self.extract_table_reference(&child, text);
                        }
                    }
                    "table_reference" => {
                        self.extract_table_reference(&child, text);
                    }
                    _ => {
                        // Recurse to find nested table references
                        self.extract_join_clause(&child, text);
                    }
                }
            }
        }
    }

    /// Extract table name and optional alias from a relation node.
    ///
    /// Tree structure:
    /// ```
    /// relation
    ///   table_reference
    ///     any_identifier ("patient")
    ///   alias
    ///     any_identifier ("p")
    /// ```
    fn extract_table_reference(&mut self, node: &Node, text: &str) {
        let mut table_name: Option<String> = None;
        let mut alias: Option<String> = None;

        // Walk children to find table_reference and alias
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                match child.kind() {
                    "table_reference" => {
                        // Extract table name from table_reference
                        for j in 0..child.child_count() {
                            if let Some(ident_node) = child.child(j as u32) {
                                if matches!(ident_node.kind(), "identifier" | "any_identifier") {
                                    if let Ok(name) = ident_node.utf8_text(text.as_bytes()) {
                                        table_name = Some(name.to_string());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    "alias" => {
                        // Extract alias name
                        for j in 0..child.child_count() {
                            if let Some(alias_node) = child.child(j as u32) {
                                if matches!(alias_node.kind(), "identifier" | "any_identifier") {
                                    if let Ok(name) = alias_node.utf8_text(text.as_bytes()) {
                                        alias = Some(name.to_string());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Register the table reference
        if let Some(table) = table_name {
            let resolved = ResolvedTable::Table {
                schema: None,
                name: table.clone(),
            };

            // If there's an alias, map alias → table
            if let Some(alias_name) = alias {
                self.alias_map.insert(alias_name, resolved.clone());
            }

            // Also map table name → table (for non-aliased references)
            self.alias_map.insert(table, resolved);
        }
    }

    /// Extract subquery and its alias from a relation node containing a subquery.
    ///
    /// Tree structure:
    /// ```
    /// relation
    ///   subquery
    ///     ...
    ///   alias
    ///     any_identifier ("p")
    /// ```
    fn extract_subquery_from_relation(&mut self, node: &Node, text: &str) {
        let mut alias: Option<String> = None;

        // Look for alias sibling
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "alias" {
                    // Extract alias name
                    for j in 0..child.child_count() {
                        if let Some(alias_node) = child.child(j as u32) {
                            if matches!(alias_node.kind(), "identifier" | "any_identifier") {
                                if let Ok(name) = alias_node.utf8_text(text.as_bytes()) {
                                    alias = Some(name.to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Register subquery alias
        if let Some(alias_name) = alias {
            self.alias_map.insert(
                alias_name,
                ResolvedTable::Subquery {
                    columns: Vec::new(), // TODO: Analyze subquery columns
                },
            );
        }
    }

    /// Extract alias name from an alias node.
    fn extract_alias_name(&self, node: &Node, text: &str) -> Option<String> {
        // Look for identifier in alias
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if matches!(child.kind(), "identifier" | "any_identifier") {
                    if let Ok(name) = child.utf8_text(text.as_bytes()) {
                        return Some(name.to_string());
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_sql(sql: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .expect("Failed to set language");
        parser.parse(sql, None).expect("Failed to parse SQL")
    }

    #[test]
    fn test_basic_table_alias() {
        let sql = "SELECT p.resource FROM patient p";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert!(resolver.is_alias("p"));
    }

    #[test]
    fn test_table_without_alias() {
        let sql = "SELECT resource FROM patient";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("patient"), Some("patient"));
    }

    #[test]
    fn test_multiple_joins_same_table() {
        let sql = "SELECT * FROM patient p1 JOIN patient p2 ON p1.id = p2.id";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p1"), Some("patient"));
        assert_eq!(resolver.get_table_name("p2"), Some("patient"));
    }

    #[test]
    fn test_alias_resolution_with_as_keyword() {
        // Test user-reported issue: "it try to resolve alias as TABLE name and not as alias"
        let sql = "SELECT * FROM patient as p WHERE p.resource->>'{}' = 'test'";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        // Verify p is recognized as an alias
        assert!(resolver.is_alias("p"), "p should be recognized as an alias");

        // Verify p maps to patient
        assert_eq!(
            resolver.get_table_name("p"),
            Some("patient"),
            "p should resolve to patient table"
        );

        // Verify patient is also in the map (for non-aliased references)
        assert_eq!(
            resolver.get_table_name("patient"),
            Some("patient"),
            "patient should also be accessible directly"
        );
    }

    #[test]
    fn test_cte_basic() {
        let sql = "WITH recent AS (SELECT * FROM patient) SELECT * FROM recent";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        // CTE should be recognized
        assert!(resolver.is_alias("recent"));
    }

    #[test]
    fn test_subquery_alias() {
        let sql = "SELECT * FROM (SELECT * FROM patient) p";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        // Subquery alias should be recognized
        assert!(resolver.is_alias("p"));
    }

    // ============================================================================
    // Comprehensive Test Suite (20+ scenarios)
    // ============================================================================

    #[test]
    fn test_left_join() {
        let sql = "SELECT * FROM patient p LEFT JOIN observation o ON p.id = o.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
    }

    #[test]
    fn test_inner_join() {
        let sql = "SELECT * FROM patient p INNER JOIN observation o ON p.id = o.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
    }

    #[test]
    fn test_multiple_different_tables() {
        let sql = "SELECT * FROM patient p JOIN observation o ON p.id = o.subject JOIN condition c ON p.id = c.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
        assert_eq!(resolver.get_table_name("c"), Some("condition"));
    }

    #[test]
    fn test_self_join() {
        let sql = "SELECT * FROM patient p1 JOIN patient p2 ON p1.id = p2.id";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p1"), Some("patient"));
        assert_eq!(resolver.get_table_name("p2"), Some("patient"));
        assert!(resolver.is_alias("p1"));
        assert!(resolver.is_alias("p2"));
    }

    #[test]
    fn test_triple_self_join() {
        let sql = "SELECT * FROM patient p1 JOIN patient p2 ON p1.id = p2.id JOIN patient p3 ON p2.id = p3.id";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p1"), Some("patient"));
        assert_eq!(resolver.get_table_name("p2"), Some("patient"));
        assert_eq!(resolver.get_table_name("p3"), Some("patient"));
    }

    #[test]
    fn test_mixed_aliased_and_non_aliased() {
        let sql = "SELECT * FROM patient p JOIN observation ON p.id = observation.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert_eq!(resolver.get_table_name("observation"), Some("observation"));
    }

    #[test]
    fn test_multiple_ctes() {
        let sql = "WITH recent AS (SELECT * FROM patient), active AS (SELECT * FROM observation) SELECT * FROM recent r JOIN active a ON r.id = a.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert!(resolver.is_alias("recent"));
        assert!(resolver.is_alias("active"));
        assert!(resolver.is_alias("r"));
        assert!(resolver.is_alias("a"));
    }

    #[test]
    fn test_cte_with_join() {
        let sql = "WITH recent AS (SELECT * FROM patient) SELECT * FROM recent r JOIN observation o ON r.id = o.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert!(resolver.is_alias("recent"));
        assert!(resolver.is_alias("r"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
    }

    #[test]
    fn test_nested_subqueries() {
        let sql = "SELECT * FROM (SELECT * FROM (SELECT * FROM patient) p1) p2";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        // Outer subquery alias
        assert!(resolver.is_alias("p2"));
        // Inner subquery alias (within the outer subquery)
        // Note: Currently we only capture top-level aliases, not nested ones
    }

    #[test]
    fn test_subquery_in_join() {
        let sql = "SELECT * FROM patient p JOIN (SELECT * FROM observation) o ON p.id = o.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert!(resolver.is_alias("o"));
    }

    #[test]
    fn test_complex_real_world_query() {
        let sql = r#"
            WITH recent_patients AS (
                SELECT * FROM patient WHERE created > NOW() - INTERVAL '30 days'
            )
            SELECT
                rp.resource->>'name' as name,
                o.resource->>'code' as observation_code
            FROM recent_patients rp
            LEFT JOIN observation o ON rp.id = o.resource->>'subject'
            WHERE rp.resource->>'active' = 'true'
        "#;
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert!(resolver.is_alias("recent_patients"));
        assert!(resolver.is_alias("rp"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
    }

    #[test]
    fn test_uppercase_sql() {
        let sql = "SELECT * FROM PATIENT P JOIN OBSERVATION O ON P.ID = O.SUBJECT";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        // Case should be preserved as-is
        assert_eq!(resolver.get_table_name("P"), Some("PATIENT"));
        assert_eq!(resolver.get_table_name("O"), Some("OBSERVATION"));
    }

    #[test]
    fn test_as_keyword_in_alias() {
        let sql = "SELECT * FROM patient AS p";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
    }

    #[test]
    fn test_cross_join() {
        let sql = "SELECT * FROM patient p CROSS JOIN observation o";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
    }

    #[test]
    fn test_right_join() {
        let sql = "SELECT * FROM patient p RIGHT JOIN observation o ON p.id = o.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
    }

    #[test]
    fn test_full_outer_join() {
        let sql = "SELECT * FROM patient p FULL OUTER JOIN observation o ON p.id = o.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
    }

    #[test]
    fn test_where_clause_with_aliases() {
        let sql = "SELECT p.resource FROM patient p WHERE p.resource->>'active' = 'true'";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
    }

    #[test]
    fn test_multiple_tables_no_join_keyword() {
        let sql = "SELECT * FROM patient p, observation o WHERE p.id = o.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        assert_eq!(resolver.get_table_name("p"), Some("patient"));
        assert_eq!(resolver.get_table_name("o"), Some("observation"));
    }

    #[test]
    fn test_get_aliases_method() {
        let sql = "SELECT * FROM patient p JOIN observation o ON p.id = o.subject";
        let tree = parse_sql(sql);
        let resolver = TableResolver::resolve(sql, &tree);

        let aliases = resolver.get_aliases();
        assert!(aliases.contains(&"p"));
        assert!(aliases.contains(&"o"));
        assert!(aliases.contains(&"patient"));
        assert!(aliases.contains(&"observation"));
        assert!(aliases.len() >= 4);
    }
}
