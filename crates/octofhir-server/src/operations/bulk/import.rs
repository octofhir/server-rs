//! Bulk import operation handler ($import)
//!
//! Imports NDJSON resources via async job.
//!
//! - System: `POST /$import` with Parameters resource containing NDJSON URLs
//!
//! The Parameters resource follows the Bulk Data Import pattern:
//! ```json
//! {
//!   "resourceType": "Parameters",
//!   "parameter": [
//!     {
//!       "name": "input",
//!       "part": [
//!         { "name": "type", "valueString": "Patient" },
//!         { "name": "url", "valueUrl": "https://example.com/patients.ndjson" }
//!       ]
//!     }
//!   ]
//! }
//! ```

use crate::operations::handler::OperationLevel;

use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use octofhir_fhir_model::provider::ModelProvider;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

use crate::async_jobs::AsyncJobRequest;
use crate::config::BulkImportConfig;
use crate::operations::handler::{OperationError, OperationHandler};
use crate::server::AppState;

/// Input source descriptor for import
#[derive(Debug, Clone)]
struct ImportInput {
    resource_type: String,
    url: String,
}

/// The $import operation handler
pub struct ImportOperation {
    config: BulkImportConfig,
}

impl ImportOperation {
    pub fn new(config: BulkImportConfig) -> Self {
        Self { config }
    }

    /// Parse FHIR Parameters resource into import inputs
    fn parse_inputs(params: &Value) -> Result<Vec<ImportInput>, OperationError> {
        let params = effective_params(params);

        // Support both direct JSON format and FHIR Parameters resource
        let inputs = if let Some(parameter) = params.get("parameter").and_then(|v| v.as_array()) {
            // FHIR Parameters format
            let mut result = Vec::new();
            for param in parameter {
                let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if name != "input" {
                    continue;
                }
                let parts = param.get("part").and_then(|v| v.as_array());
                let parts = match parts {
                    Some(p) => p,
                    None => {
                        return Err(OperationError::InvalidParameters(
                            "Each input requires a part array".into(),
                        ));
                    }
                };

                let mut resource_type = None;
                let mut url = None;
                let mut seen = std::collections::HashSet::new();

                for part in parts {
                    let part_name = part.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if !seen.insert(part_name) {
                        return Err(OperationError::InvalidParameters(
                            "Duplicate import input part".into(),
                        ));
                    }
                    if part.get("valueUrl").is_some() && part.get("valueString").is_some() {
                        return Err(OperationError::InvalidParameters(
                            "Import input part has multiple values".into(),
                        ));
                    }
                    match part_name {
                        "type" => {
                            if resource_type.is_some() {
                                return Err(OperationError::InvalidParameters(
                                    "Duplicate input type".into(),
                                ));
                            }
                            resource_type = part
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                        }
                        "url" => {
                            if url.is_some() {
                                return Err(OperationError::InvalidParameters(
                                    "Duplicate input url".into(),
                                ));
                            }
                            url = part
                                .get("valueUrl")
                                .or_else(|| part.get("valueString"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                        }
                        _ => {
                            return Err(OperationError::InvalidParameters(
                                "Unknown import input part".into(),
                            ));
                        }
                    }
                }

                match (resource_type, url) {
                    (Some(rt), Some(u)) => result.push(ImportInput {
                        resource_type: rt,
                        url: u,
                    }),
                    _ => {
                        return Err(OperationError::InvalidParameters(
                            "Each input must have 'type' and 'url' parts".to_string(),
                        ));
                    }
                }
            }
            result
        } else if let Some(input) = params.get("input").and_then(|v| v.as_array()) {
            // Simplified JSON format: { "input": [{ "type": "Patient", "url": "..." }] }
            let mut result = Vec::new();
            for item in input {
                let resource_type = item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        OperationError::InvalidParameters(
                            "Each input must have a 'type' field".to_string(),
                        )
                    })?
                    .to_string();
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        OperationError::InvalidParameters(
                            "Each input must have a 'url' field".to_string(),
                        )
                    })?
                    .to_string();
                result.push(ImportInput { resource_type, url });
            }
            result
        } else {
            return Err(OperationError::InvalidParameters(
                "Request must contain 'parameter' (FHIR Parameters) or 'input' array".to_string(),
            ));
        };

        if inputs.is_empty() {
            return Err(OperationError::InvalidParameters(
                "At least one input source is required".to_string(),
            ));
        }

        for input in &inputs {
            input
                .resource_type
                .parse::<octofhir_core::ResourceType>()
                .map_err(|_| {
                    OperationError::InvalidParameters("Unknown import resource type".into())
                })?;
            parse_source_url(&input.url).map_err(OperationError::InvalidParameters)?;
        }
        Ok(inputs)
    }
}

