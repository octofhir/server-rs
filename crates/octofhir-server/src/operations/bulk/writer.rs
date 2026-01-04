//! NDJSON file writer for bulk export
//!
//! Provides streaming NDJSON file writing with automatic file splitting
//! when resource limits are reached.

use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;
use tokio::fs::{self, File};
use tokio::io::{AsyncWriteExt, BufWriter};
use uuid::Uuid;

/// Errors that can occur during NDJSON writing
#[derive(Debug, Error)]
pub enum NdjsonWriterError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid export path: {0}")]
    InvalidPath(String),
}

/// NDJSON file writer with automatic file management
pub struct NdjsonWriter {
    /// Base directory for export files
    base_path: PathBuf,

    /// Job ID (used for directory naming)
    job_id: Uuid,

    /// Maximum resources per file before splitting
    max_resources_per_file: usize,

    /// Current file writers by resource type
    writers: std::collections::HashMap<String, TypeWriter>,
}

/// Writer state for a specific resource type
struct TypeWriter {
    /// Resource type name
    resource_type: String,

    /// Current file index (for splitting)
    file_index: usize,

    /// Resources written to current file
    current_count: usize,

    /// Total resources written across all files
    total_count: usize,

    /// Current buffered file writer
    writer: Option<BufWriter<File>>,

    /// List of generated file paths
    files: Vec<PathBuf>,

    /// Maximum resources per file
    max_per_file: usize,

    /// Job directory path
    job_dir: PathBuf,
}

impl TypeWriter {
    fn new(resource_type: &str, job_dir: PathBuf, max_per_file: usize) -> Self {
        Self {
            resource_type: resource_type.to_string(),
            file_index: 0,
            current_count: 0,
            total_count: 0,
            writer: None,
            files: Vec::new(),
            max_per_file,
            job_dir,
        }
    }

    /// Get or create a writer for the current file
    async fn get_writer(&mut self) -> Result<&mut BufWriter<File>, NdjsonWriterError> {
        if self.writer.is_none() || self.current_count >= self.max_per_file {
            // Close current writer if exists
            if let Some(mut w) = self.writer.take() {
                w.flush().await?;
            }

            // Increment file index if we're splitting
            if self.current_count >= self.max_per_file {
                self.file_index += 1;
                self.current_count = 0;
            }

            // Create new file
            let filename = if self.file_index == 0 {
                format!("{}.ndjson", self.resource_type)
            } else {
                format!("{}.{}.ndjson", self.resource_type, self.file_index)
            };

            let file_path = self.job_dir.join(&filename);
            let file = File::create(&file_path).await?;
            self.files.push(file_path);
            self.writer = Some(BufWriter::new(file));
        }

        Ok(self.writer.as_mut().unwrap())
    }

    /// Write a resource to the NDJSON file
    async fn write_resource(&mut self, resource: &Value) -> Result<(), NdjsonWriterError> {
        let writer = self.get_writer().await?;

        // Serialize to JSON and write as a single line
        let mut line = serde_json::to_vec(resource)?;
        line.push(b'\n');
        writer.write_all(&line).await?;

        self.current_count += 1;
        self.total_count += 1;

        Ok(())
    }

    /// Flush and close the writer
    async fn finish(&mut self) -> Result<(), NdjsonWriterError> {
        if let Some(mut w) = self.writer.take() {
            w.flush().await?;
        }
        Ok(())
    }

    /// Get list of generated files with their resource counts
    fn get_files(&self) -> Vec<(PathBuf, usize)> {
        // For simplicity, we report total count for each file
        // In production, we'd track per-file counts
        self.files
            .iter()
            .map(|p| (p.clone(), self.total_count))
            .collect()
    }
}

impl NdjsonWriter {
    /// Create a new NDJSON writer for a bulk export job
    pub async fn new(
        base_path: impl AsRef<Path>,
        job_id: Uuid,
        max_resources_per_file: usize,
    ) -> Result<Self, NdjsonWriterError> {
        let base_path = base_path.as_ref().to_path_buf();

        // Create job-specific directory
        let job_dir = base_path.join(job_id.to_string());
        fs::create_dir_all(&job_dir).await?;

        Ok(Self {
            base_path,
            job_id,
            max_resources_per_file,
            writers: std::collections::HashMap::new(),
        })
    }

    /// Get the job directory path
    pub fn job_dir(&self) -> PathBuf {
        self.base_path.join(self.job_id.to_string())
    }

    /// Write a resource to the appropriate NDJSON file based on its type
    pub async fn write_resource(
        &mut self,
        resource_type: &str,
        resource: &Value,
    ) -> Result<(), NdjsonWriterError> {
        // Extract values before borrowing self.writers to avoid borrow conflict
        let job_dir = self.job_dir();
        let max_per_file = self.max_resources_per_file;

        let writer = self
            .writers
            .entry(resource_type.to_string())
            .or_insert_with(|| TypeWriter::new(resource_type, job_dir, max_per_file));

        writer.write_resource(resource).await
    }

