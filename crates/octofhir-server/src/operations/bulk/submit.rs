//! `$bulk-submit` and `$bulk-submit-status` (Bulk Data Access IG, STU 4 ballot).
//!
//! A Data Provider names manifests by URL; this server (the Data Consumer) fetches them
//! and the files they list, validates each resource as a normal write would, and stores
//! it. Results are polled through the asynchronous pattern as OperationOutcome files.
//! Each resource a manifest writes is recorded, because `submissionStatus=stopped` and
//! `replacesManifestUrl` must take exactly that data back.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::Json;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream};
use octofhir_api::{ApiError, OperationOutcomeIssue};
use octofhir_auth::smart::scopes::SmartScopes;
use serde_json::{Value, json};
use sqlx_core::query::query;
use sqlx_core::row::Row;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use super::NDJSON_CONTENT_TYPE;
use super::import::{download_source, parse_source_url, read_source_line, source_client};
use super::manifest::ManifestFile;
use super::writer::NdjsonWriter;
use crate::async_jobs::{AsyncJob, AsyncJobRequest, AsyncJobStatus, CheckpointStep};
use crate::config::{BulkSpecVersion, BulkSubmitConfig, BulkSubmitter};
use crate::egress::EgressPolicy;
use crate::operations::OperationContext;
use crate::operations::handler::{OperationError, OperationHandler, OperationLevel};
use crate::server::AppState;

/// Fetches and stores one manifest.
pub(crate) const SUBMIT_JOB: &str = "bulk_submit";
/// A `$bulk-submit-status` request; its manifest is built from the tables on each poll.
pub(crate) const SUBMIT_STATUS_JOB: &str = "bulk_submit_status";
/// Takes back what a stopped submission wrote.
pub(crate) const SUBMIT_REVERT_JOB: &str = "bulk_submit_revert";

const STATUS_MANIFEST: &str =
    "http://hl7.org/fhir/uv/bulkdata/StructureDefinition/BulkSubmitStatusManifest";
const EVENT_STATUS: &str = "http://hl7.org/fhir/event-status";
const SOURCE_RESOURCE: &str =
    "http://hl7.org/fhir/StructureDefinition/operationoutcome-sourceResource";

/// File group of a job's OperationOutcomes.
const OUTCOME_FILE: &str = "outcome";

/// Headers a caller cannot choose: they belong to the connection, not the request.
const RESERVED_HEADERS: [&str; 5] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "upgrade",
];

fn invalid(message: impl Into<String>) -> OperationError {
    OperationError::InvalidParameters(message.into())
}

fn database(error: sqlx_core::Error) -> OperationError {
    tracing::error!(%error, "Bulk submission storage failed");
    OperationError::Internal("Bulk submission storage failed".into())
}

fn not_allowed() -> OperationError {
    ApiError::issues(
        StatusCode::METHOD_NOT_ALLOWED,
        vec![OperationOutcomeIssue::error(
            "not-supported",
            "This operation requires POST with a Parameters resource",
            None,
        )],
    )
    .into()
}

/// All entries of a Parameters resource with this name.
fn entries<'a>(params: &'a Value, name: &str) -> Vec<&'a Value> {
    params["parameter"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|parameter| parameter["name"] == name)
        .collect()
}

/// The value of a 0..1 parameter under one of the keys its type allows.
fn single<'a>(
    params: &'a Value,
    name: &str,
    keys: &[&str],
) -> Result<Option<&'a Value>, OperationError> {
    let found = entries(params, name);
    match found.as_slice() {
        [] => Ok(None),
        [parameter] => keys
            .iter()
            .find_map(|key| parameter.get(*key))
            .map(Some)
            .ok_or_else(|| invalid(format!("{name} requires a value of type {}", keys[0]))),
        _ => Err(invalid(format!("{name} must occur once"))),
    }
}

fn text<'a>(
    params: &'a Value,
    name: &str,
    keys: &[&str],
) -> Result<Option<&'a str>, OperationError> {
    single(params, name, keys)?
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid(format!("{name} requires a value")))
        })
        .transpose()
}

fn part<'a>(parameter: &'a Value, name: &str, key: &str) -> Option<&'a str> {
    parameter["part"]
        .as_array()?
        .iter()
        .find(|part| part["name"] == name)?
        .get(key)?
        .as_str()
}

/// Who a submission belongs to, shared by both operations.
struct Submission {
    system: String,
    value: String,
    id: String,
}

impl Submission {
    fn parse(params: &Value) -> Result<Self, OperationError> {
        if params["resourceType"] != "Parameters" {
            return Err(invalid("The request body must be a Parameters resource"));
        }
        let submitter = single(params, "submitter", &["valueIdentifier"])?
            .ok_or_else(|| invalid("submitter is required"))?;
        let identifier = |field: &str| {
            submitter[field]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("submitter requires {field}")))
        };
        Ok(Self {
            system: identifier("system")?,
            value: identifier("value")?,
            id: text(params, "submissionId", &["valueString"])?
                .ok_or_else(|| invalid("submissionId is required"))?
                .to_owned(),
        })
    }

    /// The IG has the Data Consumer agree on submitters out of band; a client tied to one
    /// cannot speak for another.
    fn authorize<'c>(
        &self,
        config: &'c BulkSubmitConfig,
        context: &OperationContext,
    ) -> Result<&'c BulkSubmitter, OperationError> {
        let listed = config
            .submitters
            .iter()
            .find(|entry| entry.system == self.system && entry.value == self.value)
            .ok_or_else(|| ApiError::forbidden("This submitter is not registered"))?;
        if let Some(client_id) = &listed.client_id
            && context.auth().map(|auth| auth.client_id()) != Some(client_id.as_str())
        {
            return Err(ApiError::forbidden("This client cannot act as the submitter").into());
        }
        Ok(listed)
    }
}

/// Whether writes run under `system/bulk-submit` rather than per-resource grants.
/// The scope names no resource types; types are only known once a manifest is read.
fn holds_submit_scope(context: &OperationContext) -> bool {
    context.auth().is_some_and(|auth| {
        SmartScopes::parse(&auth.token_claims.scope).is_ok_and(|scopes| scopes.bulk_submit)
    })
}

/// Tokens without `system/bulk-submit` must at least be able to write something;
/// each resource is then checked against their grants.
fn authorize_caller(state: &AppState, context: &OperationContext) -> Result<bool, OperationError> {
    let Some(auth) = context.auth() else {
        return if state.config.auth.policy.anonymous_access {
            Ok(false)
        } else {
            Err(ApiError::unauthorized("Authentication required").into())
        };
    };
    if holds_submit_scope(context) {
        return Ok(true);
    }
    let writes = SmartScopes::parse(&auth.token_claims.scope).is_ok_and(|scopes| {
        scopes
            .resource_scopes
            .iter()
            .any(|scope| scope.permissions.can_write())
    });
    if writes {
        Ok(false)
    } else {
        Err(ApiError::forbidden("Bulk submit requires the system/bulk-submit scope").into())
    }
}

/// Parameters of one `$bulk-submit` request beside the submission identity.
#[derive(Debug, PartialEq)]
struct SubmitRequest {
    status: Option<String>,
    manifest_url: Option<String>,
    replaces: Option<String>,
    fhir_base_url: Option<String>,
    headers: Vec<(String, String)>,
    /// Where to discover the token endpoint for the manifest and its files.
    oauth_metadata: Vec<String>,
    /// `metadata` and `import` entries: defined by use-case IGs, none known here.
    unknown_options: usize,
}

