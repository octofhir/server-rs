//! SQLQuery and SQLView Libraries (SQL on FHIR v3, `Library` profiles).
//!
//! A SQLQuery holds one query over its dependencies; a SQLView is a reusable
//! query other Libraries read as a table, and declares no parameters. Both
//! carry the SQL as `content` attachments (`application/sql`, optionally per
//! `dialect`), name their dependencies (ViewDefinitions or SQLViews) in
//! `relatedArtifact` (`depends-on`, `label` = the table name in the SQL), and
//! a SQLQuery declares its inputs in `parameter`.

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

/// `Library.type` system of the SQL on FHIR Library profiles.
const LIBRARY_TYPES: &str = "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes";

/// The dialect this server runs.
const DIALECT: &str = "postgresql";

/// Which SQL Library profile a Library conforms to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SqlLibraryKind {
    Query,
    View,
}

/// A table the SQL reads: a ViewDefinition or a SQLView.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dependency {
    /// The table name in the SQL (`relatedArtifact.label`).
    pub label: String,
    /// Canonical URL of the ViewDefinition or SQLView, optionally `|version`.
    pub canonical: String,
}

/// An input of a SQLQuery (`Library.parameter`, `use = in`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeclaredParameter {
    pub name: String,
    /// FHIR type of the value (`string`, `date`, ...).
    pub fhir_type: String,
    /// The parameter must be supplied (`min` ≥ 1).
    pub required: bool,
}

impl DeclaredParameter {
    /// The `value[x]` field carrying a value of this type in `Parameters`.
    pub(crate) fn value_field(&self) -> String {
        let mut chars = self.fhir_type.chars();
        match chars.next() {
            Some(first) => format!("value{}{}", first.to_ascii_uppercase(), chars.as_str()),
            None => "value".into(),
        }
    }

    /// The PostgreSQL type the placeholder is cast to, or `None` for a type
    /// that has no scalar SQL counterpart.
    pub(crate) fn sql_type(&self) -> Option<&'static str> {
        Some(match self.fhir_type.as_str() {
            "string" | "code" | "id" | "uri" | "url" | "canonical" | "oid" | "uuid"
            | "markdown" | "base64Binary" => "text",
            "integer" | "positiveInt" | "unsignedInt" => "integer",
            "integer64" => "bigint",
            "decimal" => "numeric",
            "boolean" => "boolean",
            "date" => "date",
            "dateTime" | "instant" => "timestamptz",
            "time" => "time",
            _ => return None,
        })
    }
}

/// A SQLQuery or SQLView, with the SQL this server runs.
#[derive(Clone, Debug)]
pub(crate) struct SqlLibrary {
    pub kind: SqlLibraryKind,
    /// `url|version`, or the resource id, for messages.
    pub display: String,
    /// The PostgreSQL variant of the SQL, else the portable one.
    pub sql: String,
    pub dependencies: Vec<Dependency>,
    pub parameters: Vec<DeclaredParameter>,
}

/// The profile `library` claims by its type, if it is a SQL Library.
pub(crate) fn kind_of(library: &Value) -> Option<SqlLibraryKind> {
    library["type"]["coding"]
        .as_array()?
        .iter()
        .filter(|coding| coding["system"] == LIBRARY_TYPES)
        .find_map(|coding| match coding["code"].as_str() {
            Some("sql-query") => Some(SqlLibraryKind::Query),
            Some("sql-view") => Some(SqlLibraryKind::View),
            _ => None,
        })
}

impl SqlLibrary {
    /// Read a SQLQuery or SQLView. An error names what does not conform to
    /// the profile (the operation answers 422).
    pub(crate) fn from_json(library: &Value) -> Result<Self, String> {
        if library["resourceType"] != "Library" {
            return Err("The subject is not a Library".into());
        }
        let kind = kind_of(library)
            .ok_or_else(|| format!("Library type is not {LIBRARY_TYPES}#sql-query or #sql-view"))?;
        let display = match (library["url"].as_str(), library["version"].as_str()) {
            (Some(url), Some(version)) => format!("{url}|{version}"),
            (Some(url), None) => url.to_owned(),
            _ => format!("Library/{}", library["id"].as_str().unwrap_or("(inline)")),
        };
        let sql = select_sql(library)?;
        let dependencies = dependencies(library)?;
        let parameters = parameters(library)?;
        if kind == SqlLibraryKind::View && !parameters.is_empty() {
            return Err("A SQLView declares no parameters".into());
        }
        Ok(Self {
            kind,
            display,
            sql,
            dependencies,
            parameters,
        })
    }
}

