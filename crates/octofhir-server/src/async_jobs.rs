//! FHIR Asynchronous Request Pattern Implementation
//!
//! This module implements the FHIR asynchronous request pattern (Prefer: respond-async)
//! which allows long-running operations to execute in the background while clients poll
//! for completion status.
//!
//! ## FHIR Specification
//! - Request: `Prefer: respond-async` header
//! - Response: 202 Accepted with `Content-Location` pointing to status endpoint
//! - Status Polling: `GET /_async-status/{job-id}`
//! - Result Retrieval: `GET /_async-status/{job-id}/result`
//!
//! ## Use Cases
//! - Large batch/transaction bundles
//! - Patient $everything with thousands of resources
//! - Bulk exports (Group $everything)
//! - Complex searches with massive result sets
//! - Custom operations taking > 5 seconds

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx_core::query::query;
use sqlx_core::row::Row;
use sqlx_postgres::PgPool;
use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur during async job operations
#[derive(Debug, Error)]
pub enum AsyncJobError {
    #[error("Job not found: {0}")]
    NotFound(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx_core::Error),

    #[error("Invalid job ID: {0}")]
    InvalidJobId(#[from] uuid::Error),

    #[error("Job execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid job status transition from {from} to {to}")]
    InvalidStatusTransition { from: String, to: String },
}

/// Job status enum matching database constraints
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsyncJobStatus {
    Queued,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for AsyncJobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AsyncJobStatus::Queued => write!(f, "queued"),
            AsyncJobStatus::InProgress => write!(f, "in_progress"),
            AsyncJobStatus::Completed => write!(f, "completed"),
            AsyncJobStatus::Failed => write!(f, "failed"),
            AsyncJobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Async job record from database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncJob {
    pub id: Uuid,
    pub status: AsyncJobStatus,
    pub request_type: String,
    pub request_method: String,
    pub request_url: String,
    pub request_body: Option<serde_json::Value>,
    pub request_headers: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub progress: f32,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub client_id: Option<String>,
    pub expires_at: DateTime<Utc>,
}

/// Request to create a new async job
#[derive(Debug, Clone)]
pub struct AsyncJobRequest {
    pub request_type: String,
    pub method: String,
    pub url: String,
    pub body: Option<serde_json::Value>,
    pub headers: Option<serde_json::Value>,
    pub client_id: Option<String>,
}

/// Configuration for async job manager
#[derive(Debug, Clone)]
pub struct AsyncJobConfig {
    pub enabled: bool,
    pub max_concurrent_jobs: usize,
    pub default_ttl_hours: i64,
    pub cleanup_interval_seconds: u64,
}

impl Default for AsyncJobConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_jobs: 10,
            default_ttl_hours: 24,
            cleanup_interval_seconds: 3600, // 1 hour
        }
    }
}

