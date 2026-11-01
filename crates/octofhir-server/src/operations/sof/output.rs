//! Output formats of `$viewdefinition-run` and `$sql-run`.
//!
//! `ndjson` (default), `json` and `csv` return the rows as a Binary in the
//! format's media type. `fhir` returns a `Parameters` with one `row` per
//! result row and one part per non-null column, typed by the SQL to FHIR
//! mapping of SQL on FHIR v3 (`$sql-run`, "Output Formats").

use base64::{Engine, engine::general_purpose::STANDARD};
use octofhir_sof::output::{CsvWriter, OutputWriter, get_writer};
use octofhir_sof::{ColumnInfo, ColumnType, ViewResult};
use serde_json::{Map, Value, json};

use crate::operations::OperationError;

/// The requested representation of the rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Ndjson,
    Json,
    Csv { header: bool },
    Fhir,
}

impl OutputFormat {
    /// `_format` (default ndjson); `header` applies to csv.
    pub(crate) fn parse(format: Option<&str>, header: bool) -> Result<Self, OperationError> {
        let Some(format) = format else {
            return Ok(Self::Ndjson);
        };
        match format.to_ascii_lowercase().as_str() {
            "ndjson" | "application/x-ndjson" | "application/ndjson" => Ok(Self::Ndjson),
            "json" | "application/json" => Ok(Self::Json),
            "csv" | "text/csv" => Ok(Self::Csv { header }),
            "fhir" => Ok(Self::Fhir),
            other => Err(OperationError::InvalidParameters(format!(
                "_format {other} is not supported; use ndjson, json, csv or fhir"
            ))),
        }
    }
}

/// How a column's values become `value[x]` in the `fhir` format.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FhirValue {
    /// Every value has this `value[x]` field.
    Field(&'static str),
    /// PostgreSQL `bytea`: `\x…` hex text, written as base64Binary.
    Bytea,
    /// A JSON value (untyped view column, `json`/`jsonb`): a scalar maps by
    /// its JSON type; an object or array has no FHIR primitive.
    JsonScalar,
    /// An SQL type without a FHIR mapping (interval, array, ...): 422 with
    /// `_format=fhir`.
    Unsupported(String),
}

/// A result column.
#[derive(Clone, Debug)]
pub(crate) struct OutputColumn {
    pub name: String,
    /// Drives the text writers (csv quoting and number rendering).
    column_type: ColumnType,
    fhir: FhirValue,
}

impl OutputColumn {
    /// A ViewDefinition column.
    pub(crate) fn from_view(column: &ColumnInfo) -> Self {
        let fhir = match column.col_type {
            ColumnType::String => FhirValue::Field("valueString"),
            ColumnType::Integer => FhirValue::Field("valueInteger"),
            ColumnType::Integer64 => FhirValue::Field("valueInteger64"),
            ColumnType::Decimal => FhirValue::Field("valueDecimal"),
            ColumnType::Boolean => FhirValue::Field("valueBoolean"),
            ColumnType::Date => FhirValue::Field("valueDate"),
            ColumnType::DateTime => FhirValue::Field("valueDateTime"),
            ColumnType::Instant => FhirValue::Field("valueInstant"),
            ColumnType::Time => FhirValue::Field("valueTime"),
            ColumnType::Base64Binary => FhirValue::Field("valueBase64Binary"),
            ColumnType::Json => FhirValue::JsonScalar,
        };
        Self {
            name: column.name.clone(),
            column_type: column.col_type,
            fhir,
        }
    }

    /// A result column of an SQL query, by its PostgreSQL type name.
    pub(crate) fn from_sql(name: &str, sql_type: &str) -> Self {
        let (column_type, fhir) = match sql_type {
            "bool" | "boolean" => (ColumnType::Boolean, FhirValue::Field("valueBoolean")),
            "int2" | "int4" | "smallint" | "integer" => {
                (ColumnType::Integer, FhirValue::Field("valueInteger"))
            }
            "int8" | "bigint" => (ColumnType::Integer64, FhirValue::Field("valueInteger64")),
            "numeric" | "float4" | "float8" | "real" | "double precision" => {
                (ColumnType::Decimal, FhirValue::Field("valueDecimal"))
            }
            "text" | "varchar" | "bpchar" | "char" | "name" | "character varying" => {
                (ColumnType::String, FhirValue::Field("valueString"))
            }
            "bytea" => (ColumnType::Base64Binary, FhirValue::Bytea),
            "date" => (ColumnType::Date, FhirValue::Field("valueDate")),
            "time" | "timetz" => (ColumnType::Time, FhirValue::Field("valueTime")),
            "timestamp" => (ColumnType::DateTime, FhirValue::Field("valueDateTime")),
            "timestamptz" => (ColumnType::Instant, FhirValue::Field("valueInstant")),
            "json" | "jsonb" => (ColumnType::Json, FhirValue::JsonScalar),
            other => (ColumnType::Json, FhirValue::Unsupported(other.to_owned())),
        };
        Self {
            name: name.to_owned(),
            column_type,
            fhir,
        }
    }

