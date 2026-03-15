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

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{StreamExt, stream};
use octofhir_db_postgres::SchemaManager;
use octofhir_search::SearchParameterRegistry;
use serde_json::{Value, json};
use sqlx_core::query_as::query_as;
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
        // OperationParams wraps non-Parameters bodies as:
        // {"resourceType":"Parameters","parameter":[{"name":"resource","resource":{...}}]}
        // Unwrap if the body was auto-wrapped
        let effective_params =
            if let Some(parameter) = params.get("parameter").and_then(|v| v.as_array()) {
                if parameter.len() == 1
                    && parameter[0].get("name").and_then(|v| v.as_str()) == Some("resource")
                {
                    // Unwrap the auto-wrapped resource
                    parameter[0].get("resource").unwrap_or(params)
                } else {
                    params
                }
            } else {
                params
            };
        let params = effective_params;

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
                    None => continue,
                };

                let mut resource_type = None;
                let mut url = None;

                for part in parts {
                    let part_name = part.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    match part_name {
                        "type" => {
                            resource_type = part
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                        }
                        "url" => {
                            url = part
                                .get("valueUrl")
                                .or_else(|| part.get("valueString"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                        }
                        _ => {}
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

        Ok(inputs)
    }
}

#[async_trait]
impl OperationHandler for ImportOperation {
    fn code(&self) -> &str {
        "import"
    }

    /// System-level: POST /$import
    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if !self.config.enabled {
            return Err(OperationError::NotSupported(
                "$import is not enabled".to_string(),
            ));
        }

        let inputs = Self::parse_inputs(params)?;

        let skip_validation = params
            .get("skipValidation")
            .and_then(|v| v.as_bool())
            .unwrap_or(self.config.default_skip_validation);

        let batch_size = params
            .get("batchSize")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.config.batch_size as u64)
            .max(1) as usize;
        let parallelism = params
            .get("parallelism")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.config.max_parallel_resources as u64)
            .max(1) as usize;

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
            url: format!("{}/fhir/$import", state.base_url),
            body: Some(job_params),
            headers: None,
            client_id: None,
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

/// Upsert a resource via INSERT ... ON CONFLICT DO UPDATE.
/// Handles new resources, existing resources, and previously-deleted resources.
async fn upsert_resource_with_indexes(
    pool: &sqlx_postgres::PgPool,
    registry: &SearchParameterRegistry,
    resource_type: &str,
    id: &str,
    resource: &Value,
) -> Result<String, octofhir_storage::StorageError> {
    let table = SchemaManager::table_name(resource_type);
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(|e| {
        octofhir_storage::StorageError::transaction_error(format!(
            "Failed to begin import upsert transaction: {e}"
        ))
    })?;

    // Single query: create transaction + upsert resource with meta injection.
    // ON CONFLICT handles both existing and deleted resources by overwriting.
    let sql = format!(
        r#"WITH new_tx AS (
               INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
           )
           INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status)
           SELECT
               $1,
               new_tx.txid,
               $2,
               $2,
               jsonb_set(
                   jsonb_set(
                       jsonb_set($3::jsonb, '{{id}}', to_jsonb($1::text)),
                       '{{meta}}', '{{}}'::jsonb, true
                   ),
                   '{{meta}}',
                   jsonb_build_object(
                       'versionId', new_tx.txid::text,
                       'lastUpdated', to_char($2 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                   ),
                   true
               ),
               'created'
           FROM new_tx
           ON CONFLICT (id) DO UPDATE
           SET txid = EXCLUDED.txid,
               resource = EXCLUDED.resource,
               status = 'updated',
               updated_at = EXCLUDED.updated_at
           RETURNING id"#
    );

    let row: (String,) = query_as::<_, (String,)>(&sql)
        .bind(id)
        .bind(now)
        .bind(resource)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            octofhir_storage::StorageError::internal(format!("Failed to upsert resource: {e}"))
        })?;

    let (refs, dates) = octofhir_db_postgres::search_index::extract_search_index_rows(
        registry,
        resource_type,
        resource,
    );
    octofhir_db_postgres::search_index::write_reference_index_with_tx(
        &mut tx,
        resource_type,
        id,
        &refs,
    )
    .await?;
    octofhir_db_postgres::search_index::write_date_index_with_tx(
        &mut tx,
        resource_type,
        id,
        &dates,
    )
    .await?;

    tx.commit().await.map_err(|e| {
        octofhir_storage::StorageError::transaction_error(format!(
            "Failed to commit import upsert transaction: {e}"
        ))
    })?;

    Ok(row.0)
}