impl SubmitRequest {
    fn parse(params: &Value) -> Result<Self, OperationError> {
        let status = match single(params, "submissionStatus", &["valueCoding"])? {
            None => None,
            Some(coding) => {
                if coding.get("system").and_then(Value::as_str) != Some(EVENT_STATUS) {
                    return Err(invalid(format!(
                        "submissionStatus requires the system {EVENT_STATUS}"
                    )));
                }
                match coding["code"].as_str() {
                    Some(code @ ("in-progress" | "completed" | "stopped")) => Some(code.to_owned()),
                    _ => return Err(invalid("Unsupported submissionStatus code")),
                }
            }
        };
        let url = |name: &str| -> Result<Option<String>, OperationError> {
            text(params, name, &["valueUrl", "valueUri"])?
                .map(|value| {
                    parse_source_url(value).map_err(invalid)?;
                    Ok(value.to_owned())
                })
                .transpose()
        };
        let manifest_url = url("manifestUrl")?;
        let replaces =
            text(params, "replacesManifestUrl", &["valueUrl", "valueUri"])?.map(str::to_owned);
        let fhir_base_url = url("fhirBaseUrl")?;
        if manifest_url.is_none() && status.is_none() {
            return Err(invalid(
                "At least one of submissionStatus and manifestUrl is required",
            ));
        }
        if manifest_url.is_some() && fhir_base_url.is_none() {
            return Err(invalid("fhirBaseUrl is required with manifestUrl"));
        }
        if let Some(format) = text(params, "outputFormat", &["valueString", "valueCode"])? {
            let media_type = format.split(';').next().unwrap_or_default().trim();
            if !matches!(
                media_type,
                NDJSON_CONTENT_TYPE | "application/ndjson" | "ndjson"
            ) {
                return Err(invalid(format!("Unsupported outputFormat: {format}")));
            }
        }
        // Refused rather than ignored: the IG makes it binding on a Data Consumer that
        // accepts it, and without it the files cannot be read as intended.
        if !entries(params, "fileEncryptionKey").is_empty() {
            return Err(OperationError::NotSupported(
                "fileEncryptionKey is not supported".into(),
            ));
        }
        let mut oauth_metadata = Vec::new();
        for parameter in entries(params, "oauthMetadataUrl") {
            let url = parameter
                .get("valueUrl")
                .or_else(|| parameter.get("valueUri"))
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("oauthMetadataUrl requires a url"))?;
            parse_source_url(url).map_err(invalid)?;
            oauth_metadata.push(url.to_owned());
        }
        let mut headers = Vec::new();
        for parameter in entries(params, "fileRequestHeader") {
            let name = part(parameter, "headerName", "valueString");
            let value = part(parameter, "headerValue", "valueString");
            let (Some(name), Some(value)) = (name, value) else {
                return Err(invalid(
                    "fileRequestHeader requires headerName and headerValue",
                ));
            };
            if RESERVED_HEADERS.contains(&name.to_ascii_lowercase().as_str())
                || HeaderName::try_from(name).is_err()
                || HeaderValue::try_from(value).is_err()
            {
                return Err(invalid(format!("Invalid fileRequestHeader {name}")));
            }
            headers.push((name.to_owned(), value.to_owned()));
        }
        Ok(Self {
            status,
            manifest_url,
            replaces,
            fhir_base_url,
            headers,
            oauth_metadata,
            unknown_options: entries(params, "metadata").len() + entries(params, "import").len(),
        })
    }
}

/// Finds or opens the submission and enforces that it stays with its client.
async fn open_submission(
    state: &AppState,
    context: &OperationContext,
    submission: &Submission,
    create: bool,
) -> Result<Option<(Uuid, String)>, OperationError> {
    let pool = state.db_pool.as_ref();
    let client = context.auth().map(|auth| auth.client_id().to_owned());
    if create {
        query(
            "INSERT INTO bulk_submissions (submitter_system, submitter_value, submission_id, client_id) \
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(&submission.system)
        .bind(&submission.value)
        .bind(&submission.id)
        .bind(&client)
        .execute(pool)
        .await
        .map_err(database)?;
    }
    let Some(row) = query(
        "SELECT id, status, client_id FROM bulk_submissions \
         WHERE submitter_system = $1 AND submitter_value = $2 AND submission_id = $3",
    )
    .bind(&submission.system)
    .bind(&submission.value)
    .bind(&submission.id)
    .fetch_optional(pool)
    .await
    .map_err(database)?
    else {
        return Ok(None);
    };
    let owner: Option<String> = row.try_get("client_id").map_err(database)?;
    if owner != client {
        return Err(ApiError::forbidden("This submission belongs to another client").into());
    }
    Ok(Some((
        row.try_get("id").map_err(database)?,
        row.try_get("status").map_err(database)?,
    )))
}

/// `$bulk-submit`
pub struct BulkSubmitOperation {
    config: BulkSubmitConfig,
}

impl BulkSubmitOperation {
    pub fn new(config: BulkSubmitConfig) -> Self {
        Self { config }
    }
}

/// A manifest of this submission by URL: `(id, job, status)`.
async fn find_manifest(
    state: &AppState,
    submission: Uuid,
    url: &str,
) -> Result<Option<(Uuid, Option<Uuid>, String)>, OperationError> {
    query(
        "SELECT id, job_id, status FROM bulk_submission_manifests \
         WHERE submission = $1 AND manifest_url = $2",
    )
    .bind(submission)
    .bind(url)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(database)?
    .map(|row| {
        Ok((
            row.try_get("id").map_err(database)?,
            row.try_get("job_id").map_err(database)?,
            row.try_get("status").map_err(database)?,
        ))
    })
    .transpose()
}

/// Stops a job so it can write nothing further; a job already over needs nothing.
async fn stop_job(state: &AppState, job: Option<Uuid>) {
    if let Some(job) = job
        && let Ok(found) = state.async_job_manager.get_job(job).await
        && matches!(
            found.status,
            AsyncJobStatus::Queued | AsyncJobStatus::InProgress
        )
        && let Err(error) = state.async_job_manager.cancel_job(job).await
    {
        tracing::warn!(%job, %error, "Cannot stop a bulk submission job");
    }
}

#[async_trait]
impl OperationHandler for BulkSubmitOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        self.config.enabled && level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "bulk-submit"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if context.is_get() {
            return Err(not_allowed());
        }
        let trusted = authorize_caller(state, context)?;
        let submission = Submission::parse(params)?;
        let submitter = submission.authorize(&self.config, context)?;
        let request = SubmitRequest::parse(params)?;
        if !request.oauth_metadata.is_empty() && submitter.provider_client_id.is_none() {
            return Err(OperationError::NotSupported(
                "oauthMetadataUrl needs bulk_submit.submitters.provider_client_id; \
                 use fileRequestHeader"
                    .into(),
            ));
        }
        let (id, status) = open_submission(state, context, &submission, true)
            .await?
            .ok_or_else(|| OperationError::Internal("Cannot open the submission".into()))?;
        if status != "in-progress" {
            return Err(ApiError::conflict(format!(
                "Submission {} is already {status}",
                submission.id
            ))
            .into());
        }

        let replaced = match &request.replaces {
            None => None,
            Some(url) => {
                let (manifest, job, _) = find_manifest(state, id, url)
                    .await?
                    .filter(|(_, _, status)| status != "replaced")
                    .ok_or_else(|| invalid(format!("Unknown replacesManifestUrl: {url}")))?;
                stop_job(state, job).await;
                Some(manifest)
            }
        };

        if let Some(url) = &request.manifest_url {
            if find_manifest(state, id, url).await?.is_some() {
                return Err(invalid(format!("manifestUrl was already submitted: {url}")));
            }
            let manifest: Uuid = query(
                "INSERT INTO bulk_submission_manifests (submission, manifest_url) \
                 VALUES ($1, $2) RETURNING id",
            )
            .bind(id)
            .bind(url)
            .fetch_one(state.db_pool.as_ref())
            .await
            .map_err(database)?
            .try_get("id")
            .map_err(database)?;
            let body = json!({
                "manifest": manifest,
                "manifest_url": url,
                "replaces": replaced,
                "fhir_base_url": request.fhir_base_url,
                "file_headers": request.headers,
                "oauth_metadata": request.oauth_metadata,
                "provider_client_id": submitter.provider_client_id,
                "provider_scope": submitter.provider_scope,
                "trusted": trusted,
                "unknown_options": request.unknown_options,
            });
            let job = submit_job(state, context, SUBMIT_JOB, body).await?;
            query("UPDATE bulk_submission_manifests SET job_id = $2 WHERE id = $1")
                .bind(manifest)
                .bind(job)
                .execute(state.db_pool.as_ref())
                .await
                .map_err(database)?;
        } else if let Some(manifest) = replaced {
            // Discarding a manifest without a successor is a job of its own.
            let body =
                json!({"manifests": [manifest], "trusted": trusted, "final_status": "replaced"});
            submit_job(state, context, SUBMIT_REVERT_JOB, body).await?;
        }

        match request.status.as_deref() {
            Some("completed") => set_status(state, id, "completed", None).await?,
            Some("stopped") => stop_submission(state, context, id, trusted).await?,
            _ => {}
        }
        Ok(json!({"resourceType": "OperationOutcome", "issue": [{
            "severity": "information",
            "code": "informational",
            "diagnostics": "Submission request accepted",
        }]}))
    }
}

