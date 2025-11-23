//! Output format writers for view results.
//!
//! This module provides writers for different output formats:
//! - CSV
//! - NDJSON (Newline Delimited JSON)
//! - JSON Array
//! - Parquet (when the `parquet` feature is enabled)
//!
//! Both synchronous and asynchronous writers are supported.

mod csv;
mod ndjson;

pub use csv::CsvWriter;
pub use ndjson::{JsonArrayWriter, NdjsonWriter};

#[cfg(feature = "parquet")]
mod parquet_writer;

#[cfg(feature = "parquet")]
pub use parquet_writer::{ParquetCompression, ParquetWriter};

use std::io::Write;
use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;
use serde_json::Value;
use tokio::io::AsyncWrite;

use crate::column::ColumnInfo;
use crate::runner::ViewResult;
use crate::{Error, Result};

/// Trait for writing view results to different output formats (synchronous).
pub trait OutputWriter: Send + Sync {
    /// Get the MIME content type for this format.
    fn content_type(&self) -> &'static str;

    /// Get the file extension for this format.
    fn file_extension(&self) -> &'static str;

    /// Write the view result to the output.
    fn write(&self, result: &ViewResult, output: &mut dyn Write) -> Result<()>;
}

/// Trait for writing view results asynchronously with streaming support.
#[async_trait]
pub trait AsyncOutputWriter: Send + Sync {
    /// Get the MIME content type for this format.
    fn content_type(&self) -> &'static str;

    /// Get the file extension for this format.
    fn file_extension(&self) -> &'static str;

    /// Write the view result to the async output.
    async fn write<W: AsyncWrite + Unpin + Send>(
        &self,
        result: &ViewResult,
        writer: W,
    ) -> Result<()>;

    /// Write rows as a stream to the async output.
    ///
    /// This is useful for large result sets where buffering everything in memory
    /// would be impractical.
    async fn write_streaming<W: AsyncWrite + Unpin + Send>(
        &self,
        columns: &[ColumnInfo],
        rows: Pin<Box<dyn Stream<Item = Vec<Value>> + Send>>,
        writer: W,
    ) -> Result<()>;
}

/// Output format enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Comma-separated values.
    Csv,

    /// Newline-delimited JSON.
    Ndjson,

    /// JSON array.
    Json,

    /// Apache Parquet (requires `parquet` feature).
    #[cfg(feature = "parquet")]
    Parquet,
}

impl OutputFormat {
    /// Parse an output format from a string.
    ///
    /// # Errors
    ///
    /// Returns an error if the format string is not recognized.
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "ndjson" | "jsonl" => Ok(Self::Ndjson),
            "json" => Ok(Self::Json),
            #[cfg(feature = "parquet")]
            "parquet" => Ok(Self::Parquet),
            _ => Err(Error::Output(format!("Unknown format: {}", s))),
        }
    }

    /// Get the file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Ndjson => "ndjson",
            Self::Json => "json",
            #[cfg(feature = "parquet")]
            Self::Parquet => "parquet",
        }
    }

    /// Get the MIME type for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Csv => "text/csv; charset=utf-8",
            Self::Ndjson => "application/x-ndjson",
            Self::Json => "application/json",
            #[cfg(feature = "parquet")]
            Self::Parquet => "application/vnd.apache.parquet",
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Csv => write!(f, "csv"),
            Self::Ndjson => write!(f, "ndjson"),
            Self::Json => write!(f, "json"),
            #[cfg(feature = "parquet")]
            Self::Parquet => write!(f, "parquet"),
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

/// Get a synchronous writer for the specified format.
///
/// # Errors
///
/// Returns an error if the format is not recognized.
pub fn get_writer(format: &str) -> Result<Box<dyn OutputWriter>> {
    match format.to_lowercase().as_str() {
        "csv" => Ok(Box::new(CsvWriter::new())),
        "ndjson" | "jsonl" => Ok(Box::new(NdjsonWriter::new())),
        "json" => Ok(Box::new(JsonArrayWriter::new())),
        #[cfg(feature = "parquet")]
        "parquet" => Ok(Box::new(ParquetWriter::new())),
        _ => Err(Error::Output(format!("Unknown format: {}", format))),
    }
}