#[async_trait]
impl OperationHandler for ImportOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        self.config.enabled && level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "import"
    }

    /// System-level: POST /$import
    async fn handle_system(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if !self.config.enabled {
            return Err(OperationError::NotSupported(
                "$import is not enabled".to_string(),
            ));
        }

        let inputs = Self::parse_inputs(params)?;

        if inputs.len() > self.config.max_sources {
            return Err(OperationError::InvalidParameters(
                "Import source count exceeds the configured limit".into(),
            ));
        }
        let params = effective_params(params);
        let skip_validation = match parameter(params, "skipValidation")? {
            None => self.config.default_skip_validation,
            Some(value) => value.as_bool().ok_or_else(|| {
                OperationError::InvalidParameters("skipValidation must be boolean".into())
            })?,
        };
        let batch_size = positive_parameter(params, "batchSize", self.config.batch_size)?;
        let parallelism =
            positive_parameter(params, "parallelism", self.config.max_parallel_resources)?;
        if let Some(format) = parameter(params, "inputFormat")?
            && !matches!(
                format.as_str(),
                Some("application/fhir+ndjson" | "application/ndjson" | "application/x-ndjson")
            )
        {
            return Err(OperationError::InvalidParameters(
                "Only NDJSON import is implemented".into(),
            ));
        }
        let known_types = state
            .model_provider
            .get_resource_types()
            .await
            .map_err(|_| OperationError::Internal("Cannot load import resource types".into()))?;
        for input in &inputs {
            if !known_types.contains(&input.resource_type) {
                return Err(OperationError::InvalidParameters(
                    "Unknown import resource type".into(),
                ));
            }
            let create = context.policy_context(
                state,
                octofhir_auth::smart::scopes::FhirOperation::Create,
                &input.resource_type,
                None,
                &Default::default(),
            );
            if let Err(error) = create {
                if !error.is_forbidden() {
                    return Err(error);
                }
                context.policy_context(
                    state,
                    octofhir_auth::smart::scopes::FhirOperation::Update,
                    &input.resource_type,
                    None,
                    &Default::default(),
                )?;
            }
        }

        // Build serializable input list for the job
        let input_list: Vec<Value> = inputs
            .iter()
            .map(|i| {
                json!({
                    "type": i.resource_type,
                    "url": i.url,
                })
            })
            .collect();

        let job_params = json!({
            "input": input_list,
            "batch_size": batch_size,
            "parallelism": parallelism,
            "skip_validation": skip_validation,
        });

        let async_request = AsyncJobRequest {
            request_type: "bulk_import".to_string(),
            method: "POST".to_string(),
            url: context.request_url().to_owned(),
            body: Some(job_params),
            headers: None,
            client_id: context.auth().map(|auth| auth.client_id().to_owned()),
            authorization: context.job_authorization(state)?,
        };

        let job_id = state
            .async_job_manager
            .submit_job(async_request)
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to submit import job: {e}")))?;

        tracing::info!(
            job_id = %job_id,
            inputs = inputs.len(),
            "Bulk import job submitted"
        );

        Ok(json!({
            "status": "accepted",
            "job_id": job_id.to_string(),
            "status_url": format!("{}/fhir/_async-status/{}", state.base_url, job_id),
        }))
    }
}

/// A parsed import line, ready for the shared native write protocol.
struct PreparedImportResource {
    line_number: usize,
    resource: Value,
}

