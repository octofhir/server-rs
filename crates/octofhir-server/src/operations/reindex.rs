//! Reindex operation handler ($reindex)
//!
//! Rebuilds `search_idx_reference` and `search_idx_date` rows from stored resources.
//!
//! - Instance: `POST /{type}/{id}/$reindex` — synchronous, immediate
//! - Type: `POST /{type}/$reindex` — async job
//! - System: `POST /$reindex` — async job

use async_trait::async_trait;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::async_jobs::AsyncJobRequest;
use crate::config::ReindexConfig;
use crate::operations::handler::{OperationError, OperationHandler};
use crate::server::AppState;

use octofhir_search::{SearchParameterRegistry, SearchParameterType};
use sqlx_postgres::PgPool;

/// The $reindex operation handler
pub struct ReindexOperation {
    config: ReindexConfig,
}

impl ReindexOperation {
    pub fn new(config: ReindexConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl OperationHandler for ReindexOperation {
    fn code(&self) -> &str {
        "reindex"
    }

    /// System-level: POST /$reindex — reindex all resource types (async)
    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let batch_size = params
            .get("batchSize")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.config.batch_size as u64) as usize;

        let consistency_check = params
            .get("consistencyCheck")
            .and_then(|v| v.as_bool())
            .unwrap_or(self.config.consistency_check);

        let job_params = json!({
            "scope": "system",
            "batch_size": batch_size,
            "consistency_check": consistency_check,
        });

        let async_request = AsyncJobRequest {
            request_type: "reindex".to_string(),
            method: "POST".to_string(),
            url: format!("{}/fhir/$reindex", state.base_url),
            body: Some(job_params),
            headers: None,
            client_id: None,
        };

        let job_id = state
            .async_job_manager
            .submit_job(async_request)
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to submit reindex job: {e}")))?;

        tracing::info!(job_id = %job_id, "System-level reindex job submitted");

        Ok(json!({
            "status": "accepted",
            "job_id": job_id.to_string(),
            "status_url": format!("{}/_async-status/{}", state.base_url, job_id),
        }))
    }

    /// Type-level: POST /{type}/$reindex — reindex all resources of a type (async)
    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let batch_size = params
            .get("batchSize")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.config.batch_size as u64) as usize;

        let consistency_check = params
            .get("consistencyCheck")
            .and_then(|v| v.as_bool())
            .unwrap_or(self.config.consistency_check);

        let job_params = json!({
            "scope": "type",
            "resource_type": resource_type,
            "batch_size": batch_size,
            "consistency_check": consistency_check,
        });

        let async_request = AsyncJobRequest {
            request_type: "reindex".to_string(),
            method: "POST".to_string(),
            url: format!("{}/fhir/{}/$reindex", state.base_url, resource_type),
            body: Some(job_params),
            headers: None,
            client_id: None,
        };

        let job_id = state
            .async_job_manager
            .submit_job(async_request)
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to submit reindex job: {e}")))?;

        tracing::info!(
            job_id = %job_id,
            resource_type = %resource_type,
            "Type-level reindex job submitted"
        );

        Ok(json!({
            "status": "accepted",
            "job_id": job_id.to_string(),
            "status_url": format!("{}/_async-status/{}", state.base_url, job_id),
        }))
    }

    /// Instance-level: POST /{type}/{id}/$reindex — synchronous, immediate
    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        let stored = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(|e| OperationError::Internal(format!("Storage error: {e}")))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{resource_type}/{id} not found"))
            })?;

        let pool = state.db_pool.as_ref();
        let registry = state.search_config.config().registry.clone();

        reindex_single_resource(pool, &registry, resource_type, id, &stored.resource).await?;

        tracing::debug!(resource_type, id, "Instance reindex completed");

        Ok(json!({
            "resourceType": "OperationOutcome",
            "issue": [{
                "severity": "information",
                "code": "informational",
                "diagnostics": format!("Reindexed {resource_type}/{id}")
            }]
        }))
    }
}

/// Reindex a single resource using direct pool access.
async fn reindex_single_resource(
    pool: &PgPool,
    registry: &SearchParameterRegistry,
    resource_type: &str,
    resource_id: &str,
    resource: &Value,
) -> Result<(), OperationError> {
    let params = registry.get_all_for_type(resource_type);

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

    octofhir_db_postgres::search_index::write_reference_index(
        pool,
        resource_type,
        resource_id,
        &refs,
    )
    .await
    .map_err(|e| OperationError::Internal(format!("Failed to write reference index: {e}")))?;

    octofhir_db_postgres::search_index::write_date_index(
        pool,
        resource_type,
        resource_id,
        &dates,
    )
    .await
    .map_err(|e| OperationError::Internal(format!("Failed to write date index: {e}")))?;

    Ok(())
}

