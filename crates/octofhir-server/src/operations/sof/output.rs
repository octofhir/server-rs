//! Output formats of `$viewdefinition-run` and `$sql-run`.
//!
//! `ndjson` (default), `json`, `csv` and `parquet` return the rows as a
//! Binary in the format's media type, which the router answers with as the
//! raw payload, or as the serialized Binary resource when the client asked
//! for a FHIR representation (Common Operation Behavior, "Content
//! Negotiation": `_format` selects the format and wins over `Accept`;
//! without it `Accept` may; then `Accept` decides raw payload or Binary
//! envelope). `fhir` returns a `Parameters` with one `row` per result row and
//! one part per non-null column, typed by the SQL to FHIR mapping of SQL on
//! FHIR v3 (`$sql-run`, "Output Formats").

use base64::{Engine, engine::general_purpose::STANDARD};
use octofhir_sof::output::{CsvWriter, OutputWriter, ParquetWriter, get_writer};
use octofhir_sof::{ColumnInfo, ColumnType, ViewResult};
use serde_json::{Map, Value, json};

use crate::operations::OperationError;

/// The requested representation of the rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Ndjson,
    Json,
    Csv { header: bool },
    Parquet,
    Fhir,
}

/// The media type of Parquet files.
pub(crate) const PARQUET_MEDIA_TYPE: &str = "application/vnd.apache.parquet";

/// Key on a rendered Binary asking the router to answer with the Binary
/// resource itself rather than its payload; never part of a response.
pub(crate) const ENVELOPE_KEY: &str = "__sofBinaryEnvelope";

/// The negotiated output: which format, and whether the payload goes out
/// wrapped in a Binary resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Negotiated {
    pub format: OutputFormat,
    pub envelope: bool,
}

impl Negotiated {
    /// `_format` (with `header`) and the request's `Accept`.
    pub(crate) fn from_request(
        format: Option<&str>,
        header: bool,
        accept: Option<&str>,
    ) -> Result<Self, OperationError> {
        let ranked = ranked_media_types(accept);
        let format = match format {
            Some(format) => OutputFormat::parse(Some(format), header)?,
            // Without `_format`, the first media type that names a format.
            None => ranked
                .iter()
                .find_map(|media| OutputFormat::from_media_type(media, header))
                .unwrap_or(OutputFormat::Ndjson),
        };
        // A FHIR media type ranked before the format's own (or a generic
        // one) asks for the Binary envelope.
        let envelope = format != OutputFormat::Fhir
            && ranked
                .iter()
                .find(|media| {
                    is_fhir_media_type(media)
                        || OutputFormat::from_media_type(media, header) == Some(format)
                        || matches!(media.as_str(), "application/octet-stream" | "*/*")
                })
                .is_some_and(|media| is_fhir_media_type(media));
        Ok(Self { format, envelope })
    }
}

fn is_fhir_media_type(media: &str) -> bool {
    matches!(media, "application/fhir+json" | "application/json+fhir")
}

/// The media types of an `Accept` header, best first (by `q`, then order),
/// without parameters; `q=0` ones dropped.
fn ranked_media_types(accept: Option<&str>) -> Vec<String> {
    let mut ranked: Vec<(f32, usize, String)> = accept
        .unwrap_or_default()
        .split(',')
        .enumerate()
        .filter_map(|(index, item)| {
            let mut parts = item.split(';');
            let media = parts.next()?.trim().to_ascii_lowercase();
            if media.is_empty() {
                return None;
            }
            let q = parts
                .filter_map(|part| part.trim().strip_prefix("q="))
                .find_map(|q| q.trim().parse::<f32>().ok())
                .unwrap_or(1.0);
            (q > 0.0).then_some((q, index, media))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    ranked.into_iter().map(|(_, _, media)| media).collect()
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
            "parquet" | PARQUET_MEDIA_TYPE | "application/parquet" => Ok(Self::Parquet),
            "fhir" => Ok(Self::Fhir),
            other => Err(OperationError::NotSupported(format!(
                "_format {other} is not supported; use ndjson, json, csv, parquet or fhir"
            ))),
        }
    }

    /// The format a media type of `Accept` names, if any.
    fn from_media_type(media: &str, header: bool) -> Option<Self> {
        match media {
            "application/x-ndjson" | "application/ndjson" | "application/fhir+ndjson" => {
                Some(Self::Ndjson)
            }
            "application/json" => Some(Self::Json),
            "text/csv" => Some(Self::Csv { header }),
            PARQUET_MEDIA_TYPE | "application/parquet" => Some(Self::Parquet),
            _ => None,
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
    /// `timestamptz`: valueInstant, rounded to the millisecond.
    Instant,
    /// An SQL type without a FHIR mapping (interval, array, a time with a
    /// zone, ...): 422 with `_format=fhir`.
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
            "time" => (ColumnType::Time, FhirValue::Field("valueTime")),
            "timestamp" => (ColumnType::DateTime, FhirValue::Field("valueDateTime")),
            "timestamptz" => (ColumnType::Instant, FhirValue::Instant),
            "json" | "jsonb" => (ColumnType::Json, FhirValue::JsonScalar),
            other => (ColumnType::Json, FhirValue::Unsupported(other.to_owned())),
        };
        Self {
            name: name.to_owned(),
            column_type,
            fhir,
        }
    }

    pub(crate) fn info(&self) -> ColumnInfo {
        ColumnInfo::new(self.name.clone(), self.column_type)
    }
}