/// Parse + validate one NDJSON line. No database access — writes happen in
/// the shared transaction protocol so a chunk has one atomic commit.
async fn prepare_import_line(
    job_id: Uuid,
    resource_type: String,
    source_url: String,
    line_number: usize,
    line: String,
) -> Result<PreparedImportResource, Value> {
    let resource: Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                job_id = %job_id,
                line = line_number,
                error = %e,
                "Invalid JSON line, skipping"
            );
            return Err(json!({
                "source": source_url,
                "line": line_number,
                "error": format!("Invalid JSON: {e}"),
            }));
        }
    };

    let actual_type = resource.get("resourceType").and_then(|v| v.as_str());
    if actual_type != Some(resource_type.as_str()) {
        return Err(json!({
            "source": source_url,
            "line": line_number,
            "error": format!(
                "Expected resourceType '{}', got '{}'",
                resource_type,
                actual_type.unwrap_or("null")
            ),
        }));
    }

    if let Some(id) = resource.get("id")
        && id
            .as_str()
            .is_none_or(|id| octofhir_core::validate_id(id).is_err())
    {
        return Err(
            json!({"source":source_url, "line":line_number, "error":"Invalid resource id"}),
        );
    }
    Ok(PreparedImportResource {
        line_number,
        resource,
    })
}

#[derive(Clone, Copy)]
struct ImportSource<'a> {
    index: usize,
    resource_type: &'a str,
    url: &'a str,
}

async fn process_import_batch(
    state: AppState,
    job_id: Uuid,
    source: ImportSource<'_>,
    lines: Vec<(usize, String)>,
    parallelism: usize,
    skip_validation: bool,
) -> Result<(usize, Vec<Value>), String> {
    let resource_type = source.resource_type;
    let source_url = source.url;
    let first_line = lines
        .first()
        .map(|(line, _)| *line)
        .ok_or("Empty import chunk")?;
    let mut step = crate::async_jobs::CheckpointStep::new(
        format!("import-{}-{first_line}", source.index),
        &json!({"source":source_url, "type":resource_type, "lines":lines, "skip_validation":skip_validation}),
    ).map_err(|error| error.to_string())?;
    step.count_only = true;
    crate::operations::OperationContext::for_job(&state, job_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(record) = state
        .async_job_manager
        .restore_checkpoint(&step)
        .await
        .map_err(|error| error.to_string())?
    {
        let errors = record["metadata"]["errors"]
            .as_array()
            .cloned()
            .ok_or("Invalid import checkpoint errors")?;
        return Ok((
            record["result"]["created"]
                .as_u64()
                .and_then(|count| usize::try_from(count).ok())
                .ok_or("Invalid import checkpoint count")?,
            errors,
        ));
    }
    let concurrency = lines.len().min(parallelism).max(1);
    let mut created = 0usize;
    let mut errors = Vec::new();

    // Phase 1 — parse + validate in parallel (no DB writes).
    let mut prepared: Vec<PreparedImportResource> = Vec::with_capacity(lines.len());
    let mut outcomes =
        stream::iter(lines.into_iter().map(|(line_number, line)| {
            let resource_type = resource_type.to_string();
            let source_url = source_url.to_string();
            async move {
                prepare_import_line(job_id, resource_type, source_url, line_number, line).await
            }
        }))
        .buffer_unordered(concurrency);

    while let Some(outcome) = outcomes.next().await {
        match outcome {
            Ok(p) => prepared.push(p),
            Err(error) => errors.push(error),
        }
    }

    if prepared.is_empty() {
        step.metadata = json!({"errors": errors});
        state
            .async_job_manager
            .save_read_checkpoint(
                &step,
                &json!({"resourceType":"Bundle", "type":"transaction-response", "entry":[]}),
                state.config.search.include_limits.max_bytes,
            )
            .await
            .map_err(|error| error.to_string())?;
        return Ok((created, errors));
    }

    prepared.sort_by_key(|resource| resource.line_number);
    let entries = prepared.iter().map(|resource| {
        let id = resource.resource["id"].as_str();
        json!({"resource": resource.resource, "request": {
            "method": if id.is_some() {"PUT"} else {"POST"},
            "url": id.map_or_else(|| resource_type.to_owned(), |id| format!("{resource_type}/{id}"))
        }})
    }).collect();
    step.metadata = json!({"errors": errors});
    match crate::handlers::execute_worker_write_chunk(
        &state,
        entries,
        skip_validation,
        step.clone(),
    )
    .await
    {
        Ok(result) => {
            created = if let Some(entries) = result["entry"].as_array() {
                entries.len()
            } else {
                result["created"]
                    .as_u64()
                    .and_then(|count| usize::try_from(count).ok())
                    .ok_or("Invalid restored import count")?
            };
        }
        Err(error) => {
            // A failed native chunk has rolled back as a group. Report every
            // affected line; never fall back to weaker individual upserts.
            for resource in &prepared {
                errors.push(json!({"source":source_url, "line":resource.line_number,
                    "error":error.to_operation_outcome()}));
            }
            step.metadata = json!({"errors": errors});
            state
                .async_job_manager
                .save_read_checkpoint(
                    &step,
                    &json!({"resourceType":"Bundle", "type":"transaction-response", "entry":[]}),
                    state.config.search.include_limits.max_bytes,
                )
                .await
                .map_err(|error| error.to_string())?;
        }
    }

    Ok((created, errors))
}

// Unwrap only the extractor's single-resource wrapper. Parameters themselves
// retain typed primitive values and repeats for strict validation.
fn effective_params(params: &Value) -> &Value {
    if let Some(parameters) = params["parameter"].as_array()
        && parameters.len() == 1
        && parameters[0]["name"] == "resource"
    {
        return parameters[0].get("resource").unwrap_or(params);
    }
    params
}

fn parameter<'a>(params: &'a Value, name: &str) -> Result<Option<&'a Value>, OperationError> {
    let Some(parameters) = params.get("parameter") else {
        return Ok(params.get(name));
    };
    let parameters = parameters
        .as_array()
        .ok_or_else(|| OperationError::InvalidParameters("parameter must be an array".into()))?;
    let mut values = parameters
        .iter()
        .filter(|parameter| parameter["name"] == name);
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(OperationError::InvalidParameters(format!(
            "{name} must occur once"
        )));
    }
    let mut fields = value
        .as_object()
        .into_iter()
        .flat_map(|obj| obj.iter())
        .filter(|(key, _)| key.starts_with("value"));
    let result = fields.next().map(|(_, value)| value).ok_or_else(|| {
        OperationError::InvalidParameters(format!("{name} requires a primitive value"))
    })?;
    if fields.next().is_some() {
        return Err(OperationError::InvalidParameters(format!(
            "{name} has multiple values"
        )));
    }
    Ok(Some(result))
}

