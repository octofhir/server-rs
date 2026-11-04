//! The SQL on FHIR mode of the PostgreSQL language server.
//!
//! A SQLQuery or SQLView reads nothing but its dependencies, each under its
//! `relatedArtifact.label`, calls only the functions of
//! [`octofhir_sof::guard::ALLOWED_FUNCTIONS`] and names its parameters
//! `:name`. In this mode the language server knows exactly that: the client
//! sends the query's relations (with the columns `$sql-describe` resolved)
//! and declared parameters as `workspace/didChangeConfiguration` settings,
//!
//! ```json
//! {"sof": {"relations": [{"name": "patient", "kind": "ViewDefinition",
//!                         "columns": [{"name": "id", "type": "text"}]}],
//!          "parameters": [{"name": "from_date", "type": "date"}]}}
//! ```
//!
//! and completion, diagnostics and the sandbox check ([`guard::check`]) work
//! against them. The database catalog is never consulted, so nothing of the
//! server's own tables is exposed.

use std::collections::{HashMap, HashSet};

use banshee_completion::types::{
    ColumnInfo, CompletionItem, CompletionItemKind, FunctionInfo, JsonbSchema, TableInfo, TableType,
};
use banshee_completion::{FunctionProvider, SchemaProvider};
use banshee_hir::{
    ColumnInfo as HirColumnInfo, SchemaProvider as HirSchemaProvider, TableInfo as HirTableInfo,
    TableType as HirTableType,
};
use banshee_syntax::{NodeOrToken, SyntaxKind};
use octofhir_sof::guard;
use octofhir_sof::library::DeclaredParameter;
use serde::Deserialize;
use serde_json::Value;
use text_size::{TextRange, TextSize};

use super::pg_server::parse_data_type;

/// What a SQL on FHIR query may read: its relations and its parameters.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct SofCatalog {
    #[serde(default)]
    relations: Vec<SofRelation>,
    #[serde(default)]
    parameters: Vec<SofParameter>,
}

/// A dependency of the query, under its label.
#[derive(Clone, Debug, Deserialize)]
struct SofRelation {
    name: String,
    /// ViewDefinition, SQLView, ValueSet or ConceptMap, for the completion
    /// detail.
    #[serde(default)]
    kind: Option<String>,
    /// Columns with their PostgreSQL types; empty when not known.
    #[serde(default)]
    columns: Vec<SofColumn>,
}

#[derive(Clone, Debug, Deserialize)]
struct SofColumn {
    name: String,
    #[serde(rename = "type", default)]
    data_type: String,
}

/// A declared `Library.parameter`.
#[derive(Clone, Debug, Deserialize)]
struct SofParameter {
    name: String,
    /// FHIR type (`string`, `date`, ...).
    #[serde(rename = "type", default)]
    fhir_type: String,
}

/// A problem found in the SQL, by byte range.
pub struct SofDiagnostic {
    pub range: TextRange,
    pub message: String,
    pub source: &'static str,
}

impl SofCatalog {
    /// The catalog in `workspace/didChangeConfiguration` settings (`sof`).
    pub fn from_settings(settings: &Value) -> Option<Self> {
        serde_json::from_value(settings.get("sof")?.clone()).ok()
    }

    fn relation(&self, name: &str) -> Option<&SofRelation> {
        self.relations
            .iter()
            .find(|relation| relation.name.eq_ignore_ascii_case(name))
    }

    /// Whether every relation's columns are known, so that a column the
    /// analysis cannot find is really missing.
    pub fn columns_known(&self) -> bool {
        self.relations
            .iter()
            .all(|relation| !relation.columns.is_empty())
    }