async fn set_status(
    state: &AppState,
    submission: Uuid,
    status: &str,
    revert_job: Option<Uuid>,
) -> Result<(), OperationError> {
    query(
        "UPDATE bulk_submissions SET status = $2, revert_job_id = $3, updated_at = NOW() \
         WHERE id = $1",
    )
    .bind(submission)
    .bind(status)
    .bind(revert_job)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(database)?;
    Ok(())
}

/// `stopped`: the Data Consumer SHALL stop retrieving files and delete what it processed.
async fn stop_submission(
    state: &AppState,
    context: &OperationContext,
    submission: Uuid,
    trusted: bool,
) -> Result<(), OperationError> {
    let rows = query(
        "SELECT id, job_id FROM bulk_submission_manifests \
         WHERE submission = $1 AND status <> 'replaced'",
    )
    .bind(submission)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(database)?;
    let mut manifests = Vec::with_capacity(rows.len());
    for row in rows {
        stop_job(state, row.try_get("job_id").map_err(database)?).await;
        manifests.push(row.try_get::<Uuid, _>("id").map_err(database)?);
    }
    let body = json!({"manifests": manifests, "trusted": trusted, "final_status": "stopped"});
    let job = submit_job(state, context, SUBMIT_REVERT_JOB, body).await?;
    set_status(state, submission, "stopped", Some(job)).await
}

async fn submit_job(
    state: &AppState,
    context: &OperationContext,
    request_type: &str,
    body: Value,
) -> Result<Uuid, OperationError> {
    state
        .async_job_manager
        .submit_job(AsyncJobRequest {
            request_type: request_type.to_owned(),
            method: "POST".to_owned(),
            url: context.request_url().to_owned(),
            body: Some(body),
            headers: None,
            client_id: context.auth().map(|auth| auth.client_id().to_owned()),
            authorization: context.job_authorization(state)?,
        })
        .await
        .map_err(|error| OperationError::Internal(format!("Cannot submit the job: {error}")))
}

/// `$bulk-submit-status`
pub struct BulkSubmitStatusOperation {
    config: BulkSubmitConfig,
}

impl BulkSubmitStatusOperation {
    pub fn new(config: BulkSubmitConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl OperationHandler for BulkSubmitStatusOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        self.config.enabled && level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "bulk-submit-status"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if context.is_get() {
            return Err(not_allowed());
        }
        authorize_caller(state, context)?;
        let submission = Submission::parse(params)?;
        submission.authorize(&self.config, context)?;
        if let Some(format) = text(params, "_outputFormat", &["valueString", "valueCode"])?
            && !matches!(
                format,
                NDJSON_CONTENT_TYPE | "application/ndjson" | "ndjson"
            )
        {
            return Err(invalid(format!("Unsupported _outputFormat: {format}")));
        }
        let (id, _) = open_submission(state, context, &submission, false)
            .await?
            .ok_or_else(|| {
                OperationError::NotFound(format!("Submission {} not found", submission.id))
            })?;
        let body = json!({
            "submission": id,
            "submission_id": submission.id,
            "separate_status": context.separate_status(),
        });
        let job = submit_job(state, context, SUBMIT_STATUS_JOB, body).await?;
        Ok(json!({
            "status": "accepted",
            "job_id": job.to_string(),
            "status_url": format!("{}/_async-status/{job}", state.fhir_base_url()),
            "separate_status": context.separate_status(),
        }))
    }
}

/// Status of a job in a bulk response. With `Prefer: separate-export-status` the HTTP
/// status is about the poll, and `X-Export-Status` is about the job.
pub(crate) fn job_response(
    status: StatusCode,
    mut headers: HeaderMap,
    body: Value,
    separate: bool,
) -> Response {
    let status = if separate {
        if let Ok(value) = HeaderValue::from_str(&status.to_string()) {
            headers.insert("X-Export-Status", value);
        }
        StatusCode::OK
    } else {
        status
    };
    (status, headers, Json(body)).into_response()
}

/// The `$bulk-submit-status` poll: 202 with what is known so far until the submission
/// is closed and every manifest is processed, then 200.
pub(crate) async fn status_response(
    state: &AppState,
    job: &AsyncJob,
    requires_token: bool,
) -> Result<Response, ApiError> {
    let body = job.request_body.as_ref();
    let submission = body
        .and_then(|body| body["submission"].as_str())
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| ApiError::internal("Invalid status job"))?;
    let separate = body.is_some_and(|body| body["separate_status"] == true);
    let failed = |error: sqlx_core::Error| {
        tracing::error!(%error, "Bulk submission status failed");
        ApiError::internal("Cannot read the submission")
    };
    let row = query(
        "SELECT s.status, s.submission_id, s.updated_at, r.status AS revert_status \
         FROM bulk_submissions s LEFT JOIN async_jobs r ON r.id = s.revert_job_id WHERE s.id = $1",
    )
    .bind(submission)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(failed)?
    .ok_or_else(|| ApiError::not_found("Submission not found"))?;
    let status: String = row.try_get("status").map_err(failed)?;
    let revert: Option<String> = row.try_get("revert_status").map_err(failed)?;
    let submission_id: String = row.try_get("submission_id").map_err(failed)?;
    let mut updated: DateTime<Utc> = row.try_get("updated_at").map_err(failed)?;
    let manifests = query(
        "SELECT m.manifest_url, m.status, m.outcome, m.updated_at, j.status AS job_status \
         FROM bulk_submission_manifests m LEFT JOIN async_jobs j ON j.id = m.job_id \
         WHERE m.submission = $1 ORDER BY m.created_at",
    )
    .bind(submission)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(failed)?;

    let mut outcome = Vec::new();
    let mut pending = 0usize;
    for manifest in &manifests {
        let url: String = manifest.try_get("manifest_url").map_err(failed)?;
        let state_name: String = manifest.try_get("status").map_err(failed)?;
        let job_state: Option<String> = manifest.try_get("job_status").map_err(failed)?;
        let files: Value = manifest.try_get("outcome").map_err(failed)?;
        updated = updated.max(manifest.try_get("updated_at").map_err(failed)?);
        // A job can end without reporting back, for example when it is cancelled.
        let running = matches!(state_name.as_str(), "queued" | "processing")
            && matches!(job_state.as_deref(), None | Some("queued" | "in_progress"));
        if running {
            pending += 1;
        }
        for mut file in files.as_array().cloned().unwrap_or_default() {
            file["manifestUrl"] = json!(url);
            outcome.push(file);
        }
    }
    let reverting = matches!(revert.as_deref(), Some("queued" | "in_progress"));
    let complete = status != "in-progress" && pending == 0 && !reverting;

    let manifest = json!({
        "manifestType": STATUS_MANIFEST,
        "submissionId": submission_id,
        "transactionTime": updated.to_rfc3339(),
        "requiresAccessToken": requires_token,
        "outputFormat": NDJSON_CONTENT_TYPE,
        "output": [],
        "outcome": outcome,
    });
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if complete {
        if let Ok(value) = HeaderValue::from_str(
            &job.expires_at
                .format("%a, %d %b %Y %H:%M:%S GMT")
                .to_string(),
        ) {
            headers.insert(header::EXPIRES, value);
        }
        return Ok(job_response(StatusCode::OK, headers, manifest, separate));
    }
    headers.insert(header::RETRY_AFTER, HeaderValue::from_static("10"));
    let done = manifests.len() - pending;
    if let Ok(value) = HeaderValue::from_str(&format!("{done} of {} manifests", manifests.len())) {
        headers.insert("X-Progress", value);
    }
    Ok(job_response(
        StatusCode::ACCEPTED,
        headers,
        manifest,
        separate,
    ))
}

