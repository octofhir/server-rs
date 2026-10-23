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

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    pub(crate) authorization: TrustedJobAuthorization,
}

/// Captured exclusively from the authenticated request extension. The stored
/// claims are delegation data, not a bearer credential or client secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TrustedJobAuthorization {
    claims: Option<octofhir_auth::token::AccessTokenClaims>,
    user_id: Option<String>,
}

impl TrustedJobAuthorization {
    pub(crate) async fn resume(
        &self,
        state: &crate::server::AppState,
    ) -> Result<Option<Arc<octofhir_auth::middleware::AuthContext>>, octofhir_api::ApiError> {
        let Some(claims) = &self.claims else {
            return if state.config.auth.policy.anonymous_access {
                Ok(None)
            } else {
                Err(octofhir_api::ApiError::forbidden(
                    "Anonymous job delegation is no longer enabled",
                ))
            };
        };
        if state
            .auth_state
            .revoked_token_storage
            .is_revoked(&claims.jti)
            .await
            .map_err(|_| octofhir_api::ApiError::internal("Cannot validate job delegation"))?
        {
            return Err(octofhir_api::ApiError::forbidden(
                "Job delegation was revoked",
            ));
        }
        let client = state
            .auth_state
            .client_storage
            .find_by_client_id(&claims.client_id)
            .await
            .map_err(|_| octofhir_api::ApiError::internal("Cannot validate job client"))?
            .filter(|client| client.active)
            .ok_or_else(|| octofhir_api::ApiError::forbidden("Job client is unavailable"))?;
        if !claims
            .scope
            .split_whitespace()
            .all(|scope| client.is_scope_allowed(scope))
        {
            return Err(octofhir_api::ApiError::forbidden(
                "Job delegation exceeds current client permissions",
            ));
        }
        let user = if let Some(user_id) = &self.user_id {
            let user = state
                .auth_state
                .user_storage
                .find_by_id(user_id)
                .await
                .map_err(|_| octofhir_api::ApiError::internal("Cannot validate job user"))?
                .filter(|user| user.active)
                .ok_or_else(|| octofhir_api::ApiError::forbidden("Job user is unavailable"))?;
            Some(octofhir_auth::middleware::UserContext {
                id: user.id,
                username: user.username,
                name: user.name,
                email: user.email,
                fhir_user: user.fhir_user,
                roles: user.roles,
                attributes: user.attributes,
            })
        } else {
            None
        };
        // A durable delegation outlives the bearer token used at kickoff; polling
        // and downloads still require a fresh authenticated token. Policies and
        // current client/user state are checked again by each resource operation.
        Ok(Some(Arc::new(octofhir_auth::middleware::AuthContext {
            token_claims: Arc::new(claims.clone()),
            client,
            user,
            patient: claims.patient.clone(),
            encounter: claims.encounter.clone(),
        })))
    }

    pub(crate) fn capture(
        auth: Option<&octofhir_auth::middleware::AuthContext>,
        anonymous_access: bool,
    ) -> Result<Self, octofhir_api::ApiError> {
        if auth.is_none() && anonymous_access {
            return Ok(Self {
                claims: None,
                user_id: None,
            });
        }
        let auth =
            auth.ok_or_else(|| octofhir_api::ApiError::unauthorized("Authentication required"))?;
        Ok(Self {
            claims: Some(auth.token_claims.as_ref().clone()),
            user_id: auth.user.as_ref().map(|user| user.id.clone()),
        })
    }

    fn client_id(&self) -> Option<&str> {
        self.claims.as_ref().map(|claims| claims.client_id.as_str())
    }

    pub(crate) fn permits_owner(
        &self,
        auth: Option<&octofhir_auth::middleware::AuthContext>,
        anonymous_access: bool,
    ) -> bool {
        let Some(claims) = &self.claims else {
            return anonymous_access && auth.is_none();
        };
        let Some(auth) = auth else {
            return false;
        };
        let current = auth.token_claims.as_ref();
        let scopes: std::collections::HashSet<_> = current.scope.split_whitespace().collect();
        current.iss == claims.iss
            && current.sub == claims.sub
            && current.client_id == claims.client_id
            && current.patient == claims.patient
            && current.encounter == claims.encounter
            && auth.user.as_ref().map(|user| &user.id) == self.user_id.as_ref()
            && claims
                .scope
                .split_whitespace()
                .all(|scope| scopes.contains(scope))
    }
}

impl AsyncJob {
    pub(crate) fn authorization(&self) -> Result<TrustedJobAuthorization, octofhir_api::ApiError> {
        self.request_headers
            .as_ref()
            .and_then(|headers| headers.get("authorization"))
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .ok_or_else(|| octofhir_api::ApiError::not_found("Async job is unavailable"))
    }

    pub(crate) fn authorize_owner(
        &self,
        auth: Option<&octofhir_auth::middleware::AuthContext>,
        anonymous_access: bool,
    ) -> Result<(), octofhir_api::ApiError> {
        if self.expires_at <= Utc::now()
            || self.status == AsyncJobStatus::Cancelled
            || !self.authorization()?.permits_owner(auth, anonymous_access)
        {
            return Err(octofhir_api::ApiError::not_found(
                "Async job is unavailable",
            ));
        }
        Ok(())
    }
}

/// Configuration for async job manager
#[derive(Debug, Clone)]
pub struct AsyncJobConfig {
    pub enabled: bool,
    pub max_concurrent_jobs: usize,
    pub max_concurrent_imports: usize,
    pub default_ttl_hours: i64,
    pub cleanup_interval_seconds: u64,
}

impl Default for AsyncJobConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_jobs: 10,
            max_concurrent_imports: 3,
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct ExecutionClaim {
    pub job_id: Uuid,
    pub execution_id: Uuid,
}

tokio::task_local! {
    static EXECUTION_CLAIM: ExecutionClaim;
}

// Only worker code selects a durable step; request headers cannot create it.
tokio::task_local! {
    static CHECKPOINT_KEY: CheckpointStep;
}

#[derive(Clone)]
pub(crate) struct CheckpointStep {
    pub(crate) key: String,
    pub(crate) fingerprint: String,
    pub(crate) metadata: serde_json::Value,
    pub(crate) count_only: bool,
}