/// The operation result: a Binary for the text formats, a Parameters for
/// `fhir`. `max_bytes` bounds the rendered size.
pub(crate) fn render(
    output: Negotiated,
    columns: &[OutputColumn],
    rows: &[Value],
    max_bytes: usize,
    integer64: bool,
) -> Result<Value, OperationError> {
    let format = output.format;
    let too_large = || {
        OperationError::InvalidParameters("The output exceeds the configured byte budget".into())
    };
    if format == OutputFormat::Fhir {
        let parameters = fhir_rows(columns, rows, integer64)?;
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
        OutputFormat::Parquet => Box::new(ParquetWriter::new()),
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
    let content_type = if format == OutputFormat::Parquet {
        PARQUET_MEDIA_TYPE
    } else {
        writer.content_type()
    };
    let mut binary = json!({
        "resourceType": "Binary",
        "contentType": content_type,
        "data": STANDARD.encode(bytes),
    });
    if output.envelope {
        binary[ENVELOPE_KEY] = json!(true);
    }
    Ok(binary)
}

/// `Parameters` with a `row` per row; a null value has no part.
fn fhir_rows(
    columns: &[OutputColumn],
    rows: &[Value],
    integer64: bool,
) -> Result<Value, OperationError> {
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
            let (field, value) = fhir_value(column, value, integer64)?;
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
    integer64: bool,
) -> Result<(&'static str, Value), OperationError> {
    match &column.fhir {
        // SQL on FHIR: a TIMESTAMP without time zone becomes a dateTime
        // without an offset; the absence of zone information is preserved.
        FhirValue::Field("valueDateTime") => Ok(("valueDateTime", value.clone())),
        // FHIR JSON writes integer64 as a string (R5 on); R4 and R4B have no
        // integer64, so a BIGINT is a decimal there.
        FhirValue::Field("valueInteger64") => Ok(if integer64 {
            ("valueInteger64", Value::String(value.to_string()))
        } else {
            ("valueDecimal", value.clone())
        }),
        FhirValue::Instant => Ok((
            "valueInstant",
            value
                .as_str()
                .and_then(round_to_millisecond)
                .map(Value::String)
                .unwrap_or_else(|| value.clone()),
        )),
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

/// A timestamp with zone rounded to the nearest millisecond (SQL on FHIR:
/// "SHOULD round to the nearest millisecond").
fn round_to_millisecond(text: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    let rounded = chrono::SubsecRound::round_subsecs(parsed, 3);
    Some(rounded.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string())
}

/// Whether the server's FHIR version has integer64 (R5 and later).
pub(crate) fn has_integer64(state: &crate::server::AppState) -> bool {
    !matches!(
        state.config.fhir.release(),
        Ok(octofhir_core::fhir::FhirVersion::R4 | octofhir_core::fhir::FhirVersion::R4B) | Err(_)
    )
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
    fn raw(format: OutputFormat) -> Negotiated {
        Negotiated {
            format,
            envelope: false,
        }
    }

    #[test]
    fn format_wins_over_accept_and_accept_picks_the_rest() {
        let negotiate = |format: Option<&str>, accept: Option<&str>| {
            Negotiated::from_request(format, true, accept).unwrap()
        };
        assert_eq!(negotiate(None, None), raw(OutputFormat::Ndjson));
        assert_eq!(
            negotiate(None, Some("text/csv")),
            raw(OutputFormat::Csv { header: true })
        );
        assert_eq!(
            negotiate(Some("ndjson"), Some("text/csv")),
            raw(OutputFormat::Ndjson)
        );
        assert_eq!(
            negotiate(None, Some("application/json;q=0.5, text/csv")),
            raw(OutputFormat::Csv { header: true })
        );
        assert_eq!(
            negotiate(None, Some(PARQUET_MEDIA_TYPE)),
            raw(OutputFormat::Parquet)
        );
        // A FHIR media type asks for the Binary envelope.
        let fhir = negotiate(Some("csv"), Some("application/fhir+json"));
        assert!(fhir.envelope);
        assert_eq!(fhir.format, OutputFormat::Csv { header: true });
        assert!(negotiate(None, Some("application/fhir+json")).envelope);
        assert!(!negotiate(Some("csv"), Some("text/csv, application/fhir+json")).envelope);
        assert!(!negotiate(Some("csv"), Some("application/octet-stream")).envelope);
        assert!(!negotiate(Some("fhir"), Some("application/fhir+json")).envelope);
    }

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
        assert_eq!(
            OutputFormat::parse(Some("parquet"), true).unwrap(),
            OutputFormat::Parquet
        );
        assert!(OutputFormat::parse(Some("xml"), true).is_err());
    }

    #[test]
    fn rows_render_as_ndjson_in_column_order() {
        let columns = vec![
            OutputColumn::from_view(&ColumnInfo::new("id", ColumnType::String)),
            OutputColumn::from_view(&ColumnInfo::new("active", ColumnType::Boolean)),
        ];
        let rows = vec![json!({"active": true, "id": "p1"})];
        let binary = render(raw(OutputFormat::Ndjson), &columns, &rows, usize::MAX, true).unwrap();
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
            "id": "p1", "n": 3, "at": "2026-01-02T03:04:05.123456+00:00",
            "local": "2026-01-02T03:04:05", "blob": "\\x6869", "flag": null
        })];
        let parameters =
            render(raw(OutputFormat::Fhir), &columns, &rows, usize::MAX, true).unwrap();
        assert_eq!(
            parameters,
            json!({"resourceType": "Parameters", "parameter": [{"name": "row", "part": [
                {"name": "id", "valueString": "p1"},
                {"name": "n", "valueInteger64": "3"},
                {"name": "at", "valueInstant": "2026-01-02T03:04:05.123+00:00"},
                {"name": "local", "valueDateTime": "2026-01-02T03:04:05"},
                {"name": "blob", "valueBase64Binary": "aGk="}
            ]}]})
        );
        // R4 / R4B have no integer64: a BIGINT is a decimal there.
        let r4 = render(raw(OutputFormat::Fhir), &columns, &rows, usize::MAX, false).unwrap();
        assert_eq!(
            r4["parameter"][0]["part"][1],
            json!({"name": "n", "valueDecimal": 3})
        );
        let zoned_time = [OutputColumn::from_sql("t", "timetz")];
        assert!(
            render(
                raw(OutputFormat::Fhir),
                &zoned_time,
                &[json!({"t": "10:00:00+02"})],
                usize::MAX,
                true
            )
            .is_err()
        );
        assert_eq!(
            render(raw(OutputFormat::Fhir), &columns, &[], usize::MAX, true).unwrap(),
            json!({"resourceType": "Parameters"})
        );
    }

    #[test]
    fn a_column_without_a_fhir_type_is_unprocessable_with_fhir_only() {
        let columns = vec![OutputColumn::from_sql("span", "interval")];
        let rows = vec![json!({"span": "1 day"})];
        assert!(render(raw(OutputFormat::Fhir), &columns, &rows, usize::MAX, true).is_err());
        assert!(render(raw(OutputFormat::Json), &columns, &rows, usize::MAX, true).is_ok());
    }
}