/// Whether a file URL is an outcome file some manifest of a submission reports.
pub(crate) async fn lists_outcome(state: &AppState, url: &str) -> bool {
    query(
        "SELECT 1 FROM bulk_submission_manifests \
         WHERE outcome @> jsonb_build_array(jsonb_build_object('url', $1::text))",
    )
    .bind(url)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .is_ok_and(|row| row.is_some())
}

/// One submitted file.
struct FileSource {
    url: String,
    /// Absent in files organized by a resource type, which hold many types.
    resource_type: Option<String>,
}

/// A line that failed, and why.
#[derive(Debug)]
struct RejectedLine {
    /// `Type/id` when the line got that far.
    resource: Option<String>,
    issues: Vec<Value>,
}

impl RejectedLine {
    fn message(code: &'static str, source: &str, line: usize, text: String) -> Self {
        Self {
            resource: None,
            issues: vec![json!({
                "severity": "error",
                "code": code,
                "diagnostics": format!("{source}, line {line}: {text}"),
            })],
        }
    }

    fn outcome(self, base: &str) -> Value {
        let mut outcome = json!({"resourceType": "OperationOutcome", "issue": self.issues});
        if let Some(resource) = self.resource {
            outcome["extension"] = json!([{
                "url": SOURCE_RESOURCE,
                "valueReference": {"reference": format!("{}/{resource}", base.trim_end_matches('/'))},
            }]);
        }
        outcome
    }
}

/// What a job has to tell about a manifest, ending up in its outcome file.
#[derive(Default)]
struct OutcomeLog {
    lines: Vec<Value>,
    written: usize,
    rejected: usize,
}

impl OutcomeLog {
    fn note(&mut self, severity: &str, code: &str, diagnostics: String) {
        self.lines
            .push(json!({"resourceType": "OperationOutcome", "issue": [{
                "severity": severity, "code": code, "diagnostics": diagnostics,
            }]}));
    }
}

/// Lets a relative reference and one under the sender's base mean the same local resource.
fn localize_references(value: &mut Value, base: &str) {
    match value {
        Value::Object(fields) => {
            for (name, field) in fields.iter_mut() {
                if name == "reference" {
                    if let Value::String(reference) = field
                        && let Some(local) = reference.strip_prefix(base)
                    {
                        *reference = local.trim_start_matches('/').to_owned();
                    }
                } else {
                    localize_references(field, base);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                localize_references(item, base);
            }
        }
        _ => {}
    }
}

/// A `outputOrganizedBy` block header: `Parameters` with a `header` reference.
fn is_block_header(resource: &Value) -> bool {
    resource["resourceType"] == "Parameters" && resource["parameter"][0]["name"] == "header"
}

/// Everything a job needs to run one manifest.
struct ManifestRun<'a> {
    state: &'a AppState,
    manifest: Uuid,
    base: String,
    trusted: bool,
    known_refs: Arc<HashSet<String>>,
}

/// A line ready to be validated and stored.
#[derive(Debug)]
struct ParsedLine {
    number: usize,
    resource: Value,
}

fn parse_line(
    source: &str,
    number: usize,
    bytes: &[u8],
    expected: Option<&str>,
    base: &str,
) -> Result<Option<ParsedLine>, RejectedLine> {
    let mut resource: Value = serde_json::from_slice(bytes).map_err(|error| {
        RejectedLine::message(
            "structure",
            source,
            number,
            format!("Invalid JSON: {error}"),
        )
    })?;
    if is_block_header(&resource) {
        return Ok(None);
    }
    let resource_type = resource["resourceType"].as_str().ok_or_else(|| {
        RejectedLine::message("structure", source, number, "No resourceType".into())
    })?;
    if expected.is_some_and(|expected| expected != resource_type) {
        return Err(RejectedLine::message(
            "structure",
            source,
            number,
            format!(
                "Expected {}, found {resource_type}",
                expected.unwrap_or_default()
            ),
        ));
    }
    match resource.get("id") {
        None => resource["id"] = json!(octofhir_core::generate_id()),
        Some(id)
            if id
                .as_str()
                .is_none_or(|id| octofhir_core::validate_id(id).is_err()) =>
        {
            return Err(RejectedLine::message(
                "value",
                source,
                number,
                "Invalid resource id".into(),
            ));
        }
        Some(_) => {}
    }
    localize_references(&mut resource, base);
    Ok(Some(ParsedLine { number, resource }))
}

fn target(resource: &Value) -> String {
    format!(
        "{}/{}",
        resource["resourceType"].as_str().unwrap_or_default(),
        resource["id"].as_str().unwrap_or_default()
    )
}