impl CheckpointStep {
    pub(crate) fn new(
        key: String,
        input: &serde_json::Value,
    ) -> Result<Self, octofhir_api::ApiError> {
        let bytes = serde_json::to_vec(input)
            .map_err(|error| octofhir_api::ApiError::internal(error.to_string()))?;
        Ok(Self {
            key,
            fingerprint: hex::encode(Sha256::digest(bytes)),
            metadata: serde_json::Value::Null,
            count_only: false,
        })
    }

    fn record(&self, result: &serde_json::Value) -> serde_json::Value {
        let result = if self.count_only {
            serde_json::json!({"created": result["entry"].as_array().map_or(0, Vec::len)})
        } else {
            result.clone()
        };
        serde_json::json!({"fingerprint":self.fingerprint, "result":result, "metadata":self.metadata})
    }

    pub(crate) async fn scope<F: std::future::Future>(self, future: F) -> F::Output {
        CHECKPOINT_KEY.scope(self, future).await
    }
}

pub(crate) async fn save_transaction_result(
    tx: &mut dyn octofhir_storage::Transaction,
    result: &serde_json::Value,
    max_bytes: usize,
) -> Result<(), octofhir_api::ApiError> {
    let Some(claim) = current_execution_claim() else {
        return Ok(());
    };
    let step = CHECKPOINT_KEY.try_with(Clone::clone).ok();
    let record = step.as_ref().map(|step| step.record(result));
    let result = record.as_ref().unwrap_or(result);
    ensure_result_budget(result, max_bytes)?;
    tx.save_async_job_checkpoint(
        claim.job_id,
        claim.execution_id,
        step.as_ref().map(|step| step.key.as_str()),
        result,
    )
    .await
    .map_err(crate::handlers::map_storage_error)
}

pub(crate) fn ensure_result_budget(
    result: &serde_json::Value,
    max_bytes: usize,
) -> Result<(), octofhir_api::ApiError> {
    struct ByteBudget {
        remaining: usize,
    }
    impl std::io::Write for ByteBudget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.remaining = self.remaining.checked_sub(bytes.len()).ok_or_else(|| {
                std::io::Error::other("Async result exceeds the configured byte budget")
            })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(
        ByteBudget {
            remaining: max_bytes,
        },
        result,
    )
    .map_err(|_| {
        octofhir_api::ApiError::bad_request("Async result exceeds the configured byte budget")
    })?;
    Ok(())
}

pub(crate) fn current_execution_claim() -> Option<ExecutionClaim> {
    EXECUTION_CLAIM.try_with(|claim| *claim).ok()
}

fn execution_id(job_id: Uuid) -> Result<Uuid, AsyncJobError> {
    EXECUTION_CLAIM
        .try_with(|claim| (claim.job_id == job_id).then_some(claim.execution_id))
        .ok()
        .flatten()
        .ok_or_else(|| AsyncJobError::InvalidStatusTransition {
            from: "unclaimed".into(),
            to: "worker mutation".into(),
        })
}

struct ClaimedJob {
    claim: ExecutionClaim,
    request_type: String,
    method: String,
    url: String,
    body: Option<serde_json::Value>,
}

/// Manager for async job lifecycle
#[derive(Clone)]
pub struct AsyncJobManager {
    db_pool: Arc<PgPool>,
    config: Arc<AsyncJobConfig>,
    executor: Arc<std::sync::RwLock<Option<JobExecutor>>>,
    /// Caps concurrently executing jobs at `config.max_concurrent_jobs` so a
    /// submit burst cannot spawn unbounded background tasks.
    job_semaphore: Arc<tokio::sync::Semaphore>,
    import_semaphore: Arc<tokio::sync::Semaphore>,
    dispatcher_started: Arc<AtomicBool>,
    wake_dispatcher: Arc<tokio::sync::Notify>,
    background: crate::lifecycle::BackgroundRuntime,
}

impl AsyncJobManager {
    /// Create a new async job manager
    pub fn new(db_pool: Arc<PgPool>, config: AsyncJobConfig) -> Self {
        let permits = config.max_concurrent_jobs.max(1);
        let import_permits = config.max_concurrent_imports.max(1);
        // Lifecycle requests must remain serviceable while resource workers hold
        // all primary-pool connections (including a configured one-connection
        // pool). This small lazy pool is only for job control, never resource SQL.
        let control_pool = sqlx_postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_lazy_with(db_pool.connect_options().as_ref().clone());
        Self {
            db_pool: Arc::new(control_pool),
            config: Arc::new(config),
            executor: Arc::new(std::sync::RwLock::new(None)),
            job_semaphore: Arc::new(tokio::sync::Semaphore::new(permits)),
            import_semaphore: Arc::new(tokio::sync::Semaphore::new(import_permits)),
            dispatcher_started: Arc::new(AtomicBool::new(false)),
            wake_dispatcher: Arc::new(tokio::sync::Notify::new()),
            background: crate::lifecycle::BackgroundRuntime::default(),
        }
    }

    pub(crate) fn with_background_runtime(
        mut self,
        background: crate::lifecycle::BackgroundRuntime,
    ) -> Self {
        self.background = background;
        self
    }

    /// Stop accepting/claiming jobs, drop in-flight execution, and await fenced
    /// claim release. Queued and checkpointed jobs remain durable for recovery.
    pub async fn shutdown(&self) {
        self.background.stop();
        self.background.wait().await;
    }

    /// Set the job executor function
    pub fn with_executor(self, executor: JobExecutor) -> Self {
        self.set_executor(executor);
        self
    }

    /// Set the job executor function after construction
    /// This allows setting the executor after the manager has been created and shared
    pub fn set_executor(&self, executor: JobExecutor) {
        *self.executor.write().unwrap() = Some(executor);
        if !self.dispatcher_started.swap(true, Ordering::AcqRel) && self.config.enabled {
            let manager = self.clone();
            self.background.spawn(async move {
                manager.dispatch_jobs().await;
            });
        }
        self.wake_dispatcher.notify_one();
    }