    fn info(&self) -> ColumnInfo {
        ColumnInfo::new(self.name.clone(), self.column_type)
    }
}

/// The operation result: a Binary for the text formats, a Parameters for
/// `fhir`. `max_bytes` bounds the rendered size.
pub(crate) fn render(
    format: OutputFormat,
    columns: &[OutputColumn],
    rows: &[Value],
    max_bytes: usize,
) -> Result<Value, OperationError> {
    let too_large = || {
        OperationError::InvalidParameters("The output exceeds the configured byte budget".into())
    };
    if format == OutputFormat::Fhir {
        let parameters = fhir_rows(columns, rows)?;
        let size = serde_json::to_vec(&parameters)
            .map_err(|error| OperationError::Internal(error.to_string()))?
            .len();
        return if size > max_bytes {
            Err(too_large())
        } else {
            Ok(parameters)
        };
    }
    let writer: Box<dyn OutputWriter> = match format {
        OutputFormat::Csv { header } => Box::new(CsvWriter::new().with_header(header)),
        OutputFormat::Json => {
            get_writer("json").map_err(|error| OperationError::Internal(error.to_string()))?
        }
        _ => get_writer("ndjson").map_err(|error| OperationError::Internal(error.to_string()))?,
    };
    let data = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .map(|column| row.get(&column.name).cloned().unwrap_or(Value::Null))
                .collect()
        })
        .collect::<Vec<Vec<Value>>>();
    let result = ViewResult {
        columns: columns.iter().map(OutputColumn::info).collect(),
        row_count: data.len(),
        data,
    };
    let mut bytes = Vec::new();
    writer
        .write(&result, &mut bytes)
        .map_err(|error| OperationError::Internal(format!("Failed to write rows: {error}")))?;
    if bytes.len() > max_bytes {
        return Err(too_large());
    }
    Ok(json!({
        "resourceType": "Binary",
        "contentType": writer.content_type(),
        "data": STANDARD.encode(bytes),
    }))
}

/// `Parameters` with a `row` per row; a null value has no part.
fn fhir_rows(columns: &[OutputColumn], rows: &[Value]) -> Result<Value, OperationError> {
    if let Some(column) = columns
        .iter()
        .find(|column| matches!(column.fhir, FhirValue::Unsupported(_)))
        && let FhirValue::Unsupported(sql_type) = &column.fhir
    {
        return Err(unprocessable(format!(
            "Column {} has SQL type {sql_type}, which has no FHIR value type; cast it in the SQL",
            column.name
        )));
    }
    let mut parameters = Vec::with_capacity(rows.len());
    for row in rows {
        let mut parts = Vec::with_capacity(columns.len());
        for column in columns {
            let value = row.get(&column.name).unwrap_or(&Value::Null);
            if value.is_null() {
                continue;
            }
            let (field, value) = fhir_value(column, value)?;
            let mut part = Map::new();
            part.insert("name".into(), Value::String(column.name.clone()));
            part.insert(field.into(), value);
            parts.push(Value::Object(part));
        }
        parameters.push(json!({"name": "row", "part": parts}));
    }
    let mut result = json!({"resourceType": "Parameters"});
    if !parameters.is_empty() {
        result["parameter"] = Value::Array(parameters);
    }
    Ok(result)
}