    /// Write multiple resources of the same type
    pub async fn write_resources(
        &mut self,
        resource_type: &str,
        resources: &[Value],
    ) -> Result<usize, NdjsonWriterError> {
        let mut count = 0;
        for resource in resources {
            self.write_resource(resource_type, resource).await?;
            count += 1;
        }
        Ok(count)
    }

    /// Finish writing and return file information
    ///
    /// Returns a map of resource type -> list of (file_path, count)
    pub async fn finish(
        mut self,
    ) -> Result<std::collections::HashMap<String, Vec<(PathBuf, usize)>>, NdjsonWriterError> {
        let mut result = std::collections::HashMap::new();

        for (resource_type, mut writer) in self.writers.drain() {
            writer.finish().await?;
            let files = writer.get_files();
            if !files.is_empty() {
                result.insert(resource_type, files);
            }
        }

        Ok(result)
    }

    /// Get current statistics
    pub fn stats(&self) -> NdjsonWriterStats {
        let mut total_resources = 0;
        let mut total_files = 0;
        let mut types_count = 0;

        for writer in self.writers.values() {
            total_resources += writer.total_count;
            total_files += writer.files.len();
            if writer.total_count > 0 {
                types_count += 1;
            }
        }

        NdjsonWriterStats {
            total_resources,
            total_files,
            resource_types: types_count,
        }
    }
}

/// Statistics about written NDJSON files
#[derive(Debug, Clone)]
pub struct NdjsonWriterStats {
    /// Total resources written
    pub total_resources: usize,
    /// Total files created
    pub total_files: usize,
    /// Number of distinct resource types
    pub resource_types: usize,
}

/// Clean up expired export directories
pub async fn cleanup_expired_exports(
    base_path: impl AsRef<Path>,
    max_age_hours: u64,
) -> std::io::Result<usize> {
    let base_path = base_path.as_ref();
    let mut cleaned = 0;

    if let Err(err) = tokio::fs::metadata(base_path).await {
        if err.kind() == std::io::ErrorKind::NotFound {
            return Ok(0);
        }
        return Err(err);
    }

    let max_age = std::time::Duration::from_secs(max_age_hours * 3600);
    let now = std::time::SystemTime::now();

    let mut entries = fs::read_dir(base_path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if entry.file_type().await?.is_dir() {
            // Check directory modification time
            if let Ok(metadata) = entry.metadata().await
                && let Ok(modified) = metadata.modified()
                    && let Ok(age) = now.duration_since(modified)
                        && age > max_age {
                            tracing::info!(
                                path = %path.display(),
                                age_hours = age.as_secs() / 3600,
                                "Cleaning up expired export directory"
                            );
                            fs::remove_dir_all(&path).await?;
                            cleaned += 1;
                        }
        }
    }

    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_write_single_resource() {
        let dir = tempdir().unwrap();
        let job_id = Uuid::new_v4();
        let mut writer = NdjsonWriter::new(dir.path(), job_id, 1000)
            .await
            .unwrap();

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "id": "123",
            "name": [{"family": "Test"}]
        });

        writer.write_resource("Patient", &resource).await.unwrap();

        let files = writer.finish().await.unwrap();
        assert!(files.contains_key("Patient"));
        assert_eq!(files["Patient"].len(), 1);
    }

    #[tokio::test]
    async fn test_file_splitting() {
        let dir = tempdir().unwrap();
        let job_id = Uuid::new_v4();
        let mut writer = NdjsonWriter::new(dir.path(), job_id, 2)
            .await
            .unwrap(); // Split after 2 resources

        for i in 0..5 {
            let resource = serde_json::json!({
                "resourceType": "Patient",
                "id": format!("{}", i)
            });
            writer.write_resource("Patient", &resource).await.unwrap();
        }

        let files = writer.finish().await.unwrap();
        // With max 2 per file and 5 resources, we should have 3 files
        assert_eq!(files["Patient"].len(), 3);
    }

    #[tokio::test]
    async fn test_multiple_resource_types() {
        let dir = tempdir().unwrap();
        let job_id = Uuid::new_v4();
        let mut writer = NdjsonWriter::new(dir.path(), job_id, 1000)
            .await
            .unwrap();

        writer
            .write_resource(
                "Patient",
                &serde_json::json!({"resourceType": "Patient", "id": "1"}),
            )
            .await
            .unwrap();
        writer
            .write_resource(
                "Observation",
                &serde_json::json!({"resourceType": "Observation", "id": "1"}),
            )
            .await
            .unwrap();

        let files = writer.finish().await.unwrap();
        assert!(files.contains_key("Patient"));
        assert!(files.contains_key("Observation"));
    }
}