    /// Queued requests stay in the database; only claimed jobs consume a task
    /// and permit. The periodic scan also discovers other replicas' submissions.
    async fn dispatch_jobs(&self) {
        let mut scan = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = scan.tick() => {},
                _ = self.wake_dispatcher.notified() => {},
            }
            loop {
                if self.background.cancellation.is_cancelled() {
                    return;
                }
                let Ok(permit) = self.job_semaphore.clone().try_acquire_owned() else {
                    break;
                };
                let executor = self.executor.read().unwrap().clone();
                let Some(executor) = executor else {
                    break;
                };
                let import_permit = self.import_semaphore.clone().try_acquire_owned().ok();
                match self.claim_job_matching(import_permit.is_some()).await {
                    Ok(Some(job)) => {
                        let import_permit = if job.request_type == "bulk_import" {
                            import_permit
                        } else {
                            None
                        };
                        let manager = self.clone();
                        self.background.spawn_tracked(async move {
                            manager.execute_job(executor, job).await;
                            drop(permit);
                            drop(import_permit);
                            manager.wake_dispatcher.notify_one();
                        });
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::error!(%error, "Cannot claim an asynchronous job");
                        break;
                    }
                }
            }
        }
    }

    #[cfg(test)]
    async fn claim_job(&self) -> Result<Option<ClaimedJob>, AsyncJobError> {
        self.claim_job_matching(true).await
    }

    async fn claim_job_matching(
        &self,
        include_imports: bool,
    ) -> Result<Option<ClaimedJob>, AsyncJobError> {
        // Recover only writes created by the native checkpoint protocol.
        // Legacy import/batch jobs may have committed without a checkpoint and
        // must never be blindly replayed.
        query(
            "UPDATE async_jobs SET status = 'failed', completed_at = clock_timestamp(), \
               error_message = 'Execution was interrupted; automatic write replay is unavailable' \
               WHERE status = 'in_progress' AND expires_at > clock_timestamp() \
               AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp()) \
               AND NOT (request_type IN ('bulk_export', 'viewdefinition_export', 'bundle-transaction') \
               OR (request_type = 'bulk_import' AND COALESCE(request_headers->>'write_protocol', '') = 'native-chunks-v1'))",
        )
        .execute(self.db_pool.as_ref())
        .await?;
        let attempt = Uuid::new_v4();
        let row = query(
            "WITH candidate AS (SELECT id FROM async_jobs \
             WHERE expires_at > clock_timestamp() AND request_headers->'authorization' IS NOT NULL \
             AND ($2 OR request_type != 'bulk_import') \
             AND (status = 'queued' OR (status = 'in_progress' \
             AND (request_type IN ('bulk_export', 'viewdefinition_export', 'bundle-transaction') \
             OR (request_type = 'bulk_import' AND COALESCE(request_headers->>'write_protocol', '') = 'native-chunks-v1')) \
             AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp()))) \
             ORDER BY created_at, id LIMIT 1 FOR UPDATE SKIP LOCKED) \
             UPDATE async_jobs j SET status = 'in_progress', execution_id = $1, \
             lease_expires_at = LEAST(j.expires_at, clock_timestamp() + INTERVAL '30 seconds'), \
             attempts = j.attempts + 1 FROM candidate c WHERE j.id = c.id \
             RETURNING j.id, j.request_type, j.request_method, j.request_url, j.request_body",
        )
        .bind(attempt)
        .bind(include_imports)
        .fetch_optional(self.db_pool.as_ref())
        .await?;
        row.map(|row| {
            Ok(ClaimedJob {
                claim: ExecutionClaim {
                    job_id: row.try_get("id")?,
                    execution_id: attempt,
                },
                request_type: row.try_get("request_type")?,
                method: row.try_get("request_method")?,
                url: row.try_get("request_url")?,
                body: row.try_get("request_body")?,
            })
        })
        .transpose()
    }

    /// Submit a new async job
    pub async fn submit_job(&self, request: AsyncJobRequest) -> Result<Uuid, AsyncJobError> {
        if !self.config.enabled {
            return Err(AsyncJobError::ExecutionFailed(
                "Asynchronous jobs are disabled".into(),
            ));
        }
        if self.background.cancellation.is_cancelled() {
            return Err(AsyncJobError::ExecutionFailed(
                "Server is shutting down".into(),
            ));
        }
        let headers = serde_json::json!({"http":request.headers,"authorization":request.authorization,
                "write_protocol": if request.request_type == "bulk_import" {Some("native-chunks-v1")} else {None}});
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
        .bind(&headers)
        .bind(request.authorization.client_id())
        .bind(&ttl_hours)
        .fetch_one(self.db_pool.as_ref())
        .await?;

        let job_id: Uuid = row.try_get("id")?;

        tracing::info!(
            job_id = %job_id,
            request_type = %request.request_type,
            "Async job created"
        );

        self.wake_dispatcher.notify_one();
        Ok(job_id)
    }

    /// Renew the claim concurrently with execution. A database wait in a lease
    /// heartbeat must not stop polling a resource transaction that holds a job
    /// checkpoint lock and is about to commit.
    async fn execute_job(&self, executor: JobExecutor, job: ClaimedJob) {
        let claim = job.claim;
        EXECUTION_CLAIM.scope(claim, async {
            let job_id = claim.job_id;
            let execution = executor(job_id, job.request_type, job.method, job.url, job.body);
            let lease = async {
                let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
                loop {
                    heartbeat.tick().await;
                    let renewed = query(
                        "UPDATE async_jobs SET lease_expires_at = LEAST(expires_at, clock_timestamp() + INTERVAL '30 seconds') \
                         WHERE id = $1 AND execution_id = $2 AND status = 'in_progress' \
                         AND expires_at > clock_timestamp() AND lease_expires_at > clock_timestamp()",
                    ).bind(job_id).bind(claim.execution_id).execute(self.db_pool.as_ref()).await?;
                    if renewed.rows_affected() == 0 {
                        let committed = query("SELECT 1 FROM async_jobs WHERE id = $1 AND execution_id = $2 AND status = 'completed'")
                            .bind(job_id).bind(claim.execution_id).fetch_optional(self.db_pool.as_ref()).await?;
                        if committed.is_none() { return Ok::<(), sqlx_core::Error>(()); }
                    }
                }
            };
            let mut execution = Box::pin(execution);
            let mut lease = Box::pin(lease);
            let result = tokio::select! {
                biased;
                _ = self.background.cancellation.cancelled() => {
                    // SQLx rolls back the dropped resource transaction before
                    // the control connection can release a locked checkpoint.
                    drop(execution);
                    drop(lease);
                    let release = query("UPDATE async_jobs SET lease_expires_at = clock_timestamp() \
                        WHERE id = $1 AND execution_id = $2 AND status = 'in_progress'")
                        .bind(job_id).bind(claim.execution_id).execute(self.db_pool.as_ref());
                    match tokio::time::timeout(Duration::from_secs(5), release).await {
                        Ok(Ok(_)) => {},
                        outcome => tracing::warn!(%job_id, ?outcome, "Claim will recover after its lease expires"),
                    }
                    return;
                }
                result = &mut execution => result,
                stopped = &mut lease => {
                    if let Err(error) = stopped {
                        tracing::error!(%job_id, %error, "Async execution lost its control connection");
                    }
                    tracing::info!(%job_id, "Async execution stopped after cancellation or loss of its lease");
                    return;
                }
            };
            match result {
                Ok(result) => {
                    if let Err(error) = self.complete_job(job_id, result).await {
                        tracing::error!(%job_id, %error, "Cannot complete asynchronous job");
                    }
                }
                Err(error) => {
                    if let Err(error) = self.fail_job(job_id, error).await {
                        tracing::error!(%job_id, %error, "Cannot fail asynchronous job");
                    }
                }
            }
        }).await;
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
            progress: row.try_get::<Option<f64>, _>("progress")?.unwrap_or(0.0) as f32,
            error_message: row.try_get("error_message")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            completed_at: row.try_get("completed_at")?,
            client_id: row.try_get("client_id")?,
            expires_at: row.try_get("expires_at")?,
        })
    }

    /// Update job progress (0.0 to 1.0)
    pub async fn update_progress(&self, job_id: Uuid, progress: f32) -> Result<(), AsyncJobError> {
        let attempt = execution_id(job_id)?;
        let clamped_progress = progress.clamp(0.0, 1.0);

        let updated = query(
            r#"
            UPDATE async_jobs
            SET progress = $1
            WHERE id = $2 AND execution_id = $3 AND status = 'in_progress'
                AND expires_at > clock_timestamp() AND lease_expires_at > clock_timestamp()
            "#,
        )
        .bind(clamped_progress)
        .bind(job_id)
        .bind(attempt)
        .execute(self.db_pool.as_ref())
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AsyncJobError::InvalidStatusTransition {
                from: "unavailable".into(),
                to: "progress".into(),
            });
        }
        Ok(())
    }

    pub(crate) async fn restore_checkpoint(
        &self,
        step: &CheckpointStep,
    ) -> Result<Option<serde_json::Value>, octofhir_api::ApiError> {
        let claim = current_execution_claim()
            .ok_or_else(|| octofhir_api::ApiError::internal("Worker has no execution claim"))?;
        let row = query(
            "SELECT checkpoint -> $3::text AS step FROM async_jobs \
             WHERE id = $1 AND execution_id = $2 AND status = 'in_progress' \
             AND expires_at > clock_timestamp() AND lease_expires_at > clock_timestamp()",
        )
        .bind(claim.job_id)
        .bind(claim.execution_id)
        .bind(&step.key)
        .fetch_optional(self.db_pool.as_ref())
        .await
        .map_err(|_| octofhir_api::ApiError::internal("Cannot load worker checkpoint"))?
        .ok_or_else(|| octofhir_api::ApiError::conflict("Worker execution claim is unavailable"))?;
        let record: Option<serde_json::Value> = row
            .try_get("step")
            .map_err(|_| octofhir_api::ApiError::internal("Invalid worker checkpoint"))?;
        let Some(record) = record else {
            return Ok(None);
        };
        if record["fingerprint"].as_str() != Some(&step.fingerprint) {
            return Err(octofhir_api::ApiError::conflict(
                "Input differs from the committed worker checkpoint",
            ));
        }
        if record.get("result").is_none() {
            return Err(octofhir_api::ApiError::internal(
                "Invalid worker checkpoint result",
            ));
        }
        Ok(Some(record))
    }

    /// Checkpoint a read or validation failure with no resource mutation.
    /// Successful mutations use Transaction::save_async_job_checkpoint instead.
    pub(crate) async fn save_read_checkpoint(
        &self,
        step: &CheckpointStep,
        result: &serde_json::Value,
        max_bytes: usize,
    ) -> Result<(), octofhir_api::ApiError> {
        let claim = current_execution_claim()
            .ok_or_else(|| octofhir_api::ApiError::internal("Worker has no execution claim"))?;
        let record = step.record(result);
        ensure_result_budget(&record, max_bytes)?;
        let updated = query(
            "UPDATE async_jobs SET checkpoint = jsonb_set(checkpoint, ARRAY[$3::text], $4::jsonb, true) \
             WHERE id = $1 AND execution_id = $2 AND status = 'in_progress' \
             AND expires_at > clock_timestamp() AND lease_expires_at > clock_timestamp() \
             AND (NOT checkpoint ? $3::text OR checkpoint -> $3::text = $4::jsonb)",
        ).bind(claim.job_id).bind(claim.execution_id).bind(&step.key).bind(record)
            .execute(self.db_pool.as_ref()).await
            .map_err(|_| octofhir_api::ApiError::internal("Cannot save worker checkpoint"))?;
        if updated.rows_affected() != 1 {
            return Err(octofhir_api::ApiError::conflict(
                "Worker execution claim is unavailable",
            ));
        }
        Ok(())
    }

    /// Mark job as completed with result
    pub async fn complete_job(
        &self,
        job_id: Uuid,
        result: serde_json::Value,
    ) -> Result<(), AsyncJobError> {
        let attempt = execution_id(job_id)?;
        let updated = query(
            r#"
            UPDATE async_jobs
            SET
                status = 'completed',
                result = $1,
                progress = 1.0,
                completed_at = NOW()
            WHERE id = $2 AND execution_id = $3 AND status = 'in_progress'
                AND expires_at > clock_timestamp() AND lease_expires_at > clock_timestamp()
            "#,
        )
        .bind(&result)
        .bind(job_id)
        .bind(attempt)
        .execute(self.db_pool.as_ref())
        .await?;
        if updated.rows_affected() != 1 {
            let committed = query("SELECT 1 FROM async_jobs WHERE id = $1 AND execution_id = $2 AND status = 'completed' AND result = $3::jsonb")
                .bind(job_id).bind(attempt).bind(&result).fetch_optional(self.db_pool.as_ref()).await?;
            if committed.is_none() {
                return Err(AsyncJobError::InvalidStatusTransition {
                    from: "unavailable".into(),
                    to: "completed".into(),
                });
            }
        }

        tracing::info!(job_id = %job_id, "Job completed successfully");

        Ok(())
    }

    /// Mark job as failed with error message
    pub async fn fail_job(&self, job_id: Uuid, error: String) -> Result<(), AsyncJobError> {
        let attempt = execution_id(job_id)?;
        let updated = query(
            r#"
            UPDATE async_jobs
            SET
                status = 'failed',
                error_message = $1,
                completed_at = NOW()
            WHERE id = $2 AND execution_id = $3 AND status = 'in_progress'
                AND expires_at > clock_timestamp() AND lease_expires_at > clock_timestamp()
            "#,
        )
        .bind(&error)
        .bind(job_id)
        .bind(attempt)
        .execute(self.db_pool.as_ref())
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AsyncJobError::InvalidStatusTransition {
                from: "unavailable".into(),
                to: "failed".into(),
            });
        }

        tracing::error!(job_id = %job_id, error = %error, "Job failed");

        Ok(())
    }

    /// Cancel a job
    pub async fn cancel_job(&self, job_id: Uuid) -> Result<(), AsyncJobError> {
        let updated = query(
            r#"
            UPDATE async_jobs
            SET status = 'cancelled', completed_at = NOW(), result = NULL
            WHERE id = $1 AND status != 'cancelled' AND expires_at > NOW()
            "#,
        )
        .bind(job_id)
        .execute(self.db_pool.as_ref())
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AsyncJobError::NotFound(job_id.to_string()));
        }

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

        self.background.clone().spawn(async move {
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
                    progress: row.try_get::<Option<f64>, _>("progress")?.unwrap_or(0.0) as f32,
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

    #[tokio::test]
    async fn durable_claims_recover_exports_and_fence_previous_attempts() {
        use testcontainers::{ImageExt, runners::AsyncRunner};
        let postgres = testcontainers_modules::postgres::Postgres::default()
            .with_tag("17-alpine")
            .start()
            .await
            .unwrap();
        let port = postgres.get_host_port_ipv4(5432).await.unwrap();
        let database = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
        let pool = Arc::new(
            sqlx_postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&database)
                .await
                .unwrap(),
        );
        octofhir_db_postgres::migrations::run(&pool, &database)
            .await
            .unwrap();
        let first = AsyncJobManager::new(pool.clone(), AsyncJobConfig::default());
        let request = |request_type: &str| AsyncJobRequest {
            request_type: request_type.into(),
            method: "POST".into(),
            url: "/$export".into(),
            body: Some(serde_json::json!({"probe":"persistent"})),
            headers: None,
            client_id: None,
            authorization: TrustedJobAuthorization::capture(None, true).unwrap(),
        };
        let id = first.submit_job(request("bulk_export")).await.unwrap();
        // A manager created after submission discovers the persisted queue.
        let second = AsyncJobManager::new(pool.clone(), AsyncJobConfig::default());
        let (left, right) = tokio::join!(first.claim_job(), second.claim_job());
        let claims: Vec<_> = [left.unwrap(), right.unwrap()]
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(claims.len(), 1, "A queued job must have exactly one claim");
        let previous = claims.into_iter().next().unwrap();
        assert_eq!(previous.claim.job_id, id);
        assert_eq!(previous.body.unwrap()["probe"], "persistent");
        query("UPDATE async_jobs SET lease_expires_at = clock_timestamp() - INTERVAL '1 second' WHERE id = $1")
            .bind(id).execute(pool.as_ref()).await.unwrap();
        let recovered = second.claim_job().await.unwrap().unwrap();
        assert_eq!(recovered.claim.job_id, id);
        assert_ne!(previous.claim.execution_id, recovered.claim.execution_id);
        EXECUTION_CLAIM
            .scope(previous.claim, async {
                assert!(first.update_progress(id, 0.99).await.is_err());
                assert!(
                    first
                        .complete_job(id, serde_json::json!({"stale":true}))
                        .await
                        .is_err()
                );
                assert!(first.fail_job(id, "stale failure".into()).await.is_err());
            })
            .await;
        EXECUTION_CLAIM
            .scope(recovered.claim, async {
                second.update_progress(id, 0.5).await.unwrap();
                assert_eq!(second.get_job(id).await.unwrap().progress, 0.5);
                second
                    .complete_job(id, serde_json::json!({"recovered":true}))
                    .await
                    .unwrap();
            })
            .await;
        let saved = second.get_job(id).await.unwrap();
        assert_eq!(saved.status, AsyncJobStatus::Completed);
        assert_eq!(saved.result.unwrap(), serde_json::json!({"recovered":true}));
        let attempts: i64 =
            sqlx_core::query_scalar::query_scalar("SELECT attempts FROM async_jobs WHERE id = $1")
                .bind(id)
                .fetch_one(pool.as_ref())
                .await
                .unwrap();
        assert_eq!(attempts, 2);
        // A write without an atomic checkpoint cannot be blindly re-executed.
        let write_id = first.submit_job(request("bundle-batch")).await.unwrap();
        let write = first.claim_job().await.unwrap().unwrap();
        assert_eq!(write.claim.job_id, write_id);
        query("UPDATE async_jobs SET lease_expires_at = clock_timestamp() - INTERVAL '1 second' WHERE id = $1")
            .bind(write_id).execute(pool.as_ref()).await.unwrap();
        assert!(second.claim_job().await.unwrap().is_none());
        assert_eq!(
            second.get_job(write_id).await.unwrap().status,
            AsyncJobStatus::Failed
        );

        use octofhir_storage::FhirStorage;
        octofhir_db_postgres::SchemaManager::ensure_archive_function(&pool)
            .await
            .unwrap();
        octofhir_db_postgres::SchemaManager::new(pool.as_ref().clone())
            .create_resource_schema("Patient")
            .await
            .unwrap();
        let broadcaster = octofhir_core::events::EventBroadcaster::new_shared();
        let mut events = broadcaster.subscribe();
        let storage = octofhir_storage::EventedStorage::new(
            octofhir_db_postgres::PostgresStorage::from_pool(pool.as_ref().clone()),
            broadcaster,
        );
        let atomic_id = first
            .submit_job(request("bundle-transaction"))
            .await
            .unwrap();
        let interrupted = first.claim_job().await.unwrap().unwrap();
        let mut tx = storage.begin_transaction().await.unwrap();
        tx.create(&serde_json::json!({"resourceType":"Patient","id":"atomic-once","name":[{"family":"Иванов"}]}))
            .await.unwrap();
        query("UPDATE async_jobs SET lease_expires_at = clock_timestamp() - INTERVAL '1 second' WHERE id = $1")
            .bind(atomic_id).execute(pool.as_ref()).await.unwrap();
        let recovered = second.claim_job().await.unwrap().unwrap();
        assert_eq!(recovered.claim.job_id, atomic_id);
        assert!(
            tx.save_async_job_checkpoint(
                atomic_id,
                interrupted.claim.execution_id,
                None,
                &serde_json::json!({"stale":true})
            )
            .await
            .is_err()
        );
        tx.rollback().await.unwrap();
        assert!(
            storage
                .read("Patient", "atomic-once")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            events.try_recv().is_err(),
            "A fenced transaction cannot emit an event"
        );

        let mut tx = storage.begin_transaction().await.unwrap();
        let saved = tx.create(&serde_json::json!({"resourceType":"Patient","id":"atomic-once","name":[{"family":"Иванов"}]}))
            .await.unwrap();
        let result = serde_json::json!({"resourceType":"Bundle","type":"transaction-response",
            "entry":[{"resource":saved.resource,"response":{"status":"201 Created"}}]});
        EXECUTION_CLAIM
            .scope(
                recovered.claim,
                save_transaction_result(&mut *tx, &result, 1024 * 1024),
            )
            .await
            .unwrap();
        // No clinical snapshot or final job result becomes visible until commit.
        assert!(
            storage
                .read("Patient", "atomic-once")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            second.get_job(atomic_id).await.unwrap().status,
            AsyncJobStatus::InProgress
        );
        tx.commit().await.unwrap();
        let committed = second.get_job(atomic_id).await.unwrap();
        assert_eq!(committed.status, AsyncJobStatus::Completed);
        assert_eq!(committed.result.as_ref(), Some(&result));
        EXECUTION_CLAIM
            .scope(
                recovered.claim,
                second.complete_job(atomic_id, result.clone()),
            )
            .await
            .unwrap();
        assert!(
            second.claim_job().await.unwrap().is_none(),
            "A committed POST cannot be reclaimed"
        );
        assert_eq!(
            storage
                .read("Patient", "atomic-once")
                .await
                .unwrap()
                .unwrap()
                .resource,
            saved.resource
        );
        let history = storage
            .history("Patient", Some("atomic-once"), &Default::default())
            .await
            .unwrap();
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].resource.resource, saved.resource);
        let event = events.try_recv().unwrap();
        let octofhir_core::events::SystemEvent::Resource(event) = event else {
            panic!("Expected clinical resource event");
        };
        assert_eq!(event.resource.as_deref(), Some(&saved.resource));
        assert!(
            events.try_recv().is_err(),
            "A committed checkpoint must not duplicate events"
        );

        // Native chunk checkpoints remain in progress and recover without
        // repeating an already committed create, history version or event.
        let import_id = first.submit_job(request("bulk_import")).await.unwrap();
        let import = first.claim_job().await.unwrap().unwrap();
        assert_eq!(import.claim.job_id, import_id);
        let step =
            CheckpointStep::new("import-0-1".into(), &serde_json::json!({"line":"original"}))
                .unwrap();
        let mut tx = storage.begin_transaction().await.unwrap();
        let imported = tx.create(&serde_json::json!({"resourceType":"Patient","id":"import-once", "meta":{"profile":["http://example.org/Profile"],"tag":[{"system":"urn:test","code":"keep"}]}})).await.unwrap();
        let chunk_result = serde_json::json!({"resourceType":"Bundle","type":"transaction-response","entry":[{"response":{"status":"201 Created"}}]});
        EXECUTION_CLAIM
            .scope(
                import.claim,
                step.clone().scope(save_transaction_result(
                    &mut *tx,
                    &chunk_result,
                    1024 * 1024,
                )),
            )
            .await
            .unwrap();
        assert!(
            storage
                .read("Patient", "import-once")
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            first.get_job(import_id).await.unwrap().status,
            AsyncJobStatus::InProgress
        );
        assert!(first.get_job(import_id).await.unwrap().result.is_none());
        let octofhir_core::events::SystemEvent::Resource(event) = events.try_recv().unwrap() else {
            panic!("Missing import event")
        };
        assert_eq!(event.resource.as_deref(), Some(&imported.resource));
        query("UPDATE async_jobs SET lease_expires_at = clock_timestamp() - INTERVAL '1 second' WHERE id = $1").bind(import_id).execute(pool.as_ref()).await.unwrap();
        let resumed = second.claim_job().await.unwrap().unwrap();
        assert_eq!(resumed.claim.job_id, import_id);
        EXECUTION_CLAIM
            .scope(resumed.claim, async {
                let record = second.restore_checkpoint(&step).await.unwrap().unwrap();
                assert_eq!(record["result"], chunk_result);
                let changed = CheckpointStep::new(
                    step.key.clone(),
                    &serde_json::json!({"line":"changed suffix"}),
                )
                .unwrap();
                assert!(second.restore_checkpoint(&changed).await.is_err());
                assert!(
                    second
                        .save_read_checkpoint(&changed, &serde_json::Value::Null, 1024 * 1024)
                        .await
                        .is_err()
                );
                assert_eq!(
                    second.restore_checkpoint(&step).await.unwrap().unwrap(),
                    record
                );
                second
                    .complete_job(import_id, serde_json::json!({"total_created":1}))
                    .await
                    .unwrap();
            })
            .await;
        let history = storage
            .history("Patient", Some("import-once"), &Default::default())
            .await
            .unwrap();
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].resource.resource, imported.resource);
        assert!(
            events.try_recv().is_err(),
            "Recovered chunk must not duplicate events"
        );

        // Version markers are server-owned. An old job without the marker
        // cannot be replayed, even when it contains similar request JSON.
        let legacy_id = first.submit_job(request("bulk_import")).await.unwrap();
        query("UPDATE async_jobs SET request_headers = request_headers - 'write_protocol' WHERE id = $1").bind(legacy_id).execute(pool.as_ref()).await.unwrap();
        let legacy = first.claim_job().await.unwrap().unwrap();
        assert_eq!(legacy.claim.job_id, legacy_id);
        query("UPDATE async_jobs SET lease_expires_at = clock_timestamp() - INTERVAL '1 second' WHERE id = $1").bind(legacy_id).execute(pool.as_ref()).await.unwrap();
        assert!(second.claim_job().await.unwrap().is_none());
        assert_eq!(
            second.get_job(legacy_id).await.unwrap().status,
            AsyncJobStatus::Failed
        );

        // Cancellation wins over a native write that has not committed its
        // checkpoint, so neither data nor history nor events can escape.
        let cancelled_id = first.submit_job(request("bulk_import")).await.unwrap();
        let cancelled = first.claim_job().await.unwrap().unwrap();
        let mut tx = storage.begin_transaction().await.unwrap();
        tx.create(&serde_json::json!({"resourceType":"Patient","id":"cancel-import"}))
            .await
            .unwrap();
        second.cancel_job(cancelled_id).await.unwrap();
        assert!(
            EXECUTION_CLAIM
                .scope(
                    cancelled.claim,
                    step.scope(save_transaction_result(
                        &mut *tx,
                        &chunk_result,
                        1024 * 1024
                    ))
                )
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        assert!(
            storage
                .read("Patient", "cancel-import")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            storage
                .history("Patient", Some("cancel-import"), &Default::default())
                .await
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(events.try_recv().is_err());

        let budget_id = first
            .submit_job(request("bundle-transaction"))
            .await
            .unwrap();
        let budget = first.claim_job().await.unwrap().unwrap();
        let mut tx = storage.begin_transaction().await.unwrap();
        let saved = tx
            .create(&serde_json::json!({"resourceType":"Patient","id":"over-budget"}))
            .await
            .unwrap();
        assert!(
            EXECUTION_CLAIM
                .scope(
                    budget.claim,
                    save_transaction_result(&mut *tx, &saved.resource, 1)
                )
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        EXECUTION_CLAIM
            .scope(
                budget.claim,
                first.fail_job(budget_id, "Response byte budget exceeded".into()),
            )
            .await
            .unwrap();
        assert!(
            storage
                .read("Patient", "over-budget")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            storage
                .history("Patient", Some("over-budget"), &Default::default())
                .await
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(
            events.try_recv().is_err(),
            "Oversized async result must roll back events"
        );
    }

    #[tokio::test]
    async fn router_shutdown_rolls_back_worker_and_releases_only_its_claim() {
        use octofhir_storage::FhirStorage;
        use testcontainers::{ImageExt, runners::AsyncRunner};
        let postgres = testcontainers_modules::postgres::Postgres::default()
            .with_tag("17-alpine")
            .start()
            .await
            .unwrap();
        let port = postgres.get_host_port_ipv4(5432).await.unwrap();
        let database = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
        let pool = Arc::new(
            sqlx_postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&database)
                .await
                .unwrap(),
        );
        octofhir_db_postgres::migrations::run(&pool, &database)
            .await
            .unwrap();
        octofhir_db_postgres::SchemaManager::ensure_archive_function(&pool)
            .await
            .unwrap();
        octofhir_db_postgres::SchemaManager::new(pool.as_ref().clone())
            .create_resource_schema("Patient")
            .await
            .unwrap();
        let events = octofhir_core::events::EventBroadcaster::new_shared();
        let mut receive_events = events.subscribe();
        let storage = Arc::new(octofhir_storage::EventedStorage::new(
            octofhir_db_postgres::PostgresStorage::from_pool(pool.as_ref().clone()),
            events,
        ));
        let manager = AsyncJobManager::new(
            pool.clone(),
            AsyncJobConfig {
                max_concurrent_jobs: 1,
                ..Default::default()
            },
        );
        let owner = manager.background.owner();
        let (started, mut receive) = tokio::sync::mpsc::unbounded_channel();
        let worker_storage = storage.clone();
        manager.set_executor(Arc::new(move |id, _, _, _, _| {
            let storage = worker_storage.clone();
            let started = started.clone();
            Box::pin(async move {
                let mut tx = storage
                    .begin_transaction()
                    .await
                    .map_err(|e| e.to_string())?;
                tx.create(&serde_json::json!({"resourceType":"Patient","id":"shutdown-rollback"}))
                    .await
                    .map_err(|e| e.to_string())?;
                let claim = current_execution_claim().unwrap();
                tx.save_async_job_checkpoint(
                    id,
                    claim.execution_id,
                    Some("probe"),
                    &serde_json::json!({"held":true}),
                )
                .await
                .map_err(|e| e.to_string())?;
                started.send(id).unwrap();
                std::future::pending::<()>().await;
                tx.commit().await.map_err(|e| e.to_string())?;
                Ok(serde_json::json!({"unexpected":true}))
            })
        }));
        let request = || AsyncJobRequest {
            request_type: "bundle-transaction".into(),
            method: "POST".into(),
            url: "/fhir".into(),
            body: None,
            headers: None,
            client_id: None,
            authorization: TrustedJobAuthorization::capture(None, true).unwrap(),
        };
        let running = manager.submit_job(request()).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), receive.recv())
                .await
                .unwrap()
                .unwrap(),
            running
        );
        let queued = manager.submit_job(request()).await.unwrap();
        drop(owner);
        tokio::time::timeout(Duration::from_secs(8), manager.shutdown())
            .await
            .unwrap();
        assert!(manager.submit_job(request()).await.is_err());
        assert_eq!(
            manager.get_job(running).await.unwrap().status,
            AsyncJobStatus::InProgress
        );
        assert_eq!(
            manager.get_job(queued).await.unwrap().status,
            AsyncJobStatus::Queued
        );
        assert!(
            storage
                .read("Patient", "shutdown-rollback")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            storage
                .history("Patient", Some("shutdown-rollback"), &Default::default())
                .await
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(receive_events.try_recv().is_err());
        let checkpoint: serde_json::Value = sqlx_core::query_scalar::query_scalar(
            "SELECT checkpoint FROM async_jobs WHERE id = $1",
        )
        .bind(running)
        .fetch_one(pool.as_ref())
        .await
        .unwrap();
        assert_eq!(checkpoint, serde_json::json!({}));
        let next = AsyncJobManager::new(pool, AsyncJobConfig::default());
        let recovered = next.claim_job().await.unwrap().unwrap();
        assert_eq!(recovered.claim.job_id, running);
        EXECUTION_CLAIM
            .scope(
                recovered.claim,
                next.complete_job(running, serde_json::json!({"recovered":true})),
            )
            .await
            .unwrap();
        assert_eq!(
            next.claim_job().await.unwrap().unwrap().claim.job_id,
            queued
        );
    }

    #[tokio::test]
    async fn import_limit_preserves_other_job_progress_and_releases_on_cancellation() {
        use testcontainers::{ImageExt, runners::AsyncRunner};
        let postgres = testcontainers_modules::postgres::Postgres::default()
            .with_tag("17-alpine")
            .start()
            .await
            .unwrap();
        let port = postgres.get_host_port_ipv4(5432).await.unwrap();
        let database = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
        let pool = Arc::new(
            sqlx_postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(&database)
                .await
                .unwrap(),
        );
        octofhir_db_postgres::migrations::run(&pool, &database)
            .await
            .unwrap();
        let manager = AsyncJobManager::new(
            pool,
            AsyncJobConfig {
                max_concurrent_jobs: 3,
                max_concurrent_imports: 1,
                ..Default::default()
            },
        );
        let (started, mut receive) = tokio::sync::mpsc::unbounded_channel();
        manager.set_executor(Arc::new(move |id, kind, _, _, _| {
            let started = started.clone();
            Box::pin(async move {
                if kind == "bulk_import" {
                    started.send(id).unwrap();
                    std::future::pending::<Result<serde_json::Value, String>>().await
                } else {
                    Ok(serde_json::json!({"completed":true}))
                }
            })
        }));
        let request = |kind: &str| AsyncJobRequest {
            request_type: kind.into(),
            method: "POST".into(),
            url: "/".into(),
            body: None,
            headers: None,
            client_id: None,
            authorization: TrustedJobAuthorization::capture(None, true).unwrap(),
        };
        let first = manager.submit_job(request("bulk_import")).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), receive.recv())
                .await
                .unwrap()
                .unwrap(),
            first
        );
        let second = manager.submit_job(request("bulk_import")).await.unwrap();
        let export = manager.submit_job(request("bulk_export")).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.get_job(export).await.unwrap().status != AsyncJobStatus::Completed {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("An import at capacity cannot starve a queued export");
        assert_eq!(
            manager.get_job(second).await.unwrap().status,
            AsyncJobStatus::Queued
        );
        assert!(receive.try_recv().is_err());
        manager.cancel_job(first).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), receive.recv())
                .await
                .unwrap()
                .unwrap(),
            second
        );
        manager.cancel_job(second).await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_survives_an_exhausted_primary_pool_and_fences_completion() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use testcontainers::{ImageExt, runners::AsyncRunner};
        let postgres = testcontainers_modules::postgres::Postgres::default()
            .with_tag("17-alpine")
            .start()
            .await
            .unwrap();
        let port = postgres.get_host_port_ipv4(5432).await.unwrap();
        let database = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
        let primary = Arc::new(
            sqlx_postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&database)
                .await
                .unwrap(),
        );
        octofhir_db_postgres::migrations::run(&primary, &database)
            .await
            .unwrap();
        let manager = AsyncJobManager::new(
            primary.clone(),
            AsyncJobConfig {
                max_concurrent_jobs: 1,
                ..Default::default()
            },
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let executor_pool = primary.clone();
        let executor_calls = calls.clone();
        let executor_started = started.clone();
        manager.set_executor(Arc::new(move |_, _, _, _, _| {
            let pool = executor_pool.clone();
            let calls = executor_calls.clone();
            let started = executor_started.clone();
            Box::pin(async move {
                let _transaction = pool.begin().await.map_err(|error| error.to_string())?;
                calls.fetch_add(1, Ordering::SeqCst);
                started.notify_one();
                std::future::pending::<Result<serde_json::Value, String>>().await
            })
        }));
        let request = || AsyncJobRequest {
            request_type: "cancellation-regression".into(),
            method: "POST".into(),
            url: "/".into(),
            body: None,
            headers: None,
            client_id: None,
            authorization: TrustedJobAuthorization::capture(None, true).unwrap(),
        };
        let running = manager.submit_job(request()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        let queued = manager.submit_job(request()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), manager.cancel_job(queued))
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), manager.cancel_job(running))
            .await
            .unwrap()
            .unwrap();
        let connection = tokio::time::timeout(Duration::from_secs(5), primary.acquire())
            .await
            .expect("Cancelled executor must release its held transaction")
            .unwrap();
        drop(connection);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "A cancelled queued job must never execute"
        );
        assert!(
            manager
                .complete_job(running, serde_json::json!({"leak":true}))
                .await
                .is_err()
        );
        assert!(
            manager
                .fail_job(running, "late failure".into())
                .await
                .is_err()
        );
        let job = manager.get_job(running).await.unwrap();
        assert_eq!(job.status, AsyncJobStatus::Cancelled);
        assert!(job.result.is_none());
        assert!(job.error_message.is_none());
        assert_eq!(
            manager.get_job(queued).await.unwrap().status,
            AsyncJobStatus::Cancelled
        );
        manager.db_pool.close().await;
        primary.close().await;
    }

    #[test]
    fn durable_job_owner_binds_principal_launch_and_delegated_grants() {
        let mut auth = octofhir_auth::middleware::AuthContext::system_anonymous();
        let claims = Arc::make_mut(&mut auth.token_claims);
        claims.scope = "patient/Observation.s?code=lab system/Patient.c".into();
        claims.patient = Some("p1".into());
        auth.patient = Some("p1".into());
        let owner = TrustedJobAuthorization::capture(Some(&auth), false).unwrap();
        let with_anonymous_access = TrustedJobAuthorization::capture(Some(&auth), true).unwrap();
        assert_eq!(with_anonymous_access.claims, owner.claims);
        assert!(!with_anonymous_access.permits_owner(None, true));
        let owner: TrustedJobAuthorization =
            serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap();
        assert!(owner.permits_owner(Some(&auth), false));
        assert!(!owner.permits_owner(None, false));
        for field in [
            "issuer",
            "subject",
            "client",
            "patient",
            "encounter",
            "grants",
        ] {
            let mut other = auth.clone();
            let claims = Arc::make_mut(&mut other.token_claims);
            match field {
                "issuer" => claims.iss = "another".into(),
                "subject" => claims.sub = "another".into(),
                "client" => claims.client_id = "another".into(),
                "patient" => claims.patient = Some("p2".into()),
                "encounter" => claims.encounter = Some("e1".into()),
                "grants" => claims.scope = "patient/Observation.s?code=vital".into(),
                _ => unreachable!(),
            }
            assert!(!owner.permits_owner(Some(&other), false), "{field}");
        }
        let anonymous = TrustedJobAuthorization::capture(None, true).unwrap();
        assert!(anonymous.permits_owner(None, true));
        assert!(!anonymous.permits_owner(Some(&auth), true));
        assert!(!anonymous.permits_owner(None, false));
    }

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