fn fhir_value(
    column: &OutputColumn,
    value: &Value,
) -> Result<(&'static str, Value), OperationError> {
    match &column.fhir {
        FhirValue::Field("valueDateTime") => match value.as_str() {
            // `timestamp` has no zone; a FHIR dateTime with a time needs one.
            Some(text) if text.contains('T') && !has_zone(text) => {
                Ok(("valueDateTime", Value::String(format!("{text}Z"))))
            }
            _ => Ok(("valueDateTime", value.clone())),
        },
        FhirValue::Field(field) => Ok((field, value.clone())),
        FhirValue::Bytea => {
            let hex = value.as_str().and_then(|text| text.strip_prefix("\\x"));
            let bytes = hex.and_then(decode_hex).ok_or_else(|| {
                OperationError::Internal(format!("Column {} is not bytea hex", column.name))
            })?;
            Ok(("valueBase64Binary", Value::String(STANDARD.encode(bytes))))
        }
        FhirValue::JsonScalar => match value {
            Value::Bool(_) => Ok(("valueBoolean", value.clone())),
            Value::Number(number) if number.is_i64() || number.is_u64() => {
                Ok(("valueInteger", value.clone()))
            }
            Value::Number(_) => Ok(("valueDecimal", value.clone())),
            Value::String(_) => Ok(("valueString", value.clone())),
            _ => Err(unprocessable(format!(
                "Column {} holds a JSON object or array, which has no FHIR value type",
                column.name
            ))),
        },
        FhirValue::Unsupported(_) => unreachable!("rejected before rendering"),
    }
}

fn has_zone(text: &str) -> bool {
    let time = text.split_once('T').map_or("", |(_, time)| time);
    time.ends_with('Z') || time.contains('+') || time.contains('-')
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok())
        .collect()
}

fn unprocessable(message: String) -> OperationError {
    OperationError::Api(octofhir_api::ApiError::unprocessable_entity(message, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_ndjson_by_default_and_the_four_named_ones() {
        assert_eq!(
            OutputFormat::parse(None, true).unwrap(),
            OutputFormat::Ndjson
        );
        for format in [
            "ndjson",
            "json",
            "csv",
            "text/csv",
            "fhir",
            "application/x-ndjson",
        ] {
            assert!(OutputFormat::parse(Some(format), true).is_ok(), "{format}");
        }
        assert!(OutputFormat::parse(Some("parquet"), true).is_err());
    }

    #[test]
    fn rows_render_as_ndjson_in_column_order() {
        let columns = vec![
            OutputColumn::from_view(&ColumnInfo::new("id", ColumnType::String)),
            OutputColumn::from_view(&ColumnInfo::new("active", ColumnType::Boolean)),
        ];
        let rows = vec![json!({"active": true, "id": "p1"})];
        let binary = render(OutputFormat::Ndjson, &columns, &rows, usize::MAX).unwrap();
        let text =
            String::from_utf8(STANDARD.decode(binary["data"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(text.trim(), r#"{"id":"p1","active":true}"#);
    }

    #[test]
    fn fhir_rows_follow_the_sql_to_fhir_mapping_and_omit_nulls() {
        let columns = vec![
            OutputColumn::from_sql("id", "text"),
            OutputColumn::from_sql("n", "int8"),
            OutputColumn::from_sql("at", "timestamptz"),
            OutputColumn::from_sql("local", "timestamp"),
            OutputColumn::from_sql("blob", "bytea"),
            OutputColumn::from_sql("flag", "jsonb"),
        ];
        let rows = vec![json!({
            "id": "p1", "n": 3, "at": "2026-01-02T03:04:05+00:00",
            "local": "2026-01-02T03:04:05", "blob": "\\x6869", "flag": null
        })];
        let parameters = render(OutputFormat::Fhir, &columns, &rows, usize::MAX).unwrap();
        assert_eq!(
            parameters,
            json!({"resourceType": "Parameters", "parameter": [{"name": "row", "part": [
                {"name": "id", "valueString": "p1"},
                {"name": "n", "valueInteger64": 3},
                {"name": "at", "valueInstant": "2026-01-02T03:04:05+00:00"},
                {"name": "local", "valueDateTime": "2026-01-02T03:04:05Z"},
                {"name": "blob", "valueBase64Binary": "aGk="}
            ]}]})
        );
        assert_eq!(
            render(OutputFormat::Fhir, &columns, &[], usize::MAX).unwrap(),
            json!({"resourceType": "Parameters"})
        );
    }

    #[test]
    fn a_column_without_a_fhir_type_is_unprocessable_with_fhir_only() {
        let columns = vec![OutputColumn::from_sql("span", "interval")];
        let rows = vec![json!({"span": "1 day"})];
        assert!(render(OutputFormat::Fhir, &columns, &rows, usize::MAX).is_err());
        assert!(render(OutputFormat::Json, &columns, &rows, usize::MAX).is_ok());
    }
}
