//! Bulk export status types and tracking
//!
//! Defines the data structures for tracking bulk export jobs and their output.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Level at which the export was initiated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BulkExportLevel {
    /// System-level export (GET /$export)
    System,
    /// Patient-level export (GET /Patient/$export)
    Patient,
    /// Group-level export (GET /Group/{id}/$export)
    Group,
}

impl std::fmt::Display for BulkExportLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BulkExportLevel::System => write!(f, "system"),
            BulkExportLevel::Patient => write!(f, "patient"),
            BulkExportLevel::Group => write!(f, "group"),
        }
    }
}

/// Status of a bulk export job
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BulkExportStatus {
    /// Job is queued but not yet started
    Queued,
    /// Job is currently processing
    InProgress,
    /// Job completed successfully
    Completed,
    /// Job failed with an error
    Failed,
    /// Job was cancelled by the client
    Cancelled,
}

impl std::fmt::Display for BulkExportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BulkExportStatus::Queued => write!(f, "queued"),
            BulkExportStatus::InProgress => write!(f, "in_progress"),
            BulkExportStatus::Completed => write!(f, "completed"),
            BulkExportStatus::Failed => write!(f, "failed"),
            BulkExportStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Parameters for a bulk export request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkExportParams {
    /// Output format (must be application/fhir+ndjson)
    #[serde(rename = "_outputFormat")]
    pub output_format: Option<String>,

    /// Only include resources updated since this time
    #[serde(rename = "_since")]
    pub since: Option<DateTime<Utc>>,

    /// Comma-separated list of resource types to export
    #[serde(rename = "_type")]
    pub resource_types: Option<String>,

    /// FHIR search queries per type (comma-separated type?query pairs)
    #[serde(rename = "_typeFilter")]
    pub type_filter: Option<String>,

    /// For group-level exports, the group ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
}

impl Default for BulkExportParams {
    fn default() -> Self {
        Self {
            output_format: Some(super::DEFAULT_OUTPUT_FORMAT.to_string()),
            since: None,
            resource_types: None,
            type_filter: None,
            group_id: None,
        }
    }
}

impl BulkExportParams {
    /// Parse resource types from the _type parameter
    pub fn get_resource_types(&self) -> Vec<String> {
        self.resource_types
            .as_ref()
            .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default()
    }

    /// Parse type filters into (type, query) pairs
    pub fn get_type_filters(&self) -> Result<Vec<(String, String)>, String> {
        let mut filters: Vec<(String, String)> = Vec::new();
        if let Some(value) = &self.type_filter {
            for part in value.split(',') {
                if let Some((resource_type, query)) = part.split_once('?') {
                    if resource_type.is_empty()
                        || !resource_type
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric())
                        || !resource_type.as_bytes()[0].is_ascii_uppercase()
                        || query.is_empty()
                    {
                        return Err("_typeFilter requires ResourceType?search-criteria".into());
                    }
                    filters.push((resource_type.to_owned(), query.to_owned()));
                } else if let Some((_, query)) = filters.last_mut() {
                    // A comma in a search value is an OR alternative, not a
                    // delimiter starting a new ResourceType?query expression.
                    query.push(',');
                    query.push_str(part);
                } else {
                    return Err("_typeFilter requires ResourceType?search-criteria".into());
                }
            }
        }
        for (_, query) in &filters {
            if query.split('&').any(|pair| {
                pair.split_once('=')
                    .is_none_or(|(key, value)| key.is_empty() || value.is_empty())
            }) {
                return Err("_typeFilter requires nonempty search parameter values".into());
            }
        }
        Ok(filters)
    }
}