fn positive_parameter(params: &Value, name: &str, maximum: usize) -> Result<usize, OperationError> {
    let Some(value) = parameter(params, name)? else {
        return Ok(maximum);
    };
    let number = value
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .filter(|number| *number > 0 && *number <= maximum)
        .ok_or_else(|| {
            OperationError::InvalidParameters(format!("{name} must be between 1 and {maximum}"))
        })?;
    Ok(number)
}

fn parse_source_url(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Invalid import source URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Import requires an HTTP(S) source URL without credentials or fragment".into());
    }
    Ok(url)
}

fn source_client(timeout: std::time::Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let target = attempt.url();
            if attempt.previous().len() >= 10 {
                return attempt.error("Import redirect limit exceeded");
            }
            if parse_source_url(target.as_str()).is_err()
                || attempt
                    .previous()
                    .first()
                    .is_some_and(|original| original.origin() != target.origin())
            {
                return attempt.error("Import redirect must retain the source origin");
            }
            attempt.follow()
        }))
        .build()
        .map_err(|_| "Cannot construct import HTTP client".into())
}

/// Download and attest the entire source before any chunk is committed. An
/// unnamed temporary file drops on cancellation; memory is bounded by one HTTP
/// chunk and later one native transaction batch. A changed suffix or truncated
/// source is rejected before any further write on recovery.
async fn download_source(
    client: &reqwest::Client,
    url: &str,
    max_bytes: u64,
) -> Result<(tokio::fs::File, String), String> {
    let url = parse_source_url(url)?;
    let mut response = client.get(url).send().await.map_err(|error| {
        if error.is_timeout() {
            "Import source request timed out".into()
        } else {
            format!("Import source request failed: {}", error.without_url())
        }
    })?;
    if !response.status().is_success() {
        return Err(format!("Import source returned HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err("Import source exceeds the configured byte limit".into());
    }
    let file = tempfile::tempfile().map_err(|_| "Cannot create import source spool")?;
    let mut file = tokio::fs::File::from_std(file);
    let mut length = 0u64;
    let mut digest = Sha256::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            "Import source read timed out".into()
        } else {
            format!("Import source read failed: {}", error.without_url())
        }
    })? {
        length = length
            .checked_add(chunk.len() as u64)
            .ok_or("Import source size overflow")?;
        if length > max_bytes {
            return Err("Import source exceeds the configured byte limit".into());
        }
        digest.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|_| "Cannot write import source spool")?;
    }
    file.flush()
        .await
        .map_err(|_| "Cannot flush import source spool")?;
    file.rewind()
        .await
        .map_err(|_| "Cannot rewind import source spool")?;
    Ok((file, hex::encode(digest.finalize())))
}

