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
#[serde(rename_all = "lowercase")]
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

/// Manager for async job lifecycle
#[derive(Clone)]
pub struct AsyncJobManager {
    db_pool: Arc<PgPool>,
    config: Arc<AsyncJobConfig>,
}

impl AsyncJobManager {
    /// Create a new async job manager
    pub fn new(db_pool: Arc<PgPool>, config: AsyncJobConfig) -> Self {
        Self {
            db_pool,
            config: Arc::new(config),
        }
    }

    /// Submit a new async job
    ///
    /// Creates a job record in the database and returns the job ID.
    /// The job will be in `queued` status and needs to be executed separately.
    pub async fn submit_job(&self, request: AsyncJobRequest) -> Result<Uuid, AsyncJobError> {
        use sqlx_core::{Executor, Row};

        let ttl_hours = format!("{} hours", self.config.default_ttl_hours);
        let row = sqlx_core::query(
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

        Ok(job_id)
    }

    /// Get job details by ID
    pub async fn get_job(&self, job_id: Uuid) -> Result<AsyncJob, AsyncJobError> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                status as "status: String",
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
            job_id
        )
        .fetch_optional(&*self.db_pool)
        .await?
        .ok_or_else(|| AsyncJobError::NotFound(job_id.to_string()))?;

        // Parse status string to enum
        let status = match row.status.as_str() {
            "queued" => AsyncJobStatus::Queued,
            "in_progress" => AsyncJobStatus::InProgress,
            "completed" => AsyncJobStatus::Completed,
            "failed" => AsyncJobStatus::Failed,
            "cancelled" => AsyncJobStatus::Cancelled,
            _ => AsyncJobStatus::Failed,
        };

        Ok(AsyncJob {
            id: row.id,
            status,
            request_type: row.request_type,
            request_method: row.request_method,
            request_url: row.request_url,
            request_body: row.request_body,
            request_headers: row.request_headers,
            result: row.result,
            progress: row.progress.unwrap_or(0.0),
            error_message: row.error_message,
            created_at: row.created_at,
            updated_at: row.updated_at,
            completed_at: row.completed_at,
            client_id: row.client_id,
            expires_at: row.expires_at,
        })
    }

    /// Update job status
    pub async fn update_status(
        &self,
        job_id: Uuid,
        status: AsyncJobStatus,
    ) -> Result<(), AsyncJobError> {
        let status_str = status.to_string();

        sqlx::query!(
            r#"
            UPDATE async_jobs
            SET status = $1
            WHERE id = $2
            "#,
            status_str,
            job_id
        )
        .execute(&*self.db_pool)
        .await?;

        tracing::debug!(job_id = %job_id, status = %status_str, "Job status updated");

        Ok(())
    }

    /// Update job progress (0.0 to 1.0)
    pub async fn update_progress(&self, job_id: Uuid, progress: f32) -> Result<(), AsyncJobError> {
        let clamped_progress = progress.clamp(0.0, 1.0);

        sqlx::query!(
            r#"
            UPDATE async_jobs
            SET progress = $1
            WHERE id = $2
            "#,
            clamped_progress,
            job_id
        )
        .execute(&*self.db_pool)
        .await?;

        Ok(())
    }

    /// Mark job as completed with result
    pub async fn complete_job(
        &self,
        job_id: Uuid,
        result: serde_json::Value,
    ) -> Result<(), AsyncJobError> {
        sqlx::query!(
            r#"
            UPDATE async_jobs
            SET
                status = 'completed',
                result = $1,
                progress = 1.0,
                completed_at = NOW()
            WHERE id = $2
            "#,
            result,
            job_id
        )
        .execute(&*self.db_pool)
        .await?;

        tracing::info!(job_id = %job_id, "Job completed successfully");

        Ok(())
    }

    /// Mark job as failed with error message
    pub async fn fail_job(&self, job_id: Uuid, error: String) -> Result<(), AsyncJobError> {
        sqlx::query!(
            r#"
            UPDATE async_jobs
            SET
                status = 'failed',
                error_message = $1,
                completed_at = NOW()
            WHERE id = $2
            "#,
            error,
            job_id
        )
        .execute(&*self.db_pool)
        .await?;

        tracing::error!(job_id = %job_id, error = %error, "Job failed");

        Ok(())
    }

    /// Cancel a job
    pub async fn cancel_job(&self, job_id: Uuid) -> Result<(), AsyncJobError> {
        sqlx::query!(
            r#"
            UPDATE async_jobs
            SET status = 'cancelled'
            WHERE id = $1 AND status IN ('queued', 'in_progress')
            "#,
            job_id
        )
        .execute(&*self.db_pool)
        .await?;

        tracing::info!(job_id = %job_id, "Job cancelled");

        Ok(())
    }

    /// Clean up expired jobs
    ///
    /// Deletes jobs that have passed their expiration time.
    /// This should be called periodically (e.g., every hour).
    pub async fn cleanup_expired_jobs(&self) -> Result<u64, AsyncJobError> {
        let result = sqlx::query!(
            r#"
            DELETE FROM async_jobs
            WHERE expires_at < NOW()
            "#
        )
        .execute(&*self.db_pool)
        .await?;

        let deleted = result.rows_affected();

        if deleted > 0 {
            tracing::info!(deleted = deleted, "Cleaned up expired async jobs");
        }

        Ok(deleted)
    }

    /// Start background cleanup task
    ///
    /// Spawns a tokio task that periodically cleans up expired jobs.
    /// Returns a handle that can be used to cancel the task.
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
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                status as "status: String",
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
            client_id,
            limit
        )
        .fetch_all(&*self.db_pool)
        .await?;

        let jobs = rows
            .into_iter()
            .map(|row| {
                let status = match row.status.as_str() {
                    "queued" => AsyncJobStatus::Queued,
                    "in_progress" => AsyncJobStatus::InProgress,
                    "completed" => AsyncJobStatus::Completed,
                    "failed" => AsyncJobStatus::Failed,
                    "cancelled" => AsyncJobStatus::Cancelled,
                    _ => AsyncJobStatus::Failed,
                };

                AsyncJob {
                    id: row.id,
                    status,
                    request_type: row.request_type,
                    request_method: row.request_method,
                    request_url: row.request_url,
                    request_body: row.request_body,
                    request_headers: row.request_headers,
                    result: row.result,
                    progress: row.progress.unwrap_or(0.0),
                    error_message: row.error_message,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    completed_at: row.completed_at,
                    client_id: row.client_id,
                    expires_at: row.expires_at,
                }
            })
            .collect();

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