impl ManifestRun<'_> {
    /// Validate one batch line by line, so a bad resource costs only itself, then
    /// write the rest as one chunk.
    async fn store_batch(
        &self,
        file_index: usize,
        source: &FileSource,
        lines: Vec<(usize, Vec<u8>)>,
    ) -> Result<(usize, Vec<Value>), String> {
        let state = self.state;
        let first = lines.first().map_or(0, |(line, _)| *line);
        let fingerprint: Vec<_> = lines
            .iter()
            .map(|(line, bytes)| (line, String::from_utf8_lossy(bytes)))
            .collect();
        let mut step = CheckpointStep::new(
            format!("submit-{file_index}-{first}"),
            &json!({"source": source.url, "lines": fingerprint}),
        )
        .map_err(|error| error.to_string())?;
        step.count_only = true;
        if let Some(record) = state
            .async_job_manager
            .restore_checkpoint(&step)
            .await
            .map_err(|error| error.to_string())?
        {
            return Ok((
                record["result"]["created"].as_u64().unwrap_or(0) as usize,
                record["metadata"]["errors"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            ));
        }

        let mut rejected = Vec::new();
        let mut parsed = Vec::with_capacity(lines.len());
        for (number, bytes) in &lines {
            match parse_line(
                &source.url,
                *number,
                bytes,
                source.resource_type.as_deref(),
                &self.base,
            ) {
                Ok(Some(line)) => parsed.push(line),
                Ok(None) => {}
                Err(line) => rejected.push(line.outcome(&self.base)),
            }
        }

        let parallelism = state.config.bulk_import.max_parallel_resources.max(1);
        let known = self.known_refs.clone();
        let mut checked = stream::iter(parsed.into_iter().map(|line| {
            let known = known.clone();
            async move {
                let outcome = state
                    .validation_service
                    .validate_with_known_refs(&line.resource, &known)
                    .await;
                (line, outcome)
            }
        }))
        .buffer_unordered(parallelism);
        let mut valid = Vec::new();
        while let Some((line, outcome)) = checked.next().await {
            if outcome.valid {
                valid.push(line);
            } else {
                let issues = outcome.to_operation_outcome()["issue"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                rejected.push(
                    RejectedLine {
                        resource: Some(target(&line.resource)),
                        issues,
                    }
                    .outcome(&self.base),
                );
            }
        }
        drop(checked);
        valid.sort_by_key(|line| line.number);

        // A resource occurring twice in one chunk would be two writes of one row.
        let mut last: HashMap<String, usize> = HashMap::new();
        for (index, line) in valid.iter().enumerate() {
            last.insert(target(&line.resource), index);
        }
        let mut index = 0usize;
        valid.retain(|line| {
            let keep = last.get(&target(&line.resource)) == Some(&index);
            index += 1;
            keep
        });

        step.metadata = json!({"errors": rejected});
        if valid.is_empty() {
            state
                .async_job_manager
                .save_read_checkpoint(
                    &step,
                    &json!({"resourceType": "Bundle", "type": "transaction-response", "entry": []}),
                    state.config.search.include_limits.max_bytes,
                )
                .await
                .map_err(|error| error.to_string())?;
            return Ok((0, rejected));
        }

        self.record_intent(&valid).await?;
        let entries: Vec<Value> = valid
            .iter()
            .map(|line| {
                json!({"resource": line.resource, "request": {
                    "method": "PUT",
                    "url": target(&line.resource),
                }})
            })
            .collect();
        match write_chunk(state, entries, self.trusted, step).await {
            Ok(_) => {
                self.record_versions(&valid).await?;
                Ok((valid.len(), rejected))
            }
            Err(error) => {
                // The chunk rolled back as a group, so every line of it is reported.
                let issues = error.to_operation_outcome();
                let issues =
                    serde_json::to_value(issues).map_err(|error| error.to_string())?["issue"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                for line in &valid {
                    rejected.push(
                        RejectedLine {
                            resource: Some(target(&line.resource)),
                            issues: issues.clone(),
                        }
                        .outcome(&self.base),
                    );
                }
                Ok((0, rejected))
            }
        }
    }

    /// Note each target and the version it has now, before the write commits. A replayed
    /// chunk keeps the first note: by then the row already holds the submitted version.
    async fn record_intent(&self, lines: &[ParsedLine]) -> Result<(), String> {
        let mut types = Vec::with_capacity(lines.len());
        let mut ids = Vec::with_capacity(lines.len());
        let mut priors = Vec::with_capacity(lines.len());
        for (resource_type, group) in by_type(lines) {
            let current = versions(self.state, &resource_type, &group).await?;
            for id in group {
                priors.push(current.get(&id).cloned());
                types.push(resource_type.clone());
                ids.push(id);
            }
        }
        query(
            "INSERT INTO bulk_submission_writes (manifest, resource_type, resource_id, prior_version) \
             SELECT $1, * FROM UNNEST($2::text[], $3::text[], $4::text[]) \
             ON CONFLICT (manifest, resource_type, resource_id) DO NOTHING",
        )
        .bind(self.manifest)
        .bind(types)
        .bind(ids)
        .bind(priors)
        .execute(self.state.db_pool.as_ref())
        .await
        .map_err(|error| format!("Cannot record submission writes: {error}"))?;
        Ok(())
    }

    async fn record_versions(&self, lines: &[ParsedLine]) -> Result<(), String> {
        for (resource_type, group) in by_type(lines) {
            let current = versions(self.state, &resource_type, &group).await?;
            let (ids, written): (Vec<String>, Vec<String>) = current.into_iter().unzip();
            query(
                "UPDATE bulk_submission_writes w SET written_version = v.version \
                 FROM UNNEST($3::text[], $4::text[]) AS v(id, version) \
                 WHERE w.manifest = $1 AND w.resource_type = $2 AND w.resource_id = v.id",
            )
            .bind(self.manifest)
            .bind(&resource_type)
            .bind(ids)
            .bind(written)
            .execute(self.state.db_pool.as_ref())
            .await
            .map_err(|error| format!("Cannot record submission writes: {error}"))?;
        }
        Ok(())
    }
}

fn by_type(lines: &[ParsedLine]) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for line in lines {
        groups
            .entry(
                line.resource["resourceType"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
            .or_default()
            .push(line.resource["id"].as_str().unwrap_or_default().to_owned());
    }
    groups
}

/// Current version of each live resource among `ids`.
async fn versions(
    state: &AppState,
    resource_type: &str,
    ids: &[String],
) -> Result<HashMap<String, String>, String> {
    Ok(state
        .storage
        .read_many(resource_type, ids)
        .await
        .map_err(|error| format!("Cannot read {resource_type}: {error}"))?
        .into_iter()
        .map(|resource| (resource.id, resource.version_id))
        .collect())
}

async fn write_chunk(
    state: &AppState,
    entries: Vec<Value>,
    trusted: bool,
    step: CheckpointStep,
) -> Result<Value, ApiError> {
    if trusted {
        crate::handlers::execute_trusted_write_chunk(state, entries, step).await
    } else {
        // Each resource was validated above, against the whole manifest.
        crate::handlers::execute_worker_write_chunk(state, entries, true, step).await
    }
}

/// Claims of a SMART Backend Services client assertion.
#[derive(serde::Serialize)]
struct ClientAssertion<'a> {
    iss: &'a str,
    sub: &'a str,
    aud: &'a str,
    exp: i64,
    jti: String,
}

/// This server as a SMART Backend Services client of a Data Provider.
struct ProviderAuth<'a> {
    state: &'a AppState,
    client: &'a reqwest::Client,
    token_endpoint: String,
    client_id: String,
    scope: String,
    /// A token and when to stop using it.
    current: Option<(String, Instant)>,
}

impl<'a> ProviderAuth<'a> {
    /// Finds the token endpoint in the first metadata document that has one.
    async fn discover(
        state: &'a AppState,
        client: &'a reqwest::Client,
        egress: &EgressPolicy,
        params: &Value,
    ) -> Result<Option<Self>, String> {
        let urls: Vec<String> =
            serde_json::from_value(params["oauth_metadata"].clone()).unwrap_or_default();
        if urls.is_empty() {
            return Ok(None);
        }
        let client_id = params["provider_client_id"]
            .as_str()
            .ok_or("No client id for the provider's authorization server")?;
        for url in &urls {
            let document = fetch_json(client, url, state, egress, &[]).await?;
            if let Some(token_endpoint) = document["token_endpoint"].as_str() {
                egress.check_url(&parse_source_url(token_endpoint)?)?;
                return Ok(Some(Self {
                    state,
                    client,
                    token_endpoint: token_endpoint.to_owned(),
                    client_id: client_id.to_owned(),
                    scope: params["provider_scope"]
                        .as_str()
                        .unwrap_or("system/*.read")
                        .to_owned(),
                    current: None,
                }));
            }
        }
        Err("No oauthMetadataUrl document names a token_endpoint".into())
    }

    /// `Authorization` for the next request, renewing a token close to its end.
    async fn header(&mut self) -> Result<(String, String), String> {
        if let Some((token, until)) = &self.current
            && Instant::now() < *until
        {
            return Ok(("Authorization".to_owned(), format!("Bearer {token}")));
        }
        let assertion = self
            .state
            .auth_state
            .jwt_service
            .encode(&ClientAssertion {
                iss: &self.client_id,
                sub: &self.client_id,
                aud: &self.token_endpoint,
                exp: Utc::now().timestamp() + 300,
                jti: Uuid::new_v4().to_string(),
            })
            .map_err(|_| "Cannot sign the client assertion")?;
        let response = self
            .client
            .post(&self.token_endpoint)
            .form(&[
                ("grant_type", "client_credentials"),
                ("scope", self.scope.as_str()),
                (
                    "client_assertion_type",
                    "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
                ),
                ("client_assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|error| {
                format!(
                    "Token request failed: {}",
                    crate::egress::describe_error(&error.without_url())
                )
            })?;
        if !response.status().is_success() {
            return Err(format!("Token endpoint returned HTTP {}", response.status()));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| "The token response is not JSON")?;
        let token = body["access_token"]
            .as_str()
            .ok_or("The token response has no access_token")?
            .to_owned();
        let lifetime = body["expires_in"].as_u64().unwrap_or(300);
        let until = Instant::now() + Duration::from_secs(lifetime.saturating_sub(30));
        let header = format!("Bearer {token}");
        self.current = Some((token, until));
        Ok(("Authorization".to_owned(), header))
    }
}

async fn fetch_json(
    client: &reqwest::Client,
    url: &str,
    state: &AppState,
    egress: &EgressPolicy,
    headers: &[(String, String)],
) -> Result<Value, String> {
    let limit = state.config.bulk_import.max_source_bytes;
    let (file, _) = download_source(client, url, limit, egress, headers).await?;
    let mut bytes = Vec::new();
    tokio::io::BufReader::new(file)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "Cannot read the document")?;
    serde_json::from_slice(&bytes).map_err(|_| format!("{url} is not valid JSON"))
}

/// Request headers with the provider's token, when there is one to use.
async fn with_token(
    headers: &[(String, String)],
    auth: &mut Option<ProviderAuth<'_>>,
) -> Result<Vec<(String, String)>, String> {
    let mut all = headers.to_vec();
    if let Some(auth) = auth {
        all.push(auth.header().await?);
    }
    Ok(all)
}

/// What the manifests reachable through `link` hold.
#[derive(Default)]
struct ManifestFiles {
    files: Vec<(String, Option<String>)>,
    /// Some manifest set `requiresAccessToken`, so the files take a token too.
    requires_token: bool,
}

/// The manifests reachable from `url` through `link`, as their file lists.
async fn read_manifests(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    auth: &mut Option<ProviderAuth<'_>>,
    state: &AppState,
    egress: &EgressPolicy,
    log: &mut OutcomeLog,
) -> Result<ManifestFiles, String> {
    let limits = &state.config.bulk_import;
    let mut found = ManifestFiles::default();
    let mut visited = HashSet::new();
    let mut next = Some(url.to_owned());
    while let Some(page) = next.take() {
        if !visited.insert(page.clone()) {
            return Err("The manifest links form a loop".into());
        }
        let request_headers = with_token(headers, auth).await?;
        let manifest = fetch_json(client, &page, state, egress, &request_headers).await?;
        if !manifest["transactionTime"].is_string() || !manifest["requiresAccessToken"].is_boolean()
        {
            return Err("The manifest requires transactionTime and requiresAccessToken".into());
        }
        found.requires_token |= manifest["requiresAccessToken"] == true;
        let organized = manifest["outputOrganizedBy"].is_string();
        for item in manifest["output"].as_array().into_iter().flatten() {
            let file_url = item["url"]
                .as_str()
                .ok_or("A manifest output item has no url")?;
            parse_source_url(file_url)?;
            let resource_type = item["type"].as_str().map(str::to_owned);
            if !organized && resource_type.is_none() {
                return Err("A manifest output item has no type".into());
            }
            found.files.push((file_url.to_owned(), resource_type));
        }
        if found.files.len() > limits.max_sources {
            return Err("The manifest exceeds the configured file limit".into());
        }
        if manifest["deleted"]
            .as_array()
            .is_some_and(|files| !files.is_empty())
        {
            log.note(
                "warning",
                "not-supported",
                format!("{page}: the deleted files of a submitted manifest are not processed"),
            );
        }
        next = manifest["link"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|link| link["relation"] == "next")
            .and_then(|link| link["url"].as_str())
            .map(str::to_owned);
    }
    Ok(found)
}

/// Collect the `Type/id` of every resource in the file and rewind it. References between
/// files of one manifest then pass validation in any file order.
async fn scan_targets(
    file: &mut tokio::fs::File,
    max_line: usize,
    known: &mut HashSet<String>,
) -> Result<(), String> {
    let mut reader = tokio::io::BufReader::new(&mut *file);
    while let Some(bytes) = read_source_line(&mut reader, max_line).await? {
        if let Ok(resource) = serde_json::from_slice::<Value>(&bytes)
            && resource["resourceType"].is_string()
            && resource["id"].is_string()
        {
            known.insert(target(&resource));
        }
    }
    drop(reader);
    tokio::io::AsyncSeekExt::rewind(file)
        .await
        .map_err(|_| "Cannot rewind the file spool")?;
    Ok(())
}

async fn run_manifest(
    state: &AppState,
    params: &Value,
    log: &mut OutcomeLog,
    manifest: Uuid,
) -> Result<(), String> {
    let config = &state.config.bulk_import;
    let url = params["manifest_url"]
        .as_str()
        .ok_or("Missing manifest url")?;
    let base = params["fhir_base_url"]
        .as_str()
        .ok_or("Missing fhirBaseUrl")?
        .to_owned();
    let headers: Vec<(String, String)> =
        serde_json::from_value(params["file_headers"].clone()).unwrap_or_default();
    let egress = Arc::new(EgressPolicy::from_server_config(&state.config.server)?);
    let client = source_client(state.config.read_timeout(), egress.clone())?;
    let mut auth = ProviderAuth::discover(state, &client, &egress, params).await?;
    let manifests = read_manifests(&client, url, &headers, &mut auth, state, &egress, log).await?;
    if !manifests.requires_token {
        auth = None;
    }

    let mut spooled = Vec::with_capacity(manifests.files.len());
    let mut known = HashSet::new();
    for (file_url, resource_type) in manifests.files {
        let file_headers = with_token(&headers, &mut auth).await?;
        match download_source(
            &client,
            &file_url,
            config.max_source_bytes,
            &egress,
            &file_headers,
        )
        .await
        {
            Ok((mut file, _)) => {
                scan_targets(&mut file, config.max_line_bytes, &mut known).await?;
                let source = FileSource {
                    url: file_url,
                    resource_type,
                };
                spooled.push((source, file));
            }
            Err(error) => {
                log.rejected += 1;
                log.note("error", "transient", format!("{file_url}: {error}"));
            }
        }
    }

    let run = ManifestRun {
        state,
        manifest,
        base,
        trusted: params["trusted"] == true,
        known_refs: Arc::new(known),
    };
    for (index, (source, file)) in spooled.into_iter().enumerate() {
        let mut reader = tokio::io::BufReader::new(file);
        let mut pending: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut pending_bytes = 0usize;
        let mut number = 0usize;
        while let Some(line) = read_source_line(&mut reader, config.max_line_bytes).await? {
            number += 1;
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            if !pending.is_empty()
                && (pending.len() == config.batch_size
                    || line.len() > config.max_batch_bytes.saturating_sub(pending_bytes))
            {
                run.store(index, &source, std::mem::take(&mut pending), log)
                    .await?;
                pending_bytes = 0;
            }
            pending_bytes += line.len();
            pending.push((number, line));
        }
        if !pending.is_empty() {
            run.store(index, &source, pending, log).await?;
        }
    }
    Ok(())
}

impl ManifestRun<'_> {
    async fn store(
        &self,
        file_index: usize,
        source: &FileSource,
        lines: Vec<(usize, Vec<u8>)>,
        log: &mut OutcomeLog,
    ) -> Result<(), String> {
        let (written, rejected) = self.store_batch(file_index, source, lines).await?;
        log.written += written;
        log.rejected += rejected.len();
        if log.lines.len() + rejected.len() > self.state.config.bulk_import.max_errors {
            return Err("The manifest exceeds the configured error limit".into());
        }
        log.lines.extend(rejected);
        Ok(())
    }
}