    fn declared(&self) -> HashMap<String, &'static str> {
        self.parameters
            .iter()
            .filter_map(|parameter| {
                let declared = DeclaredParameter {
                    name: parameter.name.clone(),
                    fhir_type: parameter.fhir_type.clone(),
                    required: false,
                };
                Some((parameter.name.clone(), declared.sql_type()?))
            })
            .collect()
    }

    /// `text` with each `:name` parameter replaced by `$1` and spaces of the
    /// same length, so the PostgreSQL parser and analysis read it as a
    /// placeholder and every offset stays where it was; with a diagnostic
    /// for each `:name` that is not declared.
    pub fn mask_parameters(&self, text: &str) -> (String, Vec<SofDiagnostic>) {
        let declared: HashSet<&str> = self
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        let mut masked = text.to_owned();
        let mut diagnostics = Vec::new();
        for (range, name) in parameter_references(text) {
            let start = usize::from(range.start());
            let end = usize::from(range.end());
            let placeholder = format!("$1{}", " ".repeat(end - start - 2));
            masked.replace_range(start..end, &placeholder);
            if !declared.contains(name.as_str()) {
                diagnostics.push(SofDiagnostic {
                    range,
                    message: format!(
                        "Parameter :{name} is not declared; add it to the query's parameters"
                    ),
                    source: "sof",
                });
            }
        }
        (masked, diagnostics)
    }

    /// The sandbox check `$sql-run` applies before running the SQL, as a
    /// diagnostic over the first line when it fails.
    pub fn guard_diagnostic(&self, text: &str) -> Option<SofDiagnostic> {
        if text.trim().is_empty() {
            return None;
        }
        let relations: HashSet<String> = self
            .relations
            .iter()
            .map(|relation| relation.name.to_ascii_lowercase())
            .collect();
        let message = guard::check(text, &relations, &self.declared()).err()?;
        let first_line = text.find('\n').unwrap_or(text.len());
        Some(SofDiagnostic {
            range: TextRange::new(TextSize::new(0), TextSize::new(first_line as u32)),
            message,
            source: "sof-guard",
        })
    }

    /// The declared parameters, when the cursor at `offset` follows `:` (and
    /// not a `::` cast).
    pub fn parameter_completions(&self, text: &str, offset: usize) -> Option<Vec<CompletionItem>> {
        let before = text.get(..offset)?;
        let word_start = before
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |index| index + 1);
        let prefix = before[..word_start].strip_suffix(':')?;
        if prefix.ends_with(':') {
            return None;
        }
        Some(
            self.parameters
                .iter()
                .map(|parameter| {
                    CompletionItem::new(CompletionItemKind::Alias, parameter.name.clone())
                        .with_detail(format!("parameter · {}", parameter.fhir_type))
                })
                .collect(),
        )
    }
}

/// Each `:name` in `text` (a `:` directly followed by a name, not part of a
/// `::` cast) with its byte range, colon included.
fn parameter_references(text: &str) -> Vec<(TextRange, String)> {
    let parse = banshee_parser::parse(text);
    let root = parse.syntax();
    let tokens: Vec<_> = root
        .descendants_with_tokens()
        .filter_map(|element| match element {
            NodeOrToken::Token(token) => Some(token),
            NodeOrToken::Node(_) => None,
        })
        .collect();
    tokens
        .windows(2)
        .filter_map(|pair| {
            let (colon, name) = (&pair[0], &pair[1]);
            let adjacent = colon.text_range().end() == name.text_range().start();
            (colon.kind() == SyntaxKind::COLON && adjacent && is_name(name.text())).then(|| {
                (
                    TextRange::new(colon.text_range().start(), name.text_range().end()),
                    name.text().to_owned(),
                )
            })
        })
        .collect()
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Completion over a [`SofCatalog`].
pub struct SofSchemaProvider<'a>(pub &'a SofCatalog);