// ============================================================================
// Async Executor
// ============================================================================

/// Execute a reindex job (called by the async job executor).
pub async fn execute_reindex(
    state: AppState,
    job_id: Uuid,
    params: Value,
) -> Result<Value, String> {
    tracing::info!(job_id = %job_id, "Starting reindex execution");

    let scope = params
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("system");

    let batch_size = params
        .get("batch_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(1000) as usize;

    let consistency_check = params
        .get("consistency_check")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let pool = state.db_pool.as_ref();
    let registry = state.search_config.config().registry.clone();

    let resource_types = match scope {
        "type" => {
            let rt = params
                .get("resource_type")
                .and_then(|v| v.as_str())
                .ok_or("Missing resource_type for type-level reindex")?;
            vec![rt.to_string()]
        }
        _ => list_resource_tables(pool).await?,
    };

    let total_types = resource_types.len();
    let mut total_reindexed: usize = 0;
    let mut total_errors: usize = 0;

    for (type_idx, resource_type) in resource_types.iter().enumerate() {
        tracing::info!(
            job_id = %job_id,
            resource_type = %resource_type,
            "Reindexing resource type ({}/{})",
            type_idx + 1,
            total_types
        );

        match reindex_resource_type(pool, &registry, resource_type, batch_size, &state, job_id, type_idx, total_types).await {
            Ok((reindexed, errors)) => {
                total_reindexed += reindexed;
                total_errors += errors;
            }
            Err(e) => {
                tracing::warn!(
                    job_id = %job_id,
                    resource_type = %resource_type,
                    error = %e,
                    "Failed to reindex resource type"
                );
            }
        }
    }

    // Optional consistency check
    let drift = if consistency_check {
        check_index_consistency(pool, &resource_types).await.unwrap_or(0)
    } else {
        0
    };

    tracing::info!(
        job_id = %job_id,
        total_reindexed,
        total_errors,
        drift,
        "Reindex completed"
    );

    Ok(json!({
        "resourceType": "Parameters",
        "parameter": [
            { "name": "totalReindexed", "valueInteger": total_reindexed },
            { "name": "totalErrors", "valueInteger": total_errors },
            { "name": "driftDetected", "valueInteger": drift },
        ]
    }))
}

/// Reindex all resources of a given type in batches.
async fn reindex_resource_type(
    pool: &PgPool,
    registry: &SearchParameterRegistry,
    resource_type: &str,
    batch_size: usize,
    state: &AppState,
    job_id: Uuid,
    type_idx: usize,
    total_types: usize,
) -> Result<(usize, usize), String> {
    let mut offset: i64 = 0;
    let mut reindexed = 0;
    let mut errors = 0;

    // Get total count for progress tracking
    let total_count = get_resource_count(pool, resource_type).await.unwrap_or(0);

    loop {
        // Fetch batch of resources
        let batch = fetch_resource_batch(pool, resource_type, batch_size, offset).await?;
        if batch.is_empty() {
            break;
        }

        let batch_len = batch.len();

        // Batch delete old indexes
        let ids: Vec<&str> = batch.iter().map(|(id, _)| id.as_str()).collect();
        if let Err(e) = batch_delete_indexes(pool, resource_type, &ids).await {
            tracing::warn!(error = %e, "Failed to batch-delete indexes for {resource_type}");
            errors += batch_len;
            offset += batch_len as i64;
            continue;
        }

        // Extract and write new indexes for each resource
        for (id, resource) in &batch {
            let params = registry.get_all_for_type(resource_type);
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

            // Write indexes (uses per-resource delete+insert internally,
            // but we already deleted above, so we use write functions that
            // also delete first — the delete is idempotent)
            if let Err(e) = octofhir_db_postgres::search_index::write_reference_index(
                pool,
                resource_type,
                id,
                &refs,
            )
            .await
            {
                tracing::warn!(error = %e, resource_type, id, "Reference index write failed");
                errors += 1;
                continue;
            }

            if let Err(e) = octofhir_db_postgres::search_index::write_date_index(
                pool,
                resource_type,
                id,
                &dates,
            )
            .await
            {
                tracing::warn!(error = %e, resource_type, id, "Date index write failed");
                errors += 1;
                continue;
            }

            reindexed += 1;
        }

        offset += batch_len as i64;

        // Update progress
        let type_progress = if total_count > 0 {
            offset as f32 / total_count as f32
        } else {
            1.0
        };
        let overall_progress =
            (type_idx as f32 + type_progress) / total_types as f32;
        state
            .async_job_manager
            .update_progress(job_id, overall_progress)
            .await
            .ok();

        if batch_len < batch_size {
            break;
        }
    }

    Ok((reindexed, errors))
}

// ============================================================================
// Database helpers
// ============================================================================

/// List all resource tables in the database.
async fn list_resource_tables(pool: &PgPool) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx_core::query_as::query_as(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' \
         AND table_name NOT LIKE '%_history' \
         AND table_name NOT LIKE '\\_%' ESCAPE '\\' \
         AND table_name NOT IN ('async_jobs', 'search_idx_reference', 'search_idx_date') \
         ORDER BY table_name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to list resource tables: {e}"))?;

    // Convert snake_case table names to PascalCase resource types
    Ok(rows
        .into_iter()
        .map(|(name,)| table_name_to_resource_type(&name))
        .collect())
}

/// Convert a snake_case table name to a PascalCase resource type.
fn table_name_to_resource_type(table_name: &str) -> String {
    table_name
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Get total resource count for a table.
async fn get_resource_count(pool: &PgPool, resource_type: &str) -> Result<i64, String> {
    let table = resource_type.to_lowercase();
    let query_str = format!(
        "SELECT count(*) FROM \"{}\" WHERE status != 'deleted'",
        table
    );
    let row: (i64,) = sqlx_core::query_as::query_as(&query_str)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to count {resource_type}: {e}"))?;
    Ok(row.0)
}

/// Fetch a batch of resources from a table.
async fn fetch_resource_batch(
    pool: &PgPool,
    resource_type: &str,
    limit: usize,
    offset: i64,
) -> Result<Vec<(String, Value)>, String> {
    let table = resource_type.to_lowercase();
    let query_str = format!(
        "SELECT id, resource FROM \"{}\" WHERE status != 'deleted' ORDER BY id LIMIT $1 OFFSET $2",
        table
    );
    let rows: Vec<(String, Value)> = sqlx_core::query_as::query_as(&query_str)
        .bind(limit as i64)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch {resource_type} batch: {e}"))?;
    Ok(rows)
}

/// Batch delete index rows for multiple resources.
async fn batch_delete_indexes(
    pool: &PgPool,
    resource_type: &str,
    ids: &[&str],
) -> Result<(), String> {
    let id_strings: Vec<String> = ids.iter().map(|s| s.to_string()).collect();

    sqlx_core::query::query(
        "DELETE FROM search_idx_reference WHERE resource_type = $1 AND resource_id = ANY($2)",
    )
    .bind(resource_type)
    .bind(&id_strings)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to batch-delete reference indexes: {e}"))?;

    sqlx_core::query::query(
        "DELETE FROM search_idx_date WHERE resource_type = $1 AND resource_id = ANY($2)",
    )
    .bind(resource_type)
    .bind(&id_strings)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to batch-delete date indexes: {e}"))?;

    Ok(())
}

/// Check index consistency: count resources with missing index rows.
async fn check_index_consistency(
    pool: &PgPool,
    resource_types: &[String],
) -> Result<usize, String> {
    let mut drift_count = 0;

    for rt in resource_types {
        let table = rt.to_lowercase();

        // Check missing reference index rows for resources that have a "subject" field
        let ref_query = format!(
            "SELECT count(*) FROM \"{table}\" r \
             WHERE r.status != 'deleted' \
             AND r.resource ? 'subject' \
             AND (r.resource->'subject' ? 'reference') \
             AND NOT EXISTS ( \
                 SELECT 1 FROM search_idx_reference sir \
                 WHERE sir.resource_type = $1 AND sir.resource_id = r.id \
             )"
        );
        if let Ok(row) = sqlx_core::query_as::query_as::<_, (i64,)>(&ref_query)
            .bind(rt.as_str())
            .fetch_one(pool)
            .await
        {
            drift_count += row.0 as usize;
        }
    }

    Ok(drift_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_name_to_resource_type() {
        assert_eq!(table_name_to_resource_type("patient"), "Patient");
        assert_eq!(table_name_to_resource_type("observation"), "Observation");
        assert_eq!(
            table_name_to_resource_type("allergy_intolerance"),
            "AllergyIntolerance"
        );
        assert_eq!(
            table_name_to_resource_type("medication_request"),
            "MedicationRequest"
        );
        assert_eq!(
            table_name_to_resource_type("care_plan"),
            "CarePlan"
        );
    }
}
