//! NDJSON (Newline Delimited JSON) output writer for view results.

use std::io::Write;
use std::pin::Pin;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::{AsyncOutputWriter, OutputWriter};
use crate::column::ColumnInfo;
use crate::runner::ViewResult;
use crate::{Error, Result};

/// NDJSON output writer.
///
/// Writes each row as a JSON object on a separate line.
/// This format is useful for streaming large result sets.
#[derive(Debug, Clone, Default)]
pub struct NdjsonWriter {
    /// Whether to pretty-print each JSON object.
    pub pretty: bool,
}

impl NdjsonWriter {
    /// Create a new NDJSON writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable pretty-printing of JSON objects.
    pub fn with_pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    /// Convert a row to a JSON object.
    fn row_to_object(columns: &[ColumnInfo], row: &[Value]) -> Value {
        let mut obj = serde_json::Map::new();
        for (col, value) in columns.iter().zip(row.iter()) {
            if !value.is_null() {
                obj.insert(col.name.clone(), value.clone());
            }
        }
        Value::Object(obj)
    }
}

impl OutputWriter for NdjsonWriter {
    fn content_type(&self) -> &'static str {
        "application/x-ndjson"
    }

    fn file_extension(&self) -> &'static str {
        "ndjson"
    }

    fn write(&self, result: &ViewResult, output: &mut dyn Write) -> Result<()> {
        for row in &result.data {
            let obj = Self::row_to_object(&result.columns, row);

            let line = if self.pretty {
                serde_json::to_string_pretty(&obj)
            } else {
                serde_json::to_string(&obj)
            }
            .map_err(|e| Error::Output(e.to_string()))?;

            writeln!(output, "{}", line).map_err(|e| Error::Output(e.to_string()))?;
        }

        Ok(())
    }
}

#[async_trait]
impl AsyncOutputWriter for NdjsonWriter {
    fn content_type(&self) -> &'static str {
        "application/x-ndjson"
    }

    fn file_extension(&self) -> &'static str {
        "ndjson"
    }

    async fn write<W: AsyncWrite + Unpin + Send>(
        &self,
        result: &ViewResult,
        mut writer: W,
    ) -> Result<()> {
        for row in &result.data {
            let obj = Self::row_to_object(&result.columns, row);

            let line = if self.pretty {
                serde_json::to_string_pretty(&obj)
            } else {
                serde_json::to_string(&obj)
            }
            .map_err(|e| Error::Output(e.to_string()))?;

            writer
                .write_all(line.as_bytes())
                .await
                .map_err(|e| Error::Output(e.to_string()))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|e| Error::Output(e.to_string()))?;
        }

        writer
            .flush()
            .await
            .map_err(|e| Error::Output(e.to_string()))?;

        Ok(())
    }

    async fn write_streaming<W: AsyncWrite + Unpin + Send>(
        &self,
        columns: &[ColumnInfo],
        rows: Pin<Box<dyn Stream<Item = Vec<Value>> + Send>>,
        mut writer: W,
    ) -> Result<()> {
        let mut rows = rows;
        while let Some(row) = rows.next().await {
            let obj = Self::row_to_object(columns, &row);

            let line = if self.pretty {
                serde_json::to_string_pretty(&obj)
            } else {
                serde_json::to_string(&obj)
            }
            .map_err(|e| Error::Output(e.to_string()))?;

            writer
                .write_all(line.as_bytes())
                .await
                .map_err(|e| Error::Output(e.to_string()))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|e| Error::Output(e.to_string()))?;
        }

        writer
            .flush()
            .await
            .map_err(|e| Error::Output(e.to_string()))?;

        Ok(())
    }
}

/// JSON array output writer.
///
/// Writes the entire result set as a single JSON array.
#[derive(Debug, Clone, Default)]
pub struct JsonArrayWriter {
    /// Whether to pretty-print the output.
    pub pretty: bool,
}

impl JsonArrayWriter {
    /// Create a new JSON array writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable pretty-printing.
    pub fn with_pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }
}