/// Writes the job's outcome file and returns it as a status manifest item.
async fn write_outcome(
    state: &AppState,
    job_id: Uuid,
    lines: &[Value],
) -> Result<Vec<Value>, String> {
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let config = &state.config.bulk_export;
    let mut writer = NdjsonWriter::new(&config.export_path, job_id, usize::MAX)
        .await
        .map_err(|error| format!("Cannot write the outcome file: {error}"))?;
    let mut severities: HashMap<String, usize> = HashMap::new();
    for line in lines {
        for issue in line["issue"].as_array().into_iter().flatten().take(1) {
            let severity = issue["severity"].as_str().unwrap_or("error").to_owned();
            *severities.entry(severity).or_default() += 1;
        }
        writer
            .write_resource(OUTCOME_FILE, line)
            .await
            .map_err(|error| format!("Cannot write the outcome file: {error}"))?;
    }
    writer
        .close(OUTCOME_FILE)
        .await
        .map_err(|error| format!("Cannot write the outcome file: {error}"))?;
    let mut items = Vec::new();
    for closed in writer.take_closed() {
        let name = closed
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid outcome file name")?;
        let mut severities: Vec<_> = severities.clone().into_iter().collect();
        severities.sort();
        items.push(
            ManifestFile {
                resource_type: None,
                url: format!("{}/_bulk-files/{job_id}/{name}", state.fhir_base_url()),
                count: closed.count,
                bytes: closed.bytes,
                severities,
            }
            .item(BulkSpecVersion::Stu4),
        );
    }
    Ok(items)
}