/// A bulk export job record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkExportJob {
    /// Unique job identifier (used in status URLs)
    pub id: Uuid,

    /// Export level (system, patient, or group)
    pub level: BulkExportLevel,

    /// Current status of the job
    pub status: BulkExportStatus,

    /// Export parameters
    pub params: BulkExportParams,

    /// Progress percentage (0.0 to 1.0)
    pub progress: f32,

    /// Error message if status is Failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,

    /// When the job was created
    pub created_at: DateTime<Utc>,

    /// When the job was last updated
    pub updated_at: DateTime<Utc>,

    /// When the job completed (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,

    /// When the export files expire
    pub expires_at: DateTime<Utc>,

    /// Client ID that initiated the export (for authorization)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Output files generated by the export
    #[serde(default)]
    pub output: Vec<BulkExportOutput>,

    /// Error files generated during export (OperationOutcome resources)
    #[serde(default)]
    pub error: Vec<BulkExportOutput>,

    /// Transient time for X-Progress header (only during polling)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transact_time: Option<DateTime<Utc>>,
}

impl BulkExportJob {
    /// Create a new bulk export job
    pub fn new(level: BulkExportLevel, params: BulkExportParams, retention_hours: u64) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            level,
            status: BulkExportStatus::Queued,
            params,
            progress: 0.0,
            error_message: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
            expires_at: now + chrono::Duration::hours(retention_hours as i64),
            client_id: None,
            output: Vec::new(),
            error: Vec::new(),
            transact_time: None,
        }
    }

    /// Check if the job is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            BulkExportStatus::Completed | BulkExportStatus::Failed | BulkExportStatus::Cancelled
        )
    }

    /// Add an output file to the job
    pub fn add_output(&mut self, output: BulkExportOutput) {
        self.output.push(output);
    }

    /// Add an error file to the job
    pub fn add_error(&mut self, error: BulkExportOutput) {
        self.error.push(error);
    }
}

/// Output file from a bulk export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkExportOutput {
    /// FHIR resource type contained in this file
    #[serde(rename = "type")]
    pub resource_type: String,

    /// URL to download the file
    pub url: String,

    /// Number of resources in the file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// Bulk export manifest returned when export completes
///
/// This is the response body for a successful status request when
/// the export is complete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkExportManifest {
    /// Time the export completed
    #[serde(rename = "transactionTime")]
    pub transaction_time: DateTime<Utc>,

    /// The original export request URL
    pub request: String,

    /// Whether the server requires authorization to access files
    #[serde(rename = "requiresAccessToken")]
    pub requires_access_token: bool,

    /// List of output files (by resource type)
    pub output: Vec<BulkExportOutput>,

    /// List of error files (OperationOutcome resources)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub error: Vec<BulkExportOutput>,

    /// Optional message about the export
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl BulkExportManifest {
    /// Create a manifest from a completed job
    pub fn from_job(job: &BulkExportJob, request_url: &str, requires_access_token: bool) -> Self {
        Self {
            transaction_time: job.completed_at.unwrap_or(job.updated_at),
            request: request_url.to_string(),
            requires_access_token,
            output: job.output.clone(),
            error: job.error.clone(),
            message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_resource_types() {
        let params = BulkExportParams {
            resource_types: Some("Patient,Observation,Condition".to_string()),
            ..Default::default()
        };

        let types = params.get_resource_types();
        assert_eq!(types, vec!["Patient", "Observation", "Condition"]);
    }

    #[test]
    fn test_parse_type_filters() {
        let params = BulkExportParams {
            type_filter: Some("Patient?gender=male,Observation?code=1234".to_string()),
            ..Default::default()
        };

        let filters = params.get_type_filters().unwrap();
        assert_eq!(filters.len(), 2);
        assert_eq!(
            filters[0],
            ("Patient".to_string(), "gender=male".to_string())
        );
        assert_eq!(
            filters[1],
            ("Observation".to_string(), "code=1234".to_string())
        );
    }

    #[test]
    fn test_job_lifecycle() {
        let params = BulkExportParams::default();
        let mut job = BulkExportJob::new(BulkExportLevel::System, params, 24);

        assert_eq!(job.status, BulkExportStatus::Queued);
        assert!(!job.is_terminal());

        job.status = BulkExportStatus::InProgress;
        assert!(!job.is_terminal());

        job.status = BulkExportStatus::Completed;
        assert!(job.is_terminal());
    }
}