/// The SQL to run: the `dialect=postgresql` attachment, else the portable
/// `application/sql` one; never a translation of another dialect.
fn select_sql(library: &Value) -> Result<String, String> {
    let contents = library["content"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if contents.is_empty() {
        return Err("The Library has no SQL content".into());
    }
    let mut portable = None;
    let mut native = None;
    for content in contents {
        let content_type = content["contentType"].as_str().unwrap_or_default();
        let (media_type, dialect) = media_type_and_dialect(content_type);
        if media_type != "application/sql" {
            return Err(format!(
                "Every content.contentType must be application/sql, found {content_type:?}"
            ));
        }
        match dialect.as_deref() {
            None => portable = Some(content),
            Some(DIALECT) => native = Some(content),
            Some(_) => {}
        }
    }
    let content = native.or(portable).ok_or_else(|| {
        format!("The Library has no SQL for the {DIALECT} dialect nor portable application/sql")
    })?;
    let data = content["data"]
        .as_str()
        .ok_or("SQL content requires data (base64)")?;
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| "SQL content data is not base64".to_owned())?;
    String::from_utf8(bytes).map_err(|_| "SQL content is not UTF-8 text".to_owned())
}

/// `application/sql;dialect=postgresql` → (`application/sql`, `postgresql`).
fn media_type_and_dialect(content_type: &str) -> (String, Option<String>) {
    let mut parts = content_type.split(';');
    let media_type = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let dialect = parts.find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key.trim().eq_ignore_ascii_case("dialect"))
            .then(|| value.trim().trim_matches('"').to_ascii_lowercase())
    });
    (media_type, dialect)
}

fn dependencies(library: &Value) -> Result<Vec<Dependency>, String> {
    let mut dependencies: Vec<Dependency> = Vec::new();
    for artifact in library["relatedArtifact"].as_array().into_iter().flatten() {
        if artifact["type"] != "depends-on" {
            continue;
        }
        let label = artifact["label"]
            .as_str()
            .ok_or("A depends-on relatedArtifact requires a label (its table name)")?;
        if !is_sql_name(label) {
            return Err(format!(
                "relatedArtifact.label {label:?} is not a valid SQL identifier ([A-Za-z][A-Za-z0-9_]*)"
            ));
        }
        let canonical = artifact["resource"]
            .as_str()
            .ok_or_else(|| format!("relatedArtifact {label} requires a resource canonical"))?;
        let label = label.to_ascii_lowercase();
        if dependencies.iter().any(|known| known.label == label) {
            return Err(format!("relatedArtifact.label {label} is not unique"));
        }
        dependencies.push(Dependency {
            label,
            canonical: canonical.to_owned(),
        });
    }
    Ok(dependencies)
}

fn parameters(library: &Value) -> Result<Vec<DeclaredParameter>, String> {
    let mut parameters: Vec<DeclaredParameter> = Vec::new();
    for parameter in library["parameter"].as_array().into_iter().flatten() {
        let name = parameter["name"]
            .as_str()
            .ok_or("Library.parameter requires a name")?;
        if parameter["use"] != "in" {
            return Err(format!("Parameter {name} must have use = in"));
        }
        if !is_sql_name(name) {
            return Err(format!(
                "Parameter name {name:?} is not a valid SQL identifier"
            ));
        }
        let fhir_type = parameter["type"]
            .as_str()
            .ok_or_else(|| format!("Parameter {name} requires a type"))?;
        if parameter["max"].as_str().is_some_and(|max| max != "1") {
            return Err(format!("Parameter {name} binds a single value (max = 1)"));
        }
        let declared = DeclaredParameter {
            name: name.to_owned(),
            fhir_type: fhir_type.to_owned(),
            required: parameter["min"].as_u64().is_some_and(|min| min >= 1),
        };
        if declared.sql_type().is_none() {
            return Err(format!(
                "Parameter {name} has type {fhir_type}, which has no SQL value"
            ));
        }
        if parameters.iter().any(|known| known.name == name) {
            return Err(format!("Parameter {name} is declared twice"));
        }
        parameters.push(declared);
    }
    Ok(parameters)
}

