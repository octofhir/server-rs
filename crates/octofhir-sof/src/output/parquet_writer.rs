//! Parquet output writer for view results.
//!
//! This module is only available when the `parquet` feature is enabled.

use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use arrow::array::{ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use super::{AsyncOutputWriter, OutputWriter};
use crate::column::{ColumnInfo, ColumnType};
use crate::runner::ViewResult;
use crate::{Error, Result};

/// Parquet compression codecs.
#[derive(Debug, Clone, Copy, Default)]
pub enum ParquetCompression {
    /// No compression.
    None,
    /// Snappy compression (default, good balance of speed and ratio).
    #[default]
    Snappy,
    /// Gzip compression (better ratio, slower).
    Gzip,
    /// LZ4 compression (faster, lower ratio).
    Lz4,
    /// Zstd compression (good balance).
    Zstd,
}

impl From<ParquetCompression> for Compression {
    fn from(compression: ParquetCompression) -> Self {
        match compression {
            ParquetCompression::None => Compression::UNCOMPRESSED,
            ParquetCompression::Snappy => Compression::SNAPPY,
            ParquetCompression::Gzip => Compression::GZIP(Default::default()),
            ParquetCompression::Lz4 => Compression::LZ4,
            ParquetCompression::Zstd => Compression::ZSTD(Default::default()),
        }
    }
}

/// Parquet output writer configuration.
#[derive(Debug, Clone, Default)]
pub struct ParquetWriter {
    /// Compression codec to use.
    pub compression: ParquetCompression,
}

impl ParquetWriter {
    /// Create a new Parquet writer with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the compression codec.
    pub fn with_compression(mut self, compression: ParquetCompression) -> Self {
        self.compression = compression;
        self
    }

    /// Convert a column type to an Arrow data type.
    fn column_to_arrow_type(col_type: ColumnType) -> DataType {
        match col_type {
            ColumnType::Integer => DataType::Int64,
            ColumnType::Decimal => DataType::Float64,
            ColumnType::Boolean => DataType::Boolean,
            // All other types are stored as strings
            ColumnType::String
            | ColumnType::Date
            | ColumnType::DateTime
            | ColumnType::Instant
            | ColumnType::Time
            | ColumnType::Base64Binary
            | ColumnType::Json => DataType::Utf8,
        }
    }

    /// Build an Arrow schema from column info.
    fn build_schema(columns: &[ColumnInfo]) -> Schema {
        let fields: Vec<Field> = columns
            .iter()
            .map(|c| Field::new(&c.name, Self::column_to_arrow_type(c.col_type), c.nullable))
            .collect();

        Schema::new(fields)
    }

    /// Build Arrow arrays from result data.
    fn build_arrays(columns: &[ColumnInfo], data: &[Vec<Value>]) -> Result<Vec<ArrayRef>> {
        columns
            .iter()
            .enumerate()
            .map(|(i, col)| {
                let values: Vec<Option<&Value>> = data
                    .iter()
                    .map(|row| row.get(i).filter(|v| !v.is_null()))
                    .collect();

                Self::values_to_array(col.col_type, &values)
            })
            .collect()
    }

    /// Convert JSON values to an Arrow array.
    fn values_to_array(col_type: ColumnType, values: &[Option<&Value>]) -> Result<ArrayRef> {
        match col_type {
            ColumnType::Integer => {
                let arr: Int64Array = values.iter().map(|v| v.and_then(|v| v.as_i64())).collect();
                Ok(Arc::new(arr))
            }
            ColumnType::Decimal => {
                let arr: Float64Array = values.iter().map(|v| v.and_then(|v| v.as_f64())).collect();
                Ok(Arc::new(arr))
            }
            ColumnType::Boolean => {
                let arr: BooleanArray =
                    values.iter().map(|v| v.and_then(|v| v.as_bool())).collect();
                Ok(Arc::new(arr))
            }
            // All string-like types
            _ => {
                let arr: StringArray = values
                    .iter()
                    .map(|v| {
                        v.map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                    })
                    .collect();
                Ok(Arc::new(arr))
            }
        }
    }

    /// Write result data to a buffer as Parquet.
    fn write_to_buffer(&self, result: &ViewResult) -> Result<Vec<u8>> {
        let schema = Arc::new(Self::build_schema(&result.columns));
        let arrays = Self::build_arrays(&result.columns, &result.data)?;

        let record_batch = RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|e| Error::Output(format!("Failed to create record batch: {}", e)))?;

        let mut buffer = Vec::new();
        {
            let props = WriterProperties::builder()
                .set_compression(self.compression.into())
                .build();

            let mut arrow_writer = ArrowWriter::try_new(&mut buffer, schema, Some(props))
                .map_err(|e| Error::Output(format!("Failed to create Parquet writer: {}", e)))?;

            arrow_writer
                .write(&record_batch)
                .map_err(|e| Error::Output(format!("Failed to write record batch: {}", e)))?;

            arrow_writer
                .close()
                .map_err(|e| Error::Output(format!("Failed to close Parquet writer: {}", e)))?;
        }

        Ok(buffer)
    }
}

