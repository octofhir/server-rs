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
use octofhir_db_postgres::SchemaManager;
use octofhir_search::SearchParameterType;
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
            .unwrap_or(self.config.batch_size as u64) as usize;

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
async fn upsert_resource(
    pool: &sqlx_postgres::PgPool,
    resource_type: &str,
    id: &str,
    resource: &Value,
) -> Result<String, octofhir_storage::StorageError> {
    let table = SchemaManager::table_name(resource_type);
    let now = Utc::now();

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
        .fetch_one(pool)
        .await
        .map_err(|e| {
            octofhir_storage::StorageError::internal(format!("Failed to upsert resource: {e}"))
        })?;

    Ok(row.0)
}

/// Write search indexes for an imported resource.
async fn write_search_indexes(state: &AppState, resource_type: &str, id: &str, resource: &Value) {
    let registry = state.search_config.config().registry.clone();
    let params = registry.get_all_for_type(resource_type);
    let pool = state.db_pool.as_ref();

    let mut refs = Vec::new();
    let mut dates = Vec::new();

    for param in &params {
        let expression = match &param.expression {
            Some(e) => e.as_str(),
            None => continue,
        };
        match param.param_type {
            SearchParameterType::Reference => {
                refs.extend(octofhir_core::search_index::extract_references(
                    resource,
                    resource_type,
                    &param.code,
                    expression,
                    None,
                ));
            }
            SearchParameterType::Date => {
                dates.extend(octofhir_core::search_index::extract_dates(
                    resource,
                    resource_type,
                    &param.code,
                    expression,
                ));
            }
            _ => {}
        }
    }

    if let Err(e) =
        octofhir_db_postgres::search_index::write_reference_index(pool, resource_type, id, &refs)
            .await
    {
        tracing::warn!(error = %e, resource_type, id, "Import: reference index write failed");
    }

    if let Err(e) =
        octofhir_db_postgres::search_index::write_date_index(pool, resource_type, id, &dates).await
    {
        tracing::warn!(error = %e, resource_type, id, "Import: date index write failed");
    }
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
        .unwrap_or(1000) as usize;

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

        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read body from {url}: {e}"))?;

        // Parse and import NDJSON lines in batches
        let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
        let total_lines = lines.len();

        tracing::info!(
            job_id = %job_id,
            resource_type = %resource_type,
            lines = total_lines,
            "Parsed NDJSON lines"
        );

        for (batch_idx, chunk) in lines.chunks(batch_size).enumerate() {
            let mut batch_created = 0;

            for (line_idx, line) in chunk.iter().enumerate() {
                let resource: Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(e) => {
                        let global_line = batch_idx * batch_size + line_idx + 1;
                        tracing::warn!(
                            job_id = %job_id,
                            line = global_line,
                            error = %e,
                            "Invalid JSON line, skipping"
                        );
                        error_details.push(json!({
                            "source": url,
                            "line": global_line,
                            "error": format!("Invalid JSON: {e}"),
                        }));
                        total_errors += 1;
                        continue;
                    }
                };

                // Validate resourceType matches expected type
                let actual_type = resource.get("resourceType").and_then(|v| v.as_str());
                if actual_type != Some(resource_type) {
                    let global_line = batch_idx * batch_size + line_idx + 1;
                    error_details.push(json!({
                        "source": url,
                        "line": global_line,
                        "error": format!(
                            "Expected resourceType '{}', got '{}'",
                            resource_type,
                            actual_type.unwrap_or("null")
                        ),
                    }));
                    total_errors += 1;
                    continue;
                }

                // Upsert: INSERT ... ON CONFLICT to handle new, existing, and deleted resources.
                let id = resource.get("id").and_then(|v| v.as_str());
                let result = if let Some(id) = id {
                    upsert_resource(state.db_pool.as_ref(), resource_type, id, &resource).await
                } else {
                    state.storage.create(&resource).await.map(|s| s.id)
                };
                match result {
                    Ok(stored_id) => {
                        batch_created += 1;
                        write_search_indexes(&state, resource_type, &stored_id, &resource).await;
                    }
                    Err(e) => {
                        let global_line = batch_idx * batch_size + line_idx + 1;
                        tracing::warn!(
                            job_id = %job_id,
                            line = global_line,
                            error = %e,
                            "Failed to create resource"
                        );
                        error_details.push(json!({
                            "source": url,
                            "line": global_line,
                            "error": format!("Create failed: {e}"),
                        }));
                        total_errors += 1;
                    }
                }
            }

            total_created += batch_created;

            // Update progress (as percentage)
            let processed = std::cmp::min((batch_idx + 1) * batch_size, total_lines);
            let progress_pct = if total_lines > 0 {
                (processed as f32 / total_lines as f32) * 100.0
            } else {
                100.0
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

    let _ = skip_validation; // TODO: wire through to storage layer

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