impl OutputWriter for JsonArrayWriter {
    fn content_type(&self) -> &'static str {
        "application/json"
    }

    fn file_extension(&self) -> &'static str {
        "json"
    }

    fn write(&self, result: &ViewResult, output: &mut dyn Write) -> Result<()> {
        let json_array = result.to_json_array();

        let json_str = if self.pretty {
            serde_json::to_string_pretty(&json_array)
        } else {
            serde_json::to_string(&json_array)
        }
        .map_err(|e| Error::Output(e.to_string()))?;

        write!(output, "{}", json_str).map_err(|e| Error::Output(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl AsyncOutputWriter for JsonArrayWriter {
    fn content_type(&self) -> &'static str {
        "application/json"
    }

    fn file_extension(&self) -> &'static str {
        "json"
    }

    async fn write<W: AsyncWrite + Unpin + Send>(
        &self,
        result: &ViewResult,
        mut writer: W,
    ) -> Result<()> {
        let json_array = result.to_json_array();

        let json_str = if self.pretty {
            serde_json::to_string_pretty(&json_array)
        } else {
            serde_json::to_string(&json_array)
        }
        .map_err(|e| Error::Output(e.to_string()))?;

        writer
            .write_all(json_str.as_bytes())
            .await
            .map_err(|e| Error::Output(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| Error::Output(e.to_string()))?;

        Ok(())
    }

    async fn write_streaming<W: AsyncWrite + Unpin + Send>(
        &self,
        columns: &[ColumnInfo],
        rows: Pin<Box<dyn Stream<Item = Vec<Value>> + Send>>,
        mut writer: W,
    ) -> Result<()> {
        // JSON arrays need to buffer all data (cannot stream)
        let mut data = Vec::new();
        let mut rows = rows;
        while let Some(row) = rows.next().await {
            data.push(row);
        }

        // Build result and use sync logic
        let result = ViewResult {
            columns: columns.to_vec(),
            data,
            row_count: 0, // Not needed for output
        };

        let json_array = result.to_json_array();

        let json_str = if self.pretty {
            serde_json::to_string_pretty(&json_array)
        } else {
            serde_json::to_string(&json_array)
        }
        .map_err(|e| Error::Output(e.to_string()))?;

        writer
            .write_all(json_str.as_bytes())
            .await
            .map_err(|e| Error::Output(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| Error::Output(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::ColumnType;
    use serde_json::json;

    #[test]
    fn test_ndjson_writer() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("name", ColumnType::String),
            ],
            data: vec![
                vec![json!("1"), json!("Alice")],
                vec![json!("2"), json!("Bob")],
            ],
            row_count: 2,
        };

        let writer = NdjsonWriter::new();
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        let output_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = output_str.lines().collect();

        assert_eq!(lines.len(), 2);

        let obj1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(obj1["id"], "1");
        assert_eq!(obj1["name"], "Alice");

        let obj2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(obj2["id"], "2");
        assert_eq!(obj2["name"], "Bob");
    }

    #[test]
    fn test_json_array_writer() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("value", ColumnType::Integer),
            ],
            data: vec![vec![json!("1"), json!(100)], vec![json!("2"), json!(200)]],
            row_count: 2,
        };

        let writer = JsonArrayWriter::new();
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        let output_str = String::from_utf8(output).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&output_str).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["id"], "1");
        assert_eq!(parsed[0]["value"], 100);
    }

    #[test]
    fn test_empty_result() {
        let result = ViewResult {
            columns: vec![ColumnInfo::new("id", ColumnType::String)],
            data: vec![],
            row_count: 0,
        };

        let writer = NdjsonWriter::new();
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        assert!(output.is_empty());

        let json_writer = JsonArrayWriter::new();
        let mut json_output = Vec::new();
        OutputWriter::write(&json_writer, &result, &mut json_output).unwrap();

        let output_str = String::from_utf8(json_output).unwrap();
        assert_eq!(output_str, "[]");
    }

    #[test]
    fn test_null_values_excluded() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("name", ColumnType::String),
            ],
            data: vec![vec![json!("1"), Value::Null]],
            row_count: 1,
        };

        let writer = NdjsonWriter::new();
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        let output_str = String::from_utf8(output).unwrap();
        let obj: serde_json::Value = serde_json::from_str(output_str.trim()).unwrap();

        // Null values should not be included in the output
        assert_eq!(obj["id"], "1");
        assert!(obj.get("name").is_none());
    }

    #[test]
    fn test_content_type_and_extension() {
        let ndjson = NdjsonWriter::new();
        assert_eq!(
            <NdjsonWriter as OutputWriter>::content_type(&ndjson),
            "application/x-ndjson"
        );
        assert_eq!(
            <NdjsonWriter as OutputWriter>::file_extension(&ndjson),
            "ndjson"
        );

        let json = JsonArrayWriter::new();
        assert_eq!(
            <JsonArrayWriter as OutputWriter>::content_type(&json),
            "application/json"
        );
        assert_eq!(
            <JsonArrayWriter as OutputWriter>::file_extension(&json),
            "json"
        );
    }

    #[tokio::test]
    async fn test_async_ndjson_writer() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("name", ColumnType::String),
            ],
            data: vec![
                vec![json!("1"), json!("Alice")],
                vec![json!("2"), json!("Bob")],
            ],
            row_count: 2,
        };

        let writer = NdjsonWriter::new();
        let mut output = Vec::new();
        AsyncOutputWriter::write(&writer, &result, &mut output)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = output_str.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn test_streaming_ndjson() {
        use futures_util::stream;

        let columns = vec![
            ColumnInfo::new("id", ColumnType::String),
            ColumnInfo::new("value", ColumnType::Integer),
        ];

        let rows: Vec<Vec<Value>> = vec![
            vec![json!("1"), json!(100)],
            vec![json!("2"), json!(200)],
            vec![json!("3"), json!(300)],
        ];

        let row_stream = stream::iter(rows);
        let boxed_stream: Pin<Box<dyn Stream<Item = Vec<Value>> + Send>> = Box::pin(row_stream);

        let writer = NdjsonWriter::new();
        let mut output = Vec::new();
        writer
            .write_streaming(&columns, boxed_stream, &mut output)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = output_str.lines().collect();
        assert_eq!(lines.len(), 3);

        let obj: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(obj["id"], "1");
        assert_eq!(obj["value"], 100);
    }
}