impl OutputWriter for ParquetWriter {
    fn content_type(&self) -> &'static str {
        "application/vnd.apache.parquet"
    }

    fn file_extension(&self) -> &'static str {
        "parquet"
    }

    fn write(&self, result: &ViewResult, output: &mut dyn Write) -> Result<()> {
        let buffer = self.write_to_buffer(result)?;
        output
            .write_all(&buffer)
            .map_err(|e| Error::Output(e.to_string()))?;
        output.flush().map_err(|e| Error::Output(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl AsyncOutputWriter for ParquetWriter {
    fn content_type(&self) -> &'static str {
        "application/vnd.apache.parquet"
    }

    fn file_extension(&self) -> &'static str {
        "parquet"
    }

    async fn write<W: AsyncWrite + Unpin + Send>(
        &self,
        result: &ViewResult,
        mut writer: W,
    ) -> Result<()> {
        // Write to buffer first (Parquet writer needs sync write)
        let buffer = self.write_to_buffer(result)?;

        // Write buffer to async writer
        writer
            .write_all(&buffer)
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
        writer: W,
    ) -> Result<()> {
        // Parquet is a columnar format, so we need to buffer all rows
        // to build efficient column chunks
        let mut data = Vec::new();
        let mut rows = rows;
        while let Some(row) = rows.next().await {
            data.push(row);
        }

        let result = ViewResult {
            columns: columns.to_vec(),
            data,
            row_count: 0,
        };

        AsyncOutputWriter::write(self, &result, writer).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parquet_writer_default() {
        let writer = ParquetWriter::new();
        assert!(matches!(writer.compression, ParquetCompression::Snappy));
    }

    #[test]
    fn test_parquet_compression_conversion() {
        assert!(matches!(
            Compression::from(ParquetCompression::None),
            Compression::UNCOMPRESSED
        ));
        assert!(matches!(
            Compression::from(ParquetCompression::Snappy),
            Compression::SNAPPY
        ));
    }

    #[test]
    fn test_column_to_arrow_type() {
        assert_eq!(
            ParquetWriter::column_to_arrow_type(ColumnType::Integer),
            DataType::Int64
        );
        assert_eq!(
            ParquetWriter::column_to_arrow_type(ColumnType::Decimal),
            DataType::Float64
        );
        assert_eq!(
            ParquetWriter::column_to_arrow_type(ColumnType::Boolean),
            DataType::Boolean
        );
        assert_eq!(
            ParquetWriter::column_to_arrow_type(ColumnType::String),
            DataType::Utf8
        );
        assert_eq!(
            ParquetWriter::column_to_arrow_type(ColumnType::DateTime),
            DataType::Utf8
        );
    }

    #[test]
    fn test_build_schema() {
        let columns = vec![
            ColumnInfo::new("id", ColumnType::String),
            ColumnInfo::new("value", ColumnType::Integer),
            ColumnInfo::new("active", ColumnType::Boolean),
        ];

        let schema = ParquetWriter::build_schema(&columns);
        assert_eq!(schema.fields().len(), 3);
        assert_eq!(schema.field(0).name(), "id");
        assert_eq!(schema.field(0).data_type(), &DataType::Utf8);
        assert_eq!(schema.field(1).name(), "value");
        assert_eq!(schema.field(1).data_type(), &DataType::Int64);
        assert_eq!(schema.field(2).name(), "active");
        assert_eq!(schema.field(2).data_type(), &DataType::Boolean);
    }

    #[test]
    fn test_parquet_write() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("value", ColumnType::Integer),
            ],
            data: vec![vec![json!("1"), json!(100)], vec![json!("2"), json!(200)]],
            row_count: 2,
        };

        let writer = ParquetWriter::new();
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        // Parquet files start with "PAR1" magic bytes
        assert!(output.len() > 4);
        assert_eq!(&output[0..4], b"PAR1");
    }

    #[test]
    fn test_parquet_empty_result() {
        let result = ViewResult {
            columns: vec![ColumnInfo::new("id", ColumnType::String)],
            data: vec![],
            row_count: 0,
        };

        let writer = ParquetWriter::new();
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        // Even empty files have the Parquet header
        assert!(output.len() > 4);
        assert_eq!(&output[0..4], b"PAR1");
    }

    #[test]
    fn test_parquet_null_values() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("name", ColumnType::String),
            ],
            data: vec![vec![json!("1"), Value::Null]],
            row_count: 1,
        };

        let writer = ParquetWriter::new();
        let mut output = Vec::new();
        OutputWriter::write(&writer, &result, &mut output).unwrap();

        // Should write successfully with null value
        assert!(output.len() > 4);
    }

    #[test]
    fn test_content_type_and_extension() {
        let writer = ParquetWriter::new();
        assert_eq!(
            <ParquetWriter as OutputWriter>::content_type(&writer),
            "application/vnd.apache.parquet"
        );
        assert_eq!(
            <ParquetWriter as OutputWriter>::file_extension(&writer),
            "parquet"
        );
    }

    #[tokio::test]
    async fn test_async_parquet_writer() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("count", ColumnType::Integer),
            ],
            data: vec![vec![json!("a"), json!(1)], vec![json!("b"), json!(2)]],
            row_count: 2,
        };

        let writer = ParquetWriter::new();
        let mut output = Vec::new();
        AsyncOutputWriter::write(&writer, &result, &mut output)
            .await
            .unwrap();

        assert!(output.len() > 4);
        assert_eq!(&output[0..4], b"PAR1");
    }
}