impl SchemaProvider for SofSchemaProvider<'_> {
    fn tables(&self) -> Vec<TableInfo> {
        self.0
            .relations
            .iter()
            .map(|relation| {
                let info = TableInfo::new(relation.name.clone()).with_type(TableType::View);
                match &relation.kind {
                    Some(kind) => info.with_description(format!("{kind} dependency")),
                    None => info,
                }
            })
            .collect()
    }

    fn columns(&self, _schema: Option<&str>, table: &str) -> Vec<ColumnInfo> {
        self.0
            .relation(table)
            .map(|relation| {
                relation
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(index, column)| {
                        ColumnInfo::new(column.name.clone(), column.data_type.clone())
                            .with_ordinal(index)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn jsonb_schema(
        &self,
        _schema: Option<&str>,
        _table: &str,
        _column: &str,
    ) -> Option<JsonbSchema> {
        None
    }
}

impl FunctionProvider for SofSchemaProvider<'_> {
    fn functions(&self) -> Vec<FunctionInfo> {
        guard::ALLOWED_FUNCTIONS
            .iter()
            .map(|name| FunctionInfo::new(*name, ""))
            .collect()
    }
}

/// Semantic analysis over a [`SofCatalog`].
pub struct SofHirProvider<'a>(pub &'a SofCatalog);

impl HirSchemaProvider for SofHirProvider<'_> {
    fn lookup_table(&self, schema: Option<&str>, name: &str) -> Option<HirTableInfo> {
        if schema.is_some() {
            return None;
        }
        self.0.relation(name).map(|relation| HirTableInfo {
            schema: None,
            name: relation.name.clone(),
            table_type: HirTableType::View,
        })
    }

    fn lookup_columns(&self, schema: Option<&str>, table: &str) -> Vec<HirColumnInfo> {
        if schema.is_some() {
            return Vec::new();
        }
        self.0
            .relation(table)
            .map(|relation| {
                relation
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(ordinal, column)| HirColumnInfo {
                        name: column.name.clone(),
                        data_type: parse_data_type(&column.data_type),
                        nullable: true,
                        ordinal,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn schema_exists(&self, _schema: &str) -> bool {
        false
    }

    fn all_table_names(&self) -> Vec<String> {
        self.0
            .relations
            .iter()
            .map(|relation| relation.name.clone())
            .collect()
    }

    fn all_schema_names(&self) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog() -> SofCatalog {
        SofCatalog::from_settings(&json!({"sof": {
            "relations": [
                {"name": "patient", "kind": "ViewDefinition",
                 "columns": [{"name": "id", "type": "text"}, {"name": "birth_date", "type": "date"}]},
                {"name": "cvd_codes", "kind": "ValueSet",
                 "columns": [{"name": "system", "type": "text"}, {"name": "code", "type": "text"}]}
            ],
            "parameters": [{"name": "from_date", "type": "date"}]
        }}))
        .expect("catalog")
    }

    #[test]
    fn parameters_are_masked_in_place() {
        let sql = "SELECT id FROM patient WHERE birth_date >= :from_date AND id::text = :other";
        let (masked, diagnostics) = catalog().mask_parameters(sql);
        assert_eq!(masked.len(), sql.len());
        assert!(masked.contains("$1        "));
        assert!(masked.contains("id::text"), "a cast is not a parameter");
        assert!(!masked.contains(":from_date") && !masked.contains(":other"));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains(":other"));
        let start = usize::from(diagnostics[0].range.start());
        assert_eq!(
            &sql[start..usize::from(diagnostics[0].range.end())],
            ":other"
        );
    }

    #[test]
    fn the_guard_check_is_a_diagnostic() {
        let catalog = catalog();
        assert!(
            catalog
                .guard_diagnostic("SELECT p.id FROM patient p JOIN cvd_codes c ON c.code = p.id WHERE p.birth_date >= :from_date")
                .is_none()
        );
        let refused = catalog
            .guard_diagnostic("SELECT pg_sleep(1) FROM patient")
            .expect("refused");
        assert!(refused.message.contains("pg_sleep"), "{}", refused.message);
        let unknown = catalog
            .guard_diagnostic("SELECT * FROM observation")
            .expect("refused");
        assert_eq!(unknown.source, "sof-guard");
    }

    #[test]
    fn parameter_completion_follows_a_single_colon() {
        let catalog = catalog();
        let sql = "SELECT * FROM patient WHERE birth_date >= :fr";
        let items = catalog
            .parameter_completions(sql, sql.len())
            .expect("parameters");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "from_date");
        assert!(catalog.parameter_completions("SELECT id::te", 13).is_none());
        assert!(catalog.parameter_completions("SELECT id", 9).is_none());
    }

    #[test]
    fn only_the_relations_are_tables() {
        let catalog = catalog();
        let provider = SofHirProvider(&catalog);
        assert!(provider.lookup_table(None, "PATIENT").is_some());
        assert!(provider.lookup_table(None, "pg_class").is_none());
        assert!(provider.lookup_table(Some("public"), "patient").is_none());
        assert_eq!(provider.lookup_columns(None, "cvd_codes").len(), 2);
        let completion = SofSchemaProvider(&catalog);
        assert_eq!(completion.tables().len(), 2);
        assert!(completion.functions().iter().any(|f| f.name == "count"));
        assert!(!completion.functions().iter().any(|f| f.name == "pg_sleep"));
    }
}