/// `sql-name`: `^[A-Za-z][A-Za-z0-9_]*$`.
fn is_sql_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn library(content: Value) -> Value {
        json!({
            "resourceType": "Library",
            "url": "http://example.org/Library/BpByPatient",
            "version": "1.0",
            "type": {"coding": [{"system": LIBRARY_TYPES, "code": "sql-query"}]},
            "status": "active",
            "relatedArtifact": [
                {"type": "depends-on", "label": "patient",
                 "resource": "http://example.org/ViewDefinition/patient"},
                {"type": "documentation", "url": "http://example.org/doc"}
            ],
            "parameter": [
                {"name": "from_date", "use": "in", "type": "date", "min": 1}
            ],
            "content": content
        })
    }

    fn sql(text: &str, content_type: &str) -> Value {
        json!({"contentType": content_type, "data": STANDARD.encode(text)})
    }

    #[test]
    fn a_sql_query_reads_with_its_dependencies_and_parameters() {
        let parsed =
            SqlLibrary::from_json(&library(json!([sql("SELECT 1", "application/sql")]))).unwrap();
        assert_eq!(parsed.kind, SqlLibraryKind::Query);
        assert_eq!(parsed.display, "http://example.org/Library/BpByPatient|1.0");
        assert_eq!(parsed.sql, "SELECT 1");
        assert_eq!(
            parsed.dependencies,
            [Dependency {
                label: "patient".into(),
                canonical: "http://example.org/ViewDefinition/patient".into()
            }]
        );
        assert_eq!(parsed.parameters[0].value_field(), "valueDate");
        assert!(parsed.parameters[0].required);
    }

    #[test]
    fn the_postgresql_variant_wins_and_other_dialects_are_never_used() {
        let parsed = SqlLibrary::from_json(&library(json!([
            sql("SELECT 'portable'", "application/sql"),
            sql("SELECT 'duck'", "application/sql;dialect=duckdb"),
            sql("SELECT 'pg'", "application/sql; dialect=postgresql")
        ])))
        .unwrap();
        assert_eq!(parsed.sql, "SELECT 'pg'");
        let error = SqlLibrary::from_json(&library(json!([sql(
            "SELECT 'duck'",
            "application/sql;dialect=duckdb"
        )])))
        .unwrap_err();
        assert!(error.contains("postgresql"), "{error}");
    }

    #[test]
    fn non_conforming_libraries_are_described() {
        let mut not_sql = library(json!([sql("SELECT 1", "application/sql")]));
        not_sql["type"] = json!({"coding": [{"code": "logic-library"}]});
        assert!(SqlLibrary::from_json(&not_sql).is_err());

        let cql = library(json!([sql("define X: 1", "text/cql")]));
        assert!(
            SqlLibrary::from_json(&cql)
                .unwrap_err()
                .contains("application/sql")
        );

        let mut bad_label = library(json!([sql("SELECT 1", "application/sql")]));
        bad_label["relatedArtifact"][0]["label"] = json!("patient; DROP");
        assert!(
            SqlLibrary::from_json(&bad_label)
                .unwrap_err()
                .contains("SQL identifier")
        );

        let mut view_with_parameters = library(json!([sql("SELECT 1", "application/sql")]));
        view_with_parameters["type"]["coding"][0]["code"] = json!("sql-view");
        assert!(
            SqlLibrary::from_json(&view_with_parameters)
                .unwrap_err()
                .contains("no parameters")
        );
    }
}