struct ImportLineOutcome {
    created: bool,
    error: Option<Value>,
}

async fn process_import_line(
    state: AppState,
    job_id: Uuid,
    resource_type: String,
    source_url: String,
    line_number: usize,
    line: String,
    skip_validation: bool,
) -> ImportLineOutcome {
    let resource: Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                job_id = %job_id,
                line = line_number,
                error = %e,
                "Invalid JSON line, skipping"
            );
            return ImportLineOutcome {
                created: false,
                error: Some(json!({
                    "source": source_url,
                    "line": line_number,
                    "error": format!("Invalid JSON: {e}"),
                })),
            };
        }
    };

    let actual_type = resource.get("resourceType").and_then(|v| v.as_str());
    if actual_type != Some(resource_type.as_str()) {
        return ImportLineOutcome {
            created: false,
            error: Some(json!({
                "source": source_url,
                "line": line_number,
                "error": format!(
                    "Expected resourceType '{}', got '{}'",
                    resource_type,
                    actual_type.unwrap_or("null")
                ),
            })),
        };
    }

    if !skip_validation {
        let validation_outcome = state.validation_service.validate(&resource).await;
        if !validation_outcome.valid {
            let diagnostics = validation_outcome
                .issues
                .iter()
                .map(|issue| issue.diagnostics.clone())
                .collect::<Vec<_>>()
                .join("; ");

            return ImportLineOutcome {
                created: false,
                error: Some(json!({
                    "source": source_url,
                    "line": line_number,
                    "error": format!("Validation failed: {diagnostics}"),
                })),
            };
        }
    }

    let id = resource.get("id").and_then(|v| v.as_str());
    let result = if let Some(id) = id {
        upsert_resource_with_indexes(
            state.db_pool.as_ref(),
            &state.search_config.config().registry,
            &resource_type,
            id,
            &resource,
        )
        .await
    } else {
        state.storage.create(&resource).await.map(|s| s.id)
    };

    match result {
        Ok(_) => ImportLineOutcome {
            created: true,
            error: None,
        },
        Err(e) => {
            tracing::warn!(
                job_id = %job_id,
                line = line_number,
                error = %e,
                "Failed to create resource"
            );
            ImportLineOutcome {
                created: false,
                error: Some(json!({
                    "source": source_url,
                    "line": line_number,
                    "error": format!("Create failed: {e}"),
                })),
            }
        }
    }
}

async fn process_import_batch(
    state: AppState,
    job_id: Uuid,
    resource_type: &str,
    source_url: &str,
    lines: Vec<(usize, String)>,
    parallelism: usize,
    skip_validation: bool,
) -> (usize, Vec<Value>) {
    let concurrency = lines.len().min(parallelism).max(1);
    let mut created = 0usize;
    let mut errors = Vec::new();

    let mut outcomes = stream::iter(lines.into_iter().map(|(line_number, line)| {
        let state = state.clone();
        let resource_type = resource_type.to_string();
        let source_url = source_url.to_string();
        async move {
            process_import_line(
                state,
                job_id,
                resource_type,
                source_url,
                line_number,
                line,
                skip_validation,
            )
            .await
        }
    }))
    .buffer_unordered(concurrency);

    while let Some(outcome) = outcomes.next().await {
        if outcome.created {
            created += 1;
        }
        if let Some(error) = outcome.error {
            errors.push(error);
        }
    }

    (created, errors)
}

