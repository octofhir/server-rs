// This filtering logic is derived from Supabase postgres-language-server
// https://github.com/supabase-community/postgres-language-server
// Licensed under MIT License
// Copyright (c) 2023 Philipp Steinrötter

//! Completion filtering based on tree-sitter AST context.
//!
//! This module determines which completions are relevant based on the SQL context
//! at the cursor position. It mirrors the Supabase postgres-language-server approach.

use pgls_treesitter::context::{TreesitterContext, WrappingClause, WrappingNode};

use super::schema_cache::{ColumnInfo, FunctionInfo, TableInfo};

/// Data about a completion item used for relevance filtering.
#[derive(Debug)]
pub enum CompletionRelevanceData<'a> {
    Table(&'a TableInfo),
    Column(&'a ColumnInfo),
    Function(&'a FunctionInfo),
    Schema(&'a str),
    Keyword(&'a str),
}

/// Filter that determines if a completion item is relevant in the current context.
pub struct CompletionFilter<'a> {
    data: CompletionRelevanceData<'a>,
}

impl<'a> From<CompletionRelevanceData<'a>> for CompletionFilter<'a> {
    fn from(value: CompletionRelevanceData<'a>) -> Self {
        Self { data: value }
    }
}

impl CompletionFilter<'_> {
    /// Check if this completion item is relevant in the given context.
    pub fn is_relevant(&self, ctx: &TreesitterContext) -> bool {
        // Keywords are always potentially relevant
        if matches!(self.data, CompletionRelevanceData::Keyword(_)) {
            return true;
        }

        // Check if we're in a completable context
        if !self.completable_context(ctx) {
            return false;
        }

        // Check specific node type first, then fall back to clause check
        if self.check_specific_node_type(ctx) {
            // If node type matched, also check invocation and qualifier
            return self.check_invocation(ctx) && self.check_mentioned_schema_or_alias(ctx);
        }

        // Fall back to clause-based filtering
        if self.check_clause(ctx) {
            return self.check_invocation(ctx) && self.check_mentioned_schema_or_alias(ctx);
        }

        false
    }

    /// Check if we're in a context where completions make sense.
    fn completable_context(&self, ctx: &TreesitterContext) -> bool {
        if ctx.wrapping_node_kind.is_none() && ctx.wrapping_clause_type.is_none() {
            return false;
        }

        let current_node_kind = ctx.node_under_cursor.kind();

        // Don't complete on keywords, operators, or ERROR nodes
        if current_node_kind.starts_with("keyword_")
            || current_node_kind == "="
            || current_node_kind == ","
            || current_node_kind == "ERROR"
        {
            return false;
        }

        // Handle literal nodes (quoted identifiers)
        if current_node_kind == "literal" {
            match self.data {
                CompletionRelevanceData::Column(_) => match ctx.wrapping_clause_type.as_ref() {
                    Some(WrappingClause::Select)
                    | Some(WrappingClause::Where)
                    | Some(WrappingClause::Join { .. })
                    | Some(WrappingClause::Update)
                    | Some(WrappingClause::Delete)
                    | Some(WrappingClause::Insert) => {
                        // the literal is probably a column
                    }
                    _ => return false,
                },
                _ => return false,
            }
        }

        // No autocompletions if we're defining an alias
        if ctx.node_under_cursor.kind() == "any_identifier"
            && ctx.history_ends_with(&["alias", "any_identifier"])
        {
            return false;
        }

        // No completions if there are two identifiers without a separator
        if ctx.node_under_cursor.prev_sibling().is_some_and(|p| {
            (p.kind() == "any_identifier" || p.kind() == "object_reference")
                && ctx.node_under_cursor.kind() == "any_identifier"
        }) {
            return false;
        }

        // No completions right after an asterisk
        if ctx.node_under_cursor.prev_sibling().is_some_and(|p| {
            p.kind() == "all_fields" && ctx.node_under_cursor.kind() == "any_identifier"
        }) {
            return false;
        }

        true
    }

    /// Check if the completion type matches the specific node type.
    fn check_specific_node_type(&self, ctx: &TreesitterContext) -> bool {
        let kind = ctx.node_under_cursor.kind();

        match kind {
            "column_identifier" => matches!(self.data, CompletionRelevanceData::Column(_)),
            "function_identifier" => matches!(self.data, CompletionRelevanceData::Function(_)),
            "schema_identifier" => matches!(self.data, CompletionRelevanceData::Schema(_)),
            "table_identifier" => matches!(self.data, CompletionRelevanceData::Table(_)),

            "any_identifier" => match self.data {
                CompletionRelevanceData::Column(_) => {
                    ctx.node_under_cursor_is_within_field(&[
                        "object_reference_1of1",
                        "object_reference_2of2",
                        "object_reference_3of3",
                        "column_reference_1of1",
                        "column_reference_2of2",
                        "column_reference_3of3",
                    ]) && !ctx.node_under_cursor_is_within_field(&["binary_expr_right"])
                }

                CompletionRelevanceData::Schema(_) => ctx.node_under_cursor_is_within_field(&[
                    "object_reference_1of1",
                    "object_reference_1of2",
                    "object_reference_1of3",
                    "type_reference_1of1",
                    "table_reference_1of1",
                    "column_reference_1of1",
                    "column_reference_1of2",
                    "function_reference_1of1",
                ]),

                CompletionRelevanceData::Function(_) => ctx.node_under_cursor_is_within_field(&[
                    "object_reference_1of1",
                    "object_reference_2of2",
                    "function_reference_1of1",
                ]),

                CompletionRelevanceData::Table(_) => ctx.node_under_cursor_is_within_field(&[
                    "object_reference_1of1",
                    "object_reference_1of2",
                    "object_reference_2of2",
                    "object_reference_2of3",
                    "table_reference_1of1",
                    "column_reference_1of1",
                    "column_reference_1of2",
                    "column_reference_2of2",
                ]),

                _ => false,
            },

            _ => false,
        }
    }

    /// Check if the completion type is appropriate for the current SQL clause.
    fn check_clause(&self, ctx: &TreesitterContext) -> bool {
        let Some(clause) = ctx.wrapping_clause_type.as_ref() else {
            return false;
        };

        match self.data {
            CompletionRelevanceData::Table(_) => match clause {
                WrappingClause::From | WrappingClause::Update => true,

                WrappingClause::Join { on_node: None } => true,
                WrappingClause::Join { on_node: Some(on) } => {
                    ctx.node_under_cursor.start_byte() < on.end_byte()
                }

                WrappingClause::Insert => {
                    ctx.wrapping_node_kind
                        .as_ref()
                        .is_none_or(|n| n != &WrappingNode::List)
                        && (ctx.before_cursor_matches_kind(&["keyword_into"])
                            || (ctx.before_cursor_matches_kind(&["."])
                                && ctx.history_ends_with(&["object_reference", "any_identifier"])))
                }

                WrappingClause::DropTable | WrappingClause::AlterTable => ctx
                    .before_cursor_matches_kind(&[
                        "keyword_exists",
                        "keyword_only",
                        "keyword_table",
                    ]),

                _ => false,
            },

            CompletionRelevanceData::Column(_) => match clause {
                WrappingClause::Select | WrappingClause::Update | WrappingClause::Delete => true,

                // Complete columns in JOIN clauses only after the ON keyword
                WrappingClause::Join { on_node: Some(on) } => {
                    ctx.node_under_cursor.start_byte() >= on.end_byte()
                }
                WrappingClause::Join { on_node: None } => false,

                WrappingClause::Insert => ctx
                    .wrapping_node_kind
                    .as_ref()
                    .is_some_and(|n| n == &WrappingNode::List),

                // Only autocomplete left side of binary expression in WHERE
                WrappingClause::Where => {
                    ctx.before_cursor_matches_kind(&["keyword_and", "keyword_where"])
                        || (ctx.before_cursor_matches_kind(&["field_qualifier"])
                            && ctx.history_ends_with(&["field", "any_identifier"]))
                }

                _ => false,
            },

            CompletionRelevanceData::Function(_) => matches!(
                clause,
                WrappingClause::From
                    | WrappingClause::Select
                    | WrappingClause::Where
                    | WrappingClause::Join { .. }
            ),

            CompletionRelevanceData::Schema(_) => match clause {
                WrappingClause::Select
                | WrappingClause::Join { .. }
                | WrappingClause::Update
                | WrappingClause::Delete => true,

                WrappingClause::Where => {
                    ctx.before_cursor_matches_kind(&["keyword_and", "keyword_where"])
                }

                WrappingClause::DropTable | WrappingClause::AlterTable => ctx
                    .before_cursor_matches_kind(&[
                        "keyword_exists",
                        "keyword_only",
                        "keyword_table",
                    ]),

                WrappingClause::Insert => {
                    ctx.wrapping_node_kind
                        .as_ref()
                        .is_none_or(|n| n != &WrappingNode::List)
                        && ctx.before_cursor_matches_kind(&["keyword_into"])
                }

                _ => false,
            },

            CompletionRelevanceData::Keyword(_) => true,
        }
    }

    /// Check if we're inside a function invocation (tables/columns not valid).
    fn check_invocation(&self, ctx: &TreesitterContext) -> bool {
        if !ctx.is_invocation {
            return true;
        }

        // Tables and columns are not valid inside function invocations
        match self.data {
            CompletionRelevanceData::Table(_) | CompletionRelevanceData::Column(_) => false,
            _ => true,
        }
    }

    /// Check if any qualifier matches the schema or alias.
    fn check_mentioned_schema_or_alias(&self, ctx: &TreesitterContext) -> bool {
        let Some(tail_qualifier) = ctx.tail_qualifier_sanitized() else {
            return true; // no qualifier = this check passes
        };

        match self.data {
            CompletionRelevanceData::Table(table) => table.schema == tail_qualifier,
            CompletionRelevanceData::Function(_func) => {
                // For now, allow all functions if there's a qualifier
                // In a full implementation, we'd check the function's schema
                true
            }
            CompletionRelevanceData::Column(col) => {
                // Check if the qualifier matches the table name or an alias
                let table = ctx
                    .get_mentioned_table_for_alias(&tail_qualifier)
                    .unwrap_or(&tail_qualifier);

                col.table_name == table.as_str()
            }
            // No schema suggestions if there already was one
            CompletionRelevanceData::Schema(_) => false,
            CompletionRelevanceData::Keyword(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Basic unit tests - full integration tests would require database setup
    #[test]
    fn test_completion_relevance_data_debug() {
        let table = TableInfo {
            schema: "public".to_string(),
            name: "patient".to_string(),
            table_type: "BASE TABLE".to_string(),
            is_fhir_table: true,
            fhir_resource_type: Some("Patient".to_string()),
        };
        let data = CompletionRelevanceData::Table(&table);
        assert!(format!("{:?}", data).contains("Table"));
    }
}