async fn finish_manifest(
    state: &AppState,
    manifest: Uuid,
    status: &str,
    outcome: &[Value],
) -> Result<(), String> {
    query(
        "UPDATE bulk_submission_manifests SET status = $2, outcome = outcome || $3::jsonb, \
         updated_at = NOW() WHERE id = $1",
    )
    .bind(manifest)
    .bind(status)
    .bind(json!(outcome))
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|error| format!("Cannot update the manifest: {error}"))?;
    Ok(())
}

/// `fileRequestHeader` values are credentials; they stay only while the job needs them.
async fn forget_file_headers(state: &AppState, job_id: Uuid) {
    if let Err(error) =
        query("UPDATE async_jobs SET request_body = request_body - 'file_headers' WHERE id = $1")
            .bind(job_id)
            .execute(state.db_pool.as_ref())
            .await
    {
        tracing::warn!(%job_id, %error, "Cannot clear submission headers");
    }
}

/// Fetch and store one manifest. A failure here is the submission's outcome, reported
/// through the status manifest, not a failure of the job.
pub async fn execute_bulk_submit(
    state: AppState,
    job_id: Uuid,
    params: Value,
) -> Result<Value, String> {
    let manifest: Uuid = serde_json::from_value(params["manifest"].clone())
        .map_err(|_| "Missing manifest in the job")?;
    let url = params["manifest_url"].as_str().unwrap_or_default();
    query("UPDATE bulk_submission_manifests SET status = 'processing' WHERE id = $1")
        .bind(manifest)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|error| format!("Cannot update the manifest: {error}"))?;

    let mut log = OutcomeLog::default();
    if params["unknown_options"].as_u64().unwrap_or(0) > 0 {
        log.note(
            "warning",
            "not-supported",
            "metadata and import parameters are not defined for this server and were ignored"
                .into(),
        );
    }
    let trusted = params["trusted"] == true;
    let replaced: Option<Uuid> = serde_json::from_value(params["replaces"].clone()).ok();
    let mut result = Ok(());
    if let Some(previous) = replaced {
        let writes = load_writes(&state, &[previous]).await?;
        match take_back(&state, writes, trusted, "replace").await {
            Ok(notes) => {
                log.lines.extend(notes);
                clear_writes(&state, &[previous]).await?;
                finish_manifest(&state, previous, "replaced", &[]).await?;
            }
            Err(error) => result = Err(error),
        }
    }
    if result.is_ok() {
        result = run_manifest(&state, &params, &mut log, manifest).await;
    }
    let status = match &result {
        Ok(()) => {
            log.note(
                "information",
                "informational",
                format!(
                    "Manifest {url}: {} resources stored, {} rejected",
                    log.written, log.rejected
                ),
            );
            "completed"
        }
        Err(error) => {
            log.note("error", "exception", format!("Manifest {url}: {error}"));
            "failed"
        }
    };
    let outcome = write_outcome(&state, job_id, &log.lines).await?;
    finish_manifest(&state, manifest, status, &outcome).await?;
    forget_file_headers(&state, job_id).await;
    Ok(json!({"manifestUrl": url, "status": status, "outcome": outcome}))
}

/// One resource to restore.
struct RecordedWrite {
    resource_type: String,
    id: String,
    /// The version before the first write, `None` if the submission created it.
    prior: Option<String>,
    /// The version the last write left, `None` if it may not have committed.
    written: Option<String>,
}

/// What these manifests wrote: per resource, the state before the first of them and
/// after the last, so writes of several manifests undo as one.
async fn load_writes(state: &AppState, manifests: &[Uuid]) -> Result<Vec<RecordedWrite>, String> {
    let rows = query(
        "SELECT resource_type, resource_id, \
                (array_agg(prior_version ORDER BY seq))[1] AS prior, \
                (array_agg(written_version ORDER BY seq DESC))[1] AS written \
         FROM bulk_submission_writes WHERE manifest = ANY($1) \
         GROUP BY resource_type, resource_id ORDER BY resource_type, resource_id",
    )
    .bind(manifests)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|error| format!("Cannot read submission writes: {error}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(RecordedWrite {
                resource_type: row.try_get("resource_type")?,
                id: row.try_get("resource_id")?,
                prior: row.try_get("prior")?,
                written: row.try_get("written")?,
            })
        })
        .collect::<Result<_, sqlx_core::Error>>()
        .map_err(|error| format!("Cannot read submission writes: {error}"))
}

async fn clear_writes(state: &AppState, manifests: &[Uuid]) -> Result<(), String> {
    query("DELETE FROM bulk_submission_writes WHERE manifest = ANY($1)")
        .bind(manifests)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|error| format!("Cannot clear submission writes: {error}"))?;
    Ok(())
}

