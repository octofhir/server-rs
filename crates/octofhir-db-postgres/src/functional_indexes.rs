//! Functional search indexes on the resource JSONB.
//!
//! Created once at bootstrap (after resource tables + the SearchParameter
//! registry are ready) for the configured `search.indexed_params`. Each index
//! matches the in-place predicate the search builder emits for that parameter,
//! so the planner uses it. No runtime/lazy creation.
//!
//! Plain `CREATE INDEX IF NOT EXISTS` (not CONCURRENTLY) is correct here: the
//! tables are empty and not yet serving traffic at bootstrap, so the brief
//! ACCESS EXCLUSIVE lock is free. A future runtime "add index on a live table"
//! path (DB console / suggest-index) should use CONCURRENTLY instead.

use octofhir_search::SearchParameterRegistry;
use octofhir_search::parameters::SearchParameterType;
use octofhir_search::sql_builder::{extraction_paths, fhirpath_to_jsonb_path, paths_to_json};
use sqlx_postgres::PgPool;
use tracing::{debug, info, warn};

/// Create functional search indexes for the configured parameters (each
/// `"ResourceType.code"`). Idempotent (`IF NOT EXISTS`); a missing table or
/// unknown parameter is skipped, not fatal. No runtime/lazy creation.
pub async fn create_default_search_indexes(
    pool: &PgPool,
    registry: &SearchParameterRegistry,
    params: &[String],
) -> usize {
    let mut created = 0usize;
    for spec in params {
        let Some((resource_type, code)) = spec.split_once('.') else {
            warn!(spec, "ignoring malformed indexed_params entry (want ResourceType.code)");
            continue;
        };
        let Some(param) = registry.get(resource_type, code) else {
            continue;
        };
        let Some(expression) = param.expression.as_deref() else {
            continue;
        };
        let segments = fhirpath_to_jsonb_path(expression, resource_type);
        let paths_json = paths_to_json(&extraction_paths(&segments, &param.element_type_hint));
        let table = resource_type.to_lowercase();

        let ddl = match param.param_type {
            SearchParameterType::Date => format!(
                "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_date\" ON \"{table}\" \
                 USING gist (tstzrange(\
                   fhir_extract_date_min(resource, '{paths_json}'::jsonb), \
                   fhir_extract_date_max(resource, '{paths_json}'::jsonb), '[]'))"
            ),
            SearchParameterType::String => format!(
                "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_str\" ON \"{table}\" \
                 USING gin (fhir_text_blob(fhir_extract_text(resource, '{paths_json}'::jsonb)) gin_trgm_ops)"
            ),
            // Token/Quantity/Reference predicates are not yet index-matched in-place;
            // their indexes are added with those predicate rewrites.
            _ => continue,
        };

        match sqlx_core::raw_sql::raw_sql(&ddl).execute(pool).await {
            Ok(_) => {
                created += 1;
                debug!(resource_type, code, "created functional search index");
            }
            Err(e) => {
                // Missing table (resource type not bootstrapped) or transient error.
                warn!(resource_type, code, error = %e, "skipped functional search index");
            }
        }
    }
    info!(created, "functional search indexes ensured");
    created
}