async fn read_source_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    maximum: usize,
) -> Result<Option<Vec<u8>>, String> {
    let mut line = Vec::new();
    loop {
        let bytes = reader
            .fill_buf()
            .await
            .map_err(|_| "Cannot read import source spool")?;
        if bytes.is_empty() {
            return Ok((!line.is_empty()).then_some(line));
        }
        let newline = bytes.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(bytes.len(), |index| index + 1);
        if count > maximum.saturating_sub(line.len()) {
            return Err("NDJSON line exceeds the configured byte limit".into());
        }
        line.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
}

fn collect_errors(
    details: &mut Vec<Value>,
    mut errors: Vec<Value>,
    maximum: usize,
) -> Result<(), String> {
    if errors.len() > maximum.saturating_sub(details.len()) {
        return Err("Import partial failures exceed the configured error limit; committed chunks remain checkpointed".into());
    }
    details.append(&mut errors);
    Ok(())
}

/// Execute a bulk import job (called by async job executor).
pub async fn execute_bulk_import(
    state: AppState,
    job_id: Uuid,
    params: Value,
) -> Result<Value, String> {
    let config = &state.config.bulk_import;
    let inputs = ImportOperation::parse_inputs(&params).map_err(|error| error.to_string())?;
    if inputs.len() > config.max_sources {
        return Err("Import source count exceeds the configured limit".into());
    }
    let batch_size = positive_parameter(&params, "batch_size", config.batch_size)
        .map_err(|error| error.to_string())?;
    let parallelism = positive_parameter(&params, "parallelism", config.max_parallel_resources)
        .map_err(|error| error.to_string())?;
    let skip_validation = params["skip_validation"]
        .as_bool()
        .ok_or("Missing skip_validation in import job")?;
    let http_client = source_client(state.config.read_timeout())?;
    let mut total_created = 0usize;
    let mut error_details = Vec::new();
    for (idx, input) in inputs.iter().enumerate() {
        crate::operations::OperationContext::for_job(&state, job_id)
            .await
            .map_err(|error| error.to_string())?;
        tracing::info!(%job_id, resource_type = %input.resource_type, input_index = idx, "Downloading import source");
        let (file, digest) =
            download_source(&http_client, &input.url, config.max_source_bytes).await?;
        let attestation = crate::async_jobs::CheckpointStep::new(format!("import-source-{idx}"), &json!({"url":input.url,"type":input.resource_type,"sha256":digest,"batch_size":batch_size,"max_batch_bytes":config.max_batch_bytes,"skip_validation":skip_validation})).map_err(|error| error.to_string())?;
        if state
            .async_job_manager
            .restore_checkpoint(&attestation)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
        {
            state
                .async_job_manager
                .save_read_checkpoint(
                    &attestation,
                    &Value::Null,
                    state.config.search.include_limits.max_bytes,
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        let mut reader = tokio::io::BufReader::new(file);
        let mut line_number = 0usize;
        let mut pending = Vec::with_capacity(batch_size);
        let mut pending_bytes = 0usize;
        let mut source_created = 0usize;
        while let Some(bytes) = read_source_line(&mut reader, config.max_line_bytes).await? {
            line_number = line_number
                .checked_add(1)
                .ok_or("Import line number overflow")?;
            if bytes.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let line = String::from_utf8(bytes)
                .map_err(|_| format!("Input {idx}, line {line_number}: NDJSON must be UTF-8"))?;
            if !pending.is_empty()
                && (pending.len() == batch_size
                    || line.len() > config.max_batch_bytes.saturating_sub(pending_bytes))
            {
                let (created, errors) = process_import_batch(
                    state.clone(),
                    job_id,
                    ImportSource {
                        index: idx,
                        resource_type: &input.resource_type,
                        url: &input.url,
                    },
                    std::mem::take(&mut pending),
                    parallelism,
                    skip_validation,
                )
                .await?;
                source_created += created;
                collect_errors(&mut error_details, errors, config.max_errors)?;
                pending_bytes = 0;
            }
            pending_bytes += line.len();
            pending.push((line_number, line));
        }
        if !pending.is_empty() {
            let (created, errors) = process_import_batch(
                state.clone(),
                job_id,
                ImportSource {
                    index: idx,
                    resource_type: &input.resource_type,
                    url: &input.url,
                },
                pending,
                parallelism,
                skip_validation,
            )
            .await?;
            source_created += created;
            collect_errors(&mut error_details, errors, config.max_errors)?;
        }
        total_created += source_created;
        state
            .async_job_manager
            .update_progress(job_id, (idx + 1) as f32 / inputs.len() as f32)
            .await
            .map_err(|error| error.to_string())?;
        tracing::info!(%job_id, input_index = idx, created = source_created, "Completed native import source");
    }
    let result = json!({"total_created":total_created, "total_errors":error_details.len(), "errors":error_details});
    crate::async_jobs::ensure_result_budget(&result, state.config.search.include_limits.max_bytes)
        .map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[test]
    fn import_parameters_reject_invalid_sources_duplicates_and_unbounded_options() {
        for url in [
            "file:///etc/passwd",
            "ftp://example.org/a",
            "https://user:pass@example.org/a",
            "https://example.org/a#part",
            "not a url",
        ] {
            assert!(
                ImportOperation::parse_inputs(&json!({"input":[{"type":"Patient","url":url}]}))
                    .is_err()
            );
        }
        let input = json!({"resourceType":"Parameters", "parameter":[{"name":"input","part":[{"name":"type","valueString":"Patient"},{"name":"url","valueUrl":"https://example.org/a"}]}]});
        assert_eq!(ImportOperation::parse_inputs(&input).unwrap().len(), 1);
        let mut duplicate = input.clone();
        duplicate["parameter"][0]["part"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"type","valueString":"Observation"}));
        assert!(ImportOperation::parse_inputs(&duplicate).is_err());
        for number in [json!(0), json!(-1), json!(1001), json!("10"), json!(1.5)] {
            assert!(positive_parameter(&json!({"batchSize":number}), "batchSize", 1000).is_err());
        }
        assert_eq!(
            positive_parameter(
                &json!({"parameter":[{"name":"batchSize","valueInteger":10}]}),
                "batchSize",
                1000
            )
            .unwrap(),
            10
        );
        assert!(parameter(&json!({"parameter":[{"name":"skipValidation","valueBoolean":true},{"name":"skipValidation","valueBoolean":false}]}), "skipValidation").is_err());
    }

    #[tokio::test]
    async fn download_attests_full_input_and_bounds_bytes_redirects_and_timeouts() {
        let source = MockServer::start().await;
        let other = MockServer::start().await;
        let body = "{\"resourceType\":\"Patient\"}\n\n";
        Mock::given(method("GET"))
            .and(path("/data"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/same"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/data"))
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/other"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/data", other.uri())),
            )
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/slow"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(300)))
            .mount(&source)
            .await;
        let client = source_client(Duration::from_millis(100)).unwrap();
        let (file, digest) = download_source(&client, &format!("{}/same", source.uri()), 1000)
            .await
            .unwrap();
        assert_eq!(digest, hex::encode(Sha256::digest(body.as_bytes())));
        let mut reader = tokio::io::BufReader::with_capacity(2, file);
        assert_eq!(
            read_source_line(&mut reader, 100).await.unwrap().unwrap(),
            b"{\"resourceType\":\"Patient\"}\n"
        );
        assert_eq!(
            read_source_line(&mut reader, 100).await.unwrap().unwrap(),
            b"\n"
        );
        assert!(read_source_line(&mut reader, 100).await.unwrap().is_none());
        assert!(
            download_source(&client, &format!("{}/data", source.uri()), 1)
                .await
                .is_err()
        );
        assert!(
            download_source(&client, &format!("{}/other", source.uri()), 1000)
                .await
                .is_err()
        );
        assert!(other.received_requests().await.unwrap().is_empty());
        assert!(
            download_source(
                &client,
                &format!("{}/slow?secret=never-log", source.uri()),
                1000
            )
            .await
            .unwrap_err()
            .contains("timed out")
        );
        let mut oversized = tokio::io::BufReader::with_capacity(2, &b"abcde\n"[..]);
        assert!(read_source_line(&mut oversized, 4).await.is_err());
        let mut trailing = tokio::io::BufReader::with_capacity(2, &b"abcd"[..]);
        assert_eq!(
            read_source_line(&mut trailing, 4).await.unwrap().unwrap(),
            b"abcd"
        );
    }
}