/// Puts the resources back: what a submission created is deleted, what it changed
/// returns to its earlier version. A resource changed since is someone else's work and
/// stays; the returned notes say so.
async fn take_back(
    state: &AppState,
    writes: Vec<RecordedWrite>,
    trusted: bool,
    step: &str,
) -> Result<Vec<Value>, String> {
    let batch = state.config.bulk_import.batch_size.max(1);
    let mut notes = Vec::new();
    for (index, chunk) in writes.chunks(batch).enumerate() {
        let mut groups: HashMap<&str, Vec<String>> = HashMap::new();
        for write in chunk {
            groups
                .entry(&write.resource_type)
                .or_default()
                .push(write.id.clone());
        }
        let mut current: HashMap<(String, String), String> = HashMap::new();
        for (resource_type, ids) in groups {
            for (id, version) in versions(state, resource_type, &ids).await? {
                current.insert((resource_type.to_owned(), id), version);
            }
        }
        let mut entries = Vec::new();
        for write in chunk {
            let key = (write.resource_type.clone(), write.id.clone());
            let Some(version) = current.get(&key) else {
                continue;
            };
            if write.prior.as_ref() == Some(version) {
                continue;
            }
            if write
                .written
                .as_ref()
                .is_some_and(|written| written != version)
            {
                notes.push(json!({"resourceType": "OperationOutcome", "issue": [{
                    "severity": "warning",
                    "code": "conflict",
                    "diagnostics": format!(
                        "{}/{} changed after the submission and was left as it is",
                        write.resource_type, write.id
                    ),
                }]}));
                continue;
            }
            let url = format!("{}/{}", write.resource_type, write.id);
            match &write.prior {
                None => entries.push(json!({"request": {"method": "DELETE", "url": url}})),
                Some(prior) => {
                    let stored = state
                        .storage
                        .vread(&write.resource_type, &write.id, prior)
                        .await
                        .map_err(|error| format!("Cannot read {url}: {error}"))?
                        .ok_or_else(|| format!("{url} has no version {prior}"))?;
                    entries.push(json!({
                        "resource": stored.resource,
                        "request": {"method": "PUT", "url": url},
                    }));
                }
            }
        }
        if entries.is_empty() {
            continue;
        }
        let checkpoint = CheckpointStep::new(format!("{step}-{index}"), &json!(entries))
            .map_err(|error| error.to_string())?;
        write_chunk(state, entries, trusted, checkpoint)
            .await
            .map_err(|error| format!("Cannot take the data back: {error}"))?;
    }
    Ok(notes)
}

/// `stopped` and a replacement without a successor: take the manifests' data back.
pub async fn execute_bulk_submit_revert(
    state: AppState,
    job_id: Uuid,
    params: Value,
) -> Result<Value, String> {
    let manifests: Vec<Uuid> = serde_json::from_value(params["manifests"].clone())
        .map_err(|_| "Missing manifests in the job")?;
    let final_status = params["final_status"]
        .as_str()
        .ok_or("Missing final status in the job")?;
    let writes = load_writes(&state, &manifests).await?;
    let count = writes.len();
    let mut lines = match take_back(&state, writes, params["trusted"] == true, "revert").await {
        Ok(notes) => notes,
        Err(error) => vec![json!({"resourceType": "OperationOutcome", "issue": [{
            "severity": "error", "code": "exception", "diagnostics": error,
        }]})],
    };
    let failed = lines
        .iter()
        .any(|line| line["issue"][0]["severity"] == "error");
    if !failed {
        clear_writes(&state, &manifests).await?;
        lines.push(json!({"resourceType": "OperationOutcome", "issue": [{
            "severity": "information",
            "code": "informational",
            "diagnostics": format!("Data of {count} resources was taken back"),
        }]}));
    }
    let outcome = write_outcome(&state, job_id, &lines).await?;
    for (index, manifest) in manifests.iter().enumerate() {
        // The file covers all of them; each outcome item names one manifest.
        let attached: &[Value] = if index == 0 { &outcome } else { &[] };
        finish_manifest(&state, *manifest, final_status, attached).await?;
    }
    Ok(json!({"taken_back": count, "outcome": outcome}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(extra: Value) -> Value {
        let mut parameter = vec![
            json!({"name":"submitter","valueIdentifier":{"system":"urn:s","value":"a"}}),
            json!({"name":"submissionId","valueString":"1"}),
        ];
        parameter.extend(extra.as_array().cloned().unwrap_or_default());
        json!({"resourceType":"Parameters","parameter":parameter})
    }

    #[test]
    fn a_request_needs_a_manifest_or_a_status_and_a_base_with_a_manifest() {
        assert!(SubmitRequest::parse(&request(json!([]))).is_err());
        assert!(
            SubmitRequest::parse(&request(
                json!([{"name":"manifestUrl","valueUrl":"https://example.org/m"}])
            ))
            .is_err()
        );
        let parsed = SubmitRequest::parse(&request(json!([
            {"name":"manifestUrl","valueUrl":"https://example.org/m"},
            {"name":"fhirBaseUrl","valueUrl":"https://example.org/fhir"},
            {"name":"fileRequestHeader","part":[
                {"name":"headerName","valueString":"Authorization"},
                {"name":"headerValue","valueString":"Bearer x"}]}
        ])))
        .unwrap();
        assert_eq!(parsed.headers.len(), 1);
        assert!(parsed.status.is_none());
    }

    #[test]
    fn status_codes_come_from_the_value_set() {
        let status = |system: &str, code: &str| {
            SubmitRequest::parse(&request(json!([
                {"name":"submissionStatus","valueCoding":{"system":system,"code":code}}
            ])))
        };
        assert_eq!(
            status(EVENT_STATUS, "stopped").unwrap().status.as_deref(),
            Some("stopped")
        );
        assert!(status(EVENT_STATUS, "entered-in-error").is_err());
        assert!(status("urn:other", "stopped").is_err());
    }

    #[test]
    fn what_cannot_be_honoured_is_refused_not_ignored() {
        let extra = json!([
            {"name":"submissionStatus","valueCoding":{"system":EVENT_STATUS,"code":"completed"}},
            {"name":"fileEncryptionKey","part":[{"name":"value","valueString":"k"}]}
        ]);
        assert!(matches!(
            SubmitRequest::parse(&request(extra)),
            Err(OperationError::NotSupported(_))
        ));
        let reserved = request(json!([
            {"name":"submissionStatus","valueCoding":{"system":EVENT_STATUS,"code":"completed"}},
            {"name":"fileRequestHeader","part":[
                {"name":"headerName","valueString":"Host"},
                {"name":"headerValue","valueString":"internal"}]}
        ]));
        assert!(SubmitRequest::parse(&reserved).is_err());
    }

    #[test]
    fn references_under_the_sender_base_become_local() {
        let mut resource = json!({"subject":{"reference":"https://a.example/fhir/Patient/1"},
            "performer":[{"reference":"https://other.example/fhir/Practitioner/2"}]});
        localize_references(&mut resource, "https://a.example/fhir");
        assert_eq!(resource["subject"]["reference"], "Patient/1");
        assert_eq!(
            resource["performer"][0]["reference"],
            "https://other.example/fhir/Practitioner/2"
        );
    }

    #[test]
    fn lines_are_checked_against_their_file() {
        let parse = |line: &str, expected| {
            parse_line("f", 1, line.as_bytes(), expected, "https://a.example/fhir")
        };
        assert!(parse("not json", None).is_err());
        assert!(parse("{\"id\":\"1\"}", None).is_err());
        assert!(
            parse(
                "{\"resourceType\":\"Patient\",\"id\":\"1\"}",
                Some("Observation")
            )
            .is_err()
        );
        assert!(parse("{\"resourceType\":\"Patient\",\"id\":\"a b\"}", None).is_err());
        let header = "{\"resourceType\":\"Parameters\",\"parameter\":[{\"name\":\"header\"}]}";
        assert!(parse(header, None).unwrap().is_none());
        let generated = parse("{\"resourceType\":\"Patient\"}", None)
            .unwrap()
            .unwrap();
        assert!(generated.resource["id"].is_string());
    }

    #[test]
    fn rejected_resources_point_at_the_source() {
        let outcome = RejectedLine {
            resource: Some("Patient/1".into()),
            issues: vec![json!({"severity":"error","code":"invalid"})],
        }
        .outcome("https://a.example/fhir/");
        assert_eq!(outcome["extension"][0]["url"], SOURCE_RESOURCE);
        assert_eq!(
            outcome["extension"][0]["valueReference"]["reference"],
            "https://a.example/fhir/Patient/1"
        );
    }
}