/// Job executor function type
/// Takes job ID and request details and returns result or error
pub type JobExecutor = Arc<
    dyn Fn(
            Uuid,                      // job_id
            String,                    // request_type
            String,                    // method
            String,                    // url
            Option<serde_json::Value>, // body
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>,
        > + Send
        + Sync,
>;

/// Manager for async job lifecycle
#[derive(Clone)]
pub struct AsyncJobManager {
    db_pool: Arc<PgPool>,
    config: Arc<AsyncJobConfig>,
    executor: Arc<std::sync::RwLock<Option<JobExecutor>>>,
}

impl AsyncJobManager {
    /// Create a new async job manager
    pub fn new(db_pool: Arc<PgPool>, config: AsyncJobConfig) -> Self {
        Self {
            db_pool,
            config: Arc::new(config),
            executor: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Set the job executor function
    pub fn with_executor(self, executor: JobExecutor) -> Self {
        *self.executor.write().unwrap() = Some(executor);
        self
    }

    /// Set the job executor function after construction
    /// This allows setting the executor after the manager has been created and shared
    pub fn set_executor(&self, executor: JobExecutor) {
        *self.executor.write().unwrap() = Some(executor);
    }

    /// Submit a new async job
    pub async fn submit_job(&self, request: AsyncJobRequest) -> Result<Uuid, AsyncJobError> {
        let ttl_hours = format!("{} hours", self.config.default_ttl_hours);
        let row = query(
            r#"
            INSERT INTO async_jobs (
                request_type,
                request_method,
                request_url,
                request_body,
                request_headers,
                client_id,
                expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, NOW() + $7::INTERVAL)
            RETURNING id
            "#,
        )
        .bind(&request.request_type)
        .bind(&request.method)
        .bind(&request.url)
        .bind(&request.body)
        .bind(&request.headers)
        .bind(&request.client_id)
        .bind(&ttl_hours)
        .fetch_one(self.db_pool.as_ref())
        .await?;

        let job_id: Uuid = row.try_get("id")?;

        tracing::info!(
            job_id = %job_id,
            request_type = %request.request_type,
            "Async job created"
        );

        // Spawn background execution if executor is configured
        if let Some(executor) = self.executor.read().unwrap().as_ref() {
            let manager = self.clone();
            let exec = executor.clone();
            let req_type = request.request_type.clone();
            let req_method = request.method.clone();
            let req_url = request.url.clone();
            let req_body = request.body.clone();

            tokio::spawn(async move {
                manager
                    .execute_job(job_id, exec, req_type, req_method, req_url, req_body)
                    .await;
            });
        }

        Ok(job_id)
    }

    /// Execute a job in the background
    async fn execute_job(
        &self,
        job_id: Uuid,
        executor: JobExecutor,
        request_type: String,
        method: String,
        url: String,
        body: Option<serde_json::Value>,
    ) {
        // Mark job as in progress
        if let Err(e) = self.update_status(job_id, AsyncJobStatus::InProgress).await {
            tracing::error!(job_id = %job_id, error = %e, "Failed to mark job as in progress");
            return;
        }

        tracing::info!(job_id = %job_id, "Starting job execution");

        // Execute the job
        let result = executor(job_id, request_type, method, url, body).await;

        // Update job based on result
        match result {
            Ok(result_data) => {
                if let Err(e) = self.complete_job(job_id, result_data).await {
                    tracing::error!(job_id = %job_id, error = %e, "Failed to mark job as completed");
                }
            }
            Err(error_msg) => {
                if let Err(e) = self.fail_job(job_id, error_msg).await {
                    tracing::error!(job_id = %job_id, error = %e, "Failed to mark job as failed");
                }
            }
        }
    }

    /// Get job details by ID
    pub async fn get_job(&self, job_id: Uuid) -> Result<AsyncJob, AsyncJobError> {
        let row = query(
            r#"
            SELECT
                id,
                status,
                request_type,
                request_method,
                request_url,
                request_body,
                request_headers,
                result,
                progress,
                error_message,
                created_at,
                updated_at,
                completed_at,
                client_id,
                expires_at
            FROM async_jobs
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .fetch_optional(self.db_pool.as_ref())
        .await?
        .ok_or_else(|| AsyncJobError::NotFound(job_id.to_string()))?;

        let status_str: String = row.try_get("status")?;
        let status = match status_str.as_str() {
            "queued" => AsyncJobStatus::Queued,
            "in_progress" => AsyncJobStatus::InProgress,
            "completed" => AsyncJobStatus::Completed,
            "failed" => AsyncJobStatus::Failed,
            "cancelled" => AsyncJobStatus::Cancelled,
            _ => AsyncJobStatus::Failed,
        };

        Ok(AsyncJob {
            id: row.try_get("id")?,
            status,
            request_type: row.try_get("request_type")?,
            request_method: row.try_get("request_method")?,
            request_url: row.try_get("request_url")?,
            request_body: row.try_get("request_body")?,
            request_headers: row.try_get("request_headers")?,
            result: row.try_get("result")?,
            progress: row.try_get("progress").unwrap_or(0.0),
            error_message: row.try_get("error_message")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            completed_at: row.try_get("completed_at")?,
            client_id: row.try_get("client_id")?,
            expires_at: row.try_get("expires_at")?,
        })
    }

    /// Update job status
    pub async fn update_status(
        &self,
        job_id: Uuid,
        status: AsyncJobStatus,
    ) -> Result<(), AsyncJobError> {
        let status_str = status.to_string();

        query(
            r#"
            UPDATE async_jobs
            SET status = $1
            WHERE id = $2
            "#,
        )
        .bind(&status_str)
        .bind(job_id)
        .execute(self.db_pool.as_ref())
        .await?;

        tracing::debug!(job_id = %job_id, status = %status_str, "Job status updated");

        Ok(())
    }

    /// Update job progress (0.0 to 1.0)
    pub async fn update_progress(&self, job_id: Uuid, progress: f32) -> Result<(), AsyncJobError> {
        let clamped_progress = progress.clamp(0.0, 1.0);

        query(
            r#"
            UPDATE async_jobs
            SET progress = $1
            WHERE id = $2
            "#,
        )
        .bind(clamped_progress)
        .bind(job_id)
        .execute(self.db_pool.as_ref())
        .await?;

        Ok(())
    }

    /// Mark job as completed with result
    pub async fn complete_job(
        &self,
        job_id: Uuid,
        result: serde_json::Value,
    ) -> Result<(), AsyncJobError> {
        query(
            r#"
            UPDATE async_jobs
            SET
                status = 'completed',
                result = $1,
                progress = 1.0,
                completed_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(&result)
        .bind(job_id)
        .execute(self.db_pool.as_ref())
        .await?;

        tracing::info!(job_id = %job_id, "Job completed successfully");

        Ok(())
    }

    /// Mark job as failed with error message
    pub async fn fail_job(&self, job_id: Uuid, error: String) -> Result<(), AsyncJobError> {
        query(
            r#"
            UPDATE async_jobs
            SET
                status = 'failed',
                error_message = $1,
                completed_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(&error)
        .bind(job_id)
        .execute(self.db_pool.as_ref())
        .await?;

        tracing::error!(job_id = %job_id, error = %error, "Job failed");

        Ok(())
    }

    /// Cancel a job
    pub async fn cancel_job(&self, job_id: Uuid) -> Result<(), AsyncJobError> {
        query(
            r#"
            UPDATE async_jobs
            SET status = 'cancelled'
            WHERE id = $1 AND status IN ('queued', 'in_progress')
            "#,
        )
        .bind(job_id)
        .execute(self.db_pool.as_ref())
        .await?;

        tracing::info!(job_id = %job_id, "Job cancelled");

        Ok(())
    }

    /// Clean up expired jobs
    pub async fn cleanup_expired_jobs(&self) -> Result<u64, AsyncJobError> {
        let result = query(
            r#"
            DELETE FROM async_jobs
            WHERE expires_at < NOW()
            "#,
        )
        .execute(self.db_pool.as_ref())
        .await?;

        let deleted = result.rows_affected();

        if deleted > 0 {
            tracing::info!(deleted = deleted, "Cleaned up expired async jobs");
        }

        Ok(deleted)
    }

    /// Start background cleanup task
    pub fn start_cleanup_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let interval_duration = Duration::from_secs(self.config.cleanup_interval_seconds);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_duration);

            loop {
                interval.tick().await;

                match self.cleanup_expired_jobs().await {
                    Ok(deleted) if deleted > 0 => {
                        tracing::debug!(deleted = deleted, "Async job cleanup completed");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Async job cleanup failed");
                    }
                    _ => {}
                }
            }
        })
    }

    /// List jobs for a specific client
    pub async fn list_client_jobs(
        &self,
        client_id: &str,
        limit: i64,
    ) -> Result<Vec<AsyncJob>, AsyncJobError> {
        let rows = query(
            r#"
            SELECT
                id,
                status,
                request_type,
                request_method,
                request_url,
                request_body,
                request_headers,
                result,
                progress,
                error_message,
                created_at,
                updated_at,
                completed_at,
                client_id,
                expires_at
            FROM async_jobs
            WHERE client_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(client_id)
        .bind(limit)
        .fetch_all(self.db_pool.as_ref())
        .await?;

        let jobs = rows
            .into_iter()
            .map(|row| {
                let status_str: String = row.try_get("status")?;
                let status = match status_str.as_str() {
                    "queued" => AsyncJobStatus::Queued,
                    "in_progress" => AsyncJobStatus::InProgress,
                    "completed" => AsyncJobStatus::Completed,
                    "failed" => AsyncJobStatus::Failed,
                    "cancelled" => AsyncJobStatus::Cancelled,
                    _ => AsyncJobStatus::Failed,
                };

                Ok(AsyncJob {
                    id: row.try_get("id")?,
                    status,
                    request_type: row.try_get("request_type")?,
                    request_method: row.try_get("request_method")?,
                    request_url: row.try_get("request_url")?,
                    request_body: row.try_get("request_body")?,
                    request_headers: row.try_get("request_headers")?,
                    result: row.try_get("result")?,
                    progress: row.try_get("progress").unwrap_or(0.0),
                    error_message: row.try_get("error_message")?,
                    created_at: row.try_get("created_at")?,
                    updated_at: row.try_get("updated_at")?,
                    completed_at: row.try_get("completed_at")?,
                    client_id: row.try_get("client_id")?,
                    expires_at: row.try_get("expires_at")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx_core::Error>>()?;

        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_serialization() {
        let status = AsyncJobStatus::InProgress;
        let serialized = serde_json::to_string(&status).unwrap();
        assert_eq!(serialized, "\"in_progress\"");

        let deserialized: AsyncJobStatus = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, status);
    }

    #[test]
    fn test_job_status_display() {
        assert_eq!(AsyncJobStatus::Queued.to_string(), "queued");
        assert_eq!(AsyncJobStatus::InProgress.to_string(), "in_progress");
        assert_eq!(AsyncJobStatus::Completed.to_string(), "completed");
        assert_eq!(AsyncJobStatus::Failed.to_string(), "failed");
        assert_eq!(AsyncJobStatus::Cancelled.to_string(), "cancelled");
    }
}