/// Execute a bulk import job (called by async job executor)
pub async fn execute_bulk_import(
    state: AppState,
    job_id: Uuid,
    params: Value,
) -> Result<Value, String> {
    tracing::info!(job_id = %job_id, "Starting bulk import execution");

    let inputs = params
        .get("input")
        .and_then(|v| v.as_array())
        .ok_or("Missing input array in job params")?;

    let batch_size = params
        .get("batch_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(1000)
        .max(1) as usize;
    let parallelism = params
        .get("parallelism")
        .and_then(|v| v.as_u64())
        .unwrap_or(32)
        .max(1) as usize;

    let skip_validation = params
        .get("skip_validation")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut total_created: usize = 0;
    let mut total_errors: usize = 0;
    let mut error_details: Vec<Value> = Vec::new();

    let http_client = reqwest::Client::new();

    for (idx, input) in inputs.iter().enumerate() {
        let resource_type = input
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Missing type in input")?;
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("Missing url in input")?;

        tracing::info!(
            job_id = %job_id,
            resource_type = %resource_type,
            url = %url,
            input_index = idx,
            "Processing import source"
        );

        // Fetch NDJSON from URL
        let response = http_client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch {url}: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let msg = format!("Failed to fetch {url}: HTTP {status}");
            tracing::error!(job_id = %job_id, %msg);
            error_details.push(json!({
                "source": url,
                "error": msg,
            }));
            total_errors += 1;
            continue;
        }

        let content_length = response.content_length();
        let mut response = response;
        let mut buffered = Vec::new();
        let mut line_number = 0usize;
        let mut processed_lines = 0usize;
        let mut source_created = 0usize;
        let mut bytes_read = 0u64;
        let mut pending_batch: Vec<(usize, String)> = Vec::with_capacity(batch_size);
        let progress_start = (idx as f32 / inputs.len() as f32) * 100.0;
        let progress_end = ((idx + 1) as f32 / inputs.len() as f32) * 100.0;

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("Failed to read chunk from {url}: {e}"))?
        {
            bytes_read = bytes_read.saturating_add(chunk.len() as u64);

            buffered.extend_from_slice(&chunk);

            while let Some(newline_pos) = buffered.iter().position(|byte| *byte == b'\n') {
                let mut line_bytes: Vec<u8> = buffered.drain(..=newline_pos).collect();
                if line_bytes.last() == Some(&b'\n') {
                    line_bytes.pop();
                }
                if line_bytes.last() == Some(&b'\r') {
                    line_bytes.pop();
                }

                let line = match String::from_utf8(line_bytes) {
                    Ok(line) => line,
                    Err(e) => {
                        line_number += 1;
                        processed_lines += 1;
                        tracing::warn!(
                            job_id = %job_id,
                            line = line_number,
                            error = %e,
                            "Invalid UTF-8 line, skipping"
                        );
                        error_details.push(json!({
                            "source": url,
                            "line": line_number,
                            "error": format!("Invalid UTF-8: {e}"),
                        }));
                        total_errors += 1;
                        continue;
                    }
                };

                if line.trim().is_empty() {
                    continue;
                }

                line_number += 1;
                processed_lines += 1;
                pending_batch.push((line_number, line));

                if pending_batch.len() >= batch_size {
                    let (batch_created, mut batch_errors) = process_import_batch(
                        state.clone(),
                        job_id,
                        resource_type,
                        url,
                        std::mem::take(&mut pending_batch),
                        parallelism,
                        skip_validation,
                    )
                    .await;
                    source_created += batch_created;
                    total_errors += batch_errors.len();
                    error_details.append(&mut batch_errors);

                    let progress_pct = if let Some(total_bytes) = content_length {
                        if total_bytes > 0 {
                            progress_start
                                + ((bytes_read as f32 / total_bytes as f32)
                                    * (progress_end - progress_start))
                        } else {
                            progress_end
                        }
                    } else {
                        progress_start
                    };

                    if let Err(e) = state
                        .async_job_manager
                        .update_progress(job_id, progress_pct)
                        .await
                    {
                        tracing::warn!(
                            job_id = %job_id,
                            error = %e,
                            "Failed to update progress"
                        );
                    }
                }
            }
        }

        if !buffered.iter().all(|byte| byte.is_ascii_whitespace()) {
            line_number += 1;
            processed_lines += 1;

            if buffered.last() == Some(&b'\r') {
                buffered.pop();
            }

            let trailing_line = String::from_utf8(buffered)
                .map_err(|e| format!("Invalid UTF-8 in trailing NDJSON line from {url}: {e}"))?;

            pending_batch.push((line_number, trailing_line));
        }

        if !pending_batch.is_empty() {
            let (batch_created, mut batch_errors) = process_import_batch(
                state.clone(),
                job_id,
                resource_type,
                url,
                std::mem::take(&mut pending_batch),
                parallelism,
                skip_validation,
            )
            .await;
            source_created += batch_created;
            total_errors += batch_errors.len();
            error_details.append(&mut batch_errors);
        }

        total_created += source_created;

        if let Err(e) = state
            .async_job_manager
            .update_progress(job_id, progress_end)
            .await
        {
            tracing::warn!(
                job_id = %job_id,
                error = %e,
                "Failed to update progress"
            );
        }

        tracing::info!(
            job_id = %job_id,
            resource_type = %resource_type,
            processed_lines = processed_lines,
            created = source_created,
            "Completed streaming NDJSON import source"
        );
    }

    let result = json!({
        "total_created": total_created,
        "total_errors": total_errors,
        "errors": error_details,
    });

    tracing::info!(
        job_id = %job_id,
        total_created = total_created,
        total_errors = total_errors,
        "Bulk import completed"
    );

    Ok(result)
}
