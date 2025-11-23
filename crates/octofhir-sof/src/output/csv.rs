//! CSV output writer for view results.

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

/// CSV output writer configuration.
#[derive(Debug, Clone)]
pub struct CsvWriter {
    /// Whether to include a header row.
    pub include_header: bool,

    /// Field delimiter (default: comma).
    pub delimiter: u8,

    /// Quote character (default: double quote).
    pub quote: u8,
}

impl Default for CsvWriter {
    fn default() -> Self {
        Self {
            include_header: true,
            delimiter: b',',
            quote: b'"',
        }
    }
}

impl CsvWriter {
    /// Create a new CSV writer with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether to include a header row.
    pub fn with_header(mut self, include: bool) -> Self {
        self.include_header = include;
        self
    }

    /// Set the field delimiter.
    pub fn with_delimiter(mut self, delimiter: u8) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// Set the quote character.
    pub fn with_quote(mut self, quote: u8) -> Self {
        self.quote = quote;
        self
    }

    /// Escape a CSV value with proper quoting.
    fn escape_csv_value(&self, value: &str) -> String {
        let delimiter_char = self.delimiter as char;
        let quote_char = self.quote as char;

        if value.contains(delimiter_char)
            || value.contains(quote_char)
            || value.contains('\n')
            || value.contains('\r')
        {
            format!(
                "{}{}{}",
                quote_char,
                value.replace(quote_char, &format!("{}{}", quote_char, quote_char)),
                quote_char
            )
        } else {
            value.to_string()
        }
    }
}

/// Convert a JSON value to a CSV-appropriate string.
fn json_value_to_csv_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            // For arrays, join with semicolons
            arr.iter()
                .map(json_value_to_csv_string)
                .collect::<Vec<_>>()
                .join(";")
        }
        Value::Object(_) => {
            // For objects, serialize as JSON
            serde_json::to_string(value).unwrap_or_default()
        }
    }
}

impl OutputWriter for CsvWriter {
    fn content_type(&self) -> &'static str {
        "text/csv; charset=utf-8"
    }

    fn file_extension(&self) -> &'static str {
        "csv"
    }

    fn write(&self, result: &ViewResult, output: &mut dyn Write) -> Result<()> {
        let mut writer = csv::WriterBuilder::new()
            .delimiter(self.delimiter)
            .quote(self.quote)
            .has_headers(false) // We'll write headers manually if needed
            .from_writer(output);

        // Write header if enabled
        if self.include_header {
            let headers: Vec<&str> = result.columns.iter().map(|c| c.name.as_str()).collect();
            writer
                .write_record(&headers)
                .map_err(|e| Error::Output(e.to_string()))?;
        }

        // Write data rows
        for row in &result.data {
            let values: Vec<String> = row.iter().map(json_value_to_csv_string).collect();
            writer
                .write_record(&values)
                .map_err(|e| Error::Output(e.to_string()))?;
        }

        writer.flush().map_err(|e| Error::Output(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl AsyncOutputWriter for CsvWriter {
    fn content_type(&self) -> &'static str {
        "text/csv; charset=utf-8"
    }

    fn file_extension(&self) -> &'static str {
        "csv"
    }

    async fn write<W: AsyncWrite + Unpin + Send>(
        &self,
        result: &ViewResult,
        mut writer: W,
    ) -> Result<()> {
        let delimiter = self.delimiter as char;

        // Write header
        if self.include_header {
            let header: String = result
                .columns
                .iter()
                .map(|c| self.escape_csv_value(&c.name))
                .collect::<Vec<_>>()
                .join(&delimiter.to_string());

            writer
                .write_all(header.as_bytes())
                .await
                .map_err(|e| Error::Output(e.to_string()))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|e| Error::Output(e.to_string()))?;
        }

        // Write data rows
        for row in &result.data {
            let line: String = row
                .iter()
                .map(|v| self.escape_csv_value(&json_value_to_csv_string(v)))
                .collect::<Vec<_>>()
                .join(&delimiter.to_string());

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
        let delimiter = self.delimiter as char;

        // Write header
        if self.include_header {
            let header: String = columns
                .iter()
                .map(|c| self.escape_csv_value(&c.name))
                .collect::<Vec<_>>()
                .join(&delimiter.to_string());

            writer
                .write_all(header.as_bytes())
                .await
                .map_err(|e| Error::Output(e.to_string()))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|e| Error::Output(e.to_string()))?;
        }

        // Stream rows
        let mut rows = rows;
        while let Some(row) = rows.next().await {
            let line: String = row
                .iter()
                .map(|v| self.escape_csv_value(&json_value_to_csv_string(v)))
                .collect::<Vec<_>>()
                .join(&delimiter.to_string());

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::ColumnType;
    use serde_json::json;

    #[test]
    fn test_csv_writer() {
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

        let writer = CsvWriter::new();
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        let csv_str = String::from_utf8(output).unwrap();
        assert!(csv_str.contains("id,name"));
        assert!(csv_str.contains("1,Alice"));
        assert!(csv_str.contains("2,Bob"));
    }

    #[test]
    fn test_csv_writer_no_header() {
        let result = ViewResult {
            columns: vec![ColumnInfo::new("id", ColumnType::String)],
            data: vec![vec![json!("1")]],
            row_count: 1,
        };

        let writer = CsvWriter::new().with_header(false);
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        let csv_str = String::from_utf8(output).unwrap();
        assert!(!csv_str.contains("id"));
        assert!(csv_str.contains("1"));
    }

    #[test]
    fn test_json_value_to_csv_string() {
        assert_eq!(json_value_to_csv_string(&Value::Null), "");
        assert_eq!(json_value_to_csv_string(&json!(true)), "true");
        assert_eq!(json_value_to_csv_string(&json!(42)), "42");
        assert_eq!(json_value_to_csv_string(&json!("hello")), "hello");
        assert_eq!(json_value_to_csv_string(&json!(["a", "b", "c"])), "a;b;c");
    }

    #[test]
    fn test_csv_escaping() {
        let writer = CsvWriter::new();

        // Value with comma needs quoting
        assert_eq!(writer.escape_csv_value("hello,world"), "\"hello,world\"");

        // Value with quote needs escaping
        assert_eq!(writer.escape_csv_value("say \"hi\""), "\"say \"\"hi\"\"\"");

        // Value with newline needs quoting
        assert_eq!(writer.escape_csv_value("line1\nline2"), "\"line1\nline2\"");

        // Plain value doesn't need quoting
        assert_eq!(writer.escape_csv_value("hello"), "hello");
    }

    #[test]
    fn test_content_type_and_extension() {
        let writer = CsvWriter::new();
        assert_eq!(
            <CsvWriter as OutputWriter>::content_type(&writer),
            "text/csv; charset=utf-8"
        );
        assert_eq!(<CsvWriter as OutputWriter>::file_extension(&writer), "csv");
    }

    #[tokio::test]
    async fn test_async_csv_writer() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("value", ColumnType::Integer),
            ],
            data: vec![vec![json!("1"), json!(100)], vec![json!("2"), json!(200)]],
            row_count: 2,
        };

        let writer = CsvWriter::new();
        let mut output = Vec::new();
        AsyncOutputWriter::write(&writer, &result, &mut output)
            .await
            .unwrap();

        let csv_str = String::from_utf8(output).unwrap();
        assert!(csv_str.contains("id,value"));
        assert!(csv_str.contains("1,100"));
        assert!(csv_str.contains("2,200"));
    }
}