/// Writer enum for async output.
///
/// Since `AsyncOutputWriter` has generic type parameters, it cannot be
/// used as a trait object. Use this enum for dynamic dispatch instead.
#[derive(Debug, Clone)]
pub enum AsyncWriter {
    /// CSV output writer.
    Csv(CsvWriter),
    /// NDJSON output writer.
    Ndjson(NdjsonWriter),
    /// JSON array output writer.
    Json(JsonArrayWriter),
    /// Parquet output writer.
    #[cfg(feature = "parquet")]
    Parquet(ParquetWriter),
}

impl AsyncWriter {
    /// Create a new async writer for the specified format.
    ///
    /// # Errors
    ///
    /// Returns an error if the format is not recognized.
    pub fn new(format: &str) -> Result<Self> {
        match format.to_lowercase().as_str() {
            "csv" => Ok(Self::Csv(CsvWriter::new())),
            "ndjson" | "jsonl" => Ok(Self::Ndjson(NdjsonWriter::new())),
            "json" => Ok(Self::Json(JsonArrayWriter::new())),
            #[cfg(feature = "parquet")]
            "parquet" => Ok(Self::Parquet(ParquetWriter::new())),
            _ => Err(Error::Output(format!("Unknown format: {}", format))),
        }
    }

    /// Get the content type for this writer.
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Csv(w) => <CsvWriter as OutputWriter>::content_type(w),
            Self::Ndjson(w) => <NdjsonWriter as OutputWriter>::content_type(w),
            Self::Json(w) => <JsonArrayWriter as OutputWriter>::content_type(w),
            #[cfg(feature = "parquet")]
            Self::Parquet(w) => <ParquetWriter as OutputWriter>::content_type(w),
        }
    }

    /// Get the file extension for this writer.
    pub fn file_extension(&self) -> &'static str {
        match self {
            Self::Csv(w) => <CsvWriter as OutputWriter>::file_extension(w),
            Self::Ndjson(w) => <NdjsonWriter as OutputWriter>::file_extension(w),
            Self::Json(w) => <JsonArrayWriter as OutputWriter>::file_extension(w),
            #[cfg(feature = "parquet")]
            Self::Parquet(w) => <ParquetWriter as OutputWriter>::file_extension(w),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_parse() {
        assert_eq!(OutputFormat::parse("csv").unwrap(), OutputFormat::Csv);
        assert_eq!(OutputFormat::parse("CSV").unwrap(), OutputFormat::Csv);
        assert_eq!(OutputFormat::parse("ndjson").unwrap(), OutputFormat::Ndjson);
        assert_eq!(OutputFormat::parse("jsonl").unwrap(), OutputFormat::Ndjson);
        assert_eq!(OutputFormat::parse("json").unwrap(), OutputFormat::Json);
        assert!(OutputFormat::parse("unknown").is_err());
    }

    #[test]
    fn test_output_format_extension() {
        assert_eq!(OutputFormat::Csv.extension(), "csv");
        assert_eq!(OutputFormat::Ndjson.extension(), "ndjson");
        assert_eq!(OutputFormat::Json.extension(), "json");
    }

    #[test]
    fn test_output_format_mime_type() {
        assert_eq!(OutputFormat::Csv.mime_type(), "text/csv; charset=utf-8");
        assert_eq!(OutputFormat::Ndjson.mime_type(), "application/x-ndjson");
        assert_eq!(OutputFormat::Json.mime_type(), "application/json");
    }

    #[test]
    fn test_get_writer() {
        assert!(get_writer("csv").is_ok());
        assert!(get_writer("ndjson").is_ok());
        assert!(get_writer("json").is_ok());
        assert!(get_writer("unknown").is_err());
    }
}
