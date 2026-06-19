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

use sqlx_core::sql_str::AssertSqlSafe;
use octofhir_search::SearchParameterRegistry;
use octofhir_search::loader::ElementTypeResolver;
use octofhir_search::parameters::{ElementTypeHint, SearchParameterType};
use octofhir_search::sql_builder::{
    AnnotatedPath, build_jsonb_accessor, build_typed_extract_fn, date_lower_paths, date_upper_paths,
    extraction_paths, fhirpath_to_jsonb_path, paths_to_json,
};
use sqlx_postgres::PgPool;
use tracing::{debug, info, warn};

/// Normalizes a scalar-or-array JSONB value to an array (null -> `[]`). Used by the
/// per-param typed extraction functions so a single element and an array of elements
/// are handled uniformly.
const FHIR_ARR_DDL: &str = "CREATE OR REPLACE FUNCTION fhir_arr(v jsonb) RETURNS jsonb \
     LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$ \
       SELECT CASE WHEN v IS NULL THEN '[]'::jsonb \
         WHEN jsonb_typeof(v)='array' THEN v ELSE jsonb_build_array(v) END; \
     $$;";

/// Create functional search indexes for the configured parameters (each
/// `"ResourceType.code"`). Idempotent (`IF NOT EXISTS`); a missing table or
/// unknown parameter is skipped, not fatal. No runtime/lazy creation.
///
/// For "indexed" STRING params a per-param TYPE-AWARE flat extraction SQL function
/// is generated (via `resolver` for element cardinality) and used in BOTH the
/// functional GIN index and the query predicate (recorded on the registry's
/// `SearchParameter.typed_extract_fn`). On any failure it falls back to the generic
/// `fhir_extract_text` extraction.
pub async fn create_default_search_indexes(
    pool: &PgPool,
    registry: &SearchParameterRegistry,
    params: &[String],
    resolver: &dyn ElementTypeResolver,
) -> usize {
    // Ensure the array-normalization helper exists before any typed function uses it.
    if let Err(e) = sqlx_core::raw_sql::raw_sql(AssertSqlSafe((FHIR_ARR_DDL).to_string())).execute(pool).await {
        warn!(error = %e, "failed to create fhir_arr helper; typed string extraction disabled");
    }

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
        let table = resource_type.to_lowercase();

        // STRING params get the typed-extraction fast path (built ahead of the DDL
        // match so we can record the function name on the registry on success).
        if param.param_type == SearchParameterType::String {
            let annotated =
                build_annotated_paths_for_param(&param, resource_type, resolver).await;
            if let Some((fn_name, fn_ddl, _)) =
                build_typed_extract_fn(resource_type, code, &annotated)
            {
                match sqlx_core::raw_sql::raw_sql(AssertSqlSafe((&fn_ddl).to_string())).execute(pool).await {
                    Ok(_) => {
                        let index_ddl = format!(
                            "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_str\" ON \"{table}\" \
                             USING gin (fhir_text_blob({fn_name}(resource)) gin_trgm_ops)"
                        );
                        match sqlx_core::raw_sql::raw_sql(AssertSqlSafe((&index_ddl).to_string())).execute(pool).await {
                            Ok(_) => {
                                registry.upsert(
                                    (*param).clone().with_typed_extract_fn(fn_name),
                                );
                                created += 1;
                                debug!(
                                    resource_type,
                                    code, "created typed functional string index"
                                );
                                continue;
                            }
                            Err(e) => {
                                warn!(
                                    resource_type,
                                    code,
                                    error = %e,
                                    "skipped typed functional string index"
                                );
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        // Function creation failed: fall back to the generic index.
                        warn!(
                            resource_type,
                            code,
                            error = %e,
                            "typed extraction function failed; using generic string index"
                        );
                    }
                }
            }
        }

        let ddl = match param.param_type {
            SearchParameterType::Date => {
                // Cheap min/max hull GiST index — the indexable prefilter the
                // in-place date predicate ANDs with an exact per-occurrence
                // multirange recheck. The hull is far cheaper to maintain on write
                // than the multirange (≈2.25x), and serves the same `&&`/`<@`/
                // `>>`/`<<` prefilter the predicate derives identically.
                let lower_json = paths_to_json(&date_lower_paths(&segments));
                let upper_json = paths_to_json(&date_upper_paths(&segments));
                format!(
                    "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_date\" ON \"{table}\" \
                     USING gist (tstzrange(\
                       fhir_extract_date_min(resource, '{lower_json}'::jsonb), \
                       fhir_extract_date_max(resource, '{upper_json}'::jsonb), '[]'))"
                )
            }
            SearchParameterType::String => {
                let paths_json = paths_to_json(&extraction_paths(&segments, &param.element_type_hint));
                format!(
                    "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_str\" ON \"{table}\" \
                     USING gin (fhir_text_blob(fhir_extract_text(resource, '{paths_json}'::jsonb)) gin_trgm_ops)"
                )
            }
            // Repeating CodeableConcept/Coding token params (e.g. Observation.category):
            // the in-place predicate is `<subtree> @> '[...]'` (subtree containment).
            // A dedicated GIN on just that subtree is small and selective, so the
            // planner uses it — the whole-resource GIN is estimated too non-selective
            // and gets skipped for a Seq Scan under LIMIT (catastrophic at scale).
            SearchParameterType::Token
                if matches!(
                    &param.element_type_hint,
                    ElementTypeHint::Array(t) if t == "CodeableConcept" || t == "Coding"
                ) =>
            {
                let subtree = build_jsonb_accessor("resource", &segments, false);
                format!(
                    "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_token\" ON \"{table}\" \
                     USING gin (({subtree}) jsonb_path_ops)"
                )
            }
            // Other token/quantity/reference predicates are not yet index-matched
            // in-place; their indexes are added with those predicate rewrites.
            _ => continue,
        };

        match sqlx_core::raw_sql::raw_sql(AssertSqlSafe((&ddl).to_string())).execute(pool).await {
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

/// Annotate each generic extraction path with per-segment array cardinality by
/// resolving every prefix (`path[..=i]`) against the element-type resolver. Segments
/// that don't resolve fall back to scalar (`false`).
async fn annotate_path(
    path: &[String],
    resource_type: &str,
    resolver: &dyn ElementTypeResolver,
) -> AnnotatedPath {
    let mut out = AnnotatedPath::with_capacity(path.len());
    for i in 0..path.len() {
        let prefix = path[..=i].join(".");
        let is_array = resolver
            .resolve(resource_type, &prefix)
            .await
            .map(|(_, is_array)| is_array)
            .unwrap_or(false);
        out.push((path[i].clone(), is_array));
    }
    out
}

/// Build the annotated extraction paths for a STRING param: the same generic paths
/// (`extraction_paths` over the FHIRPath-derived segments), each annotated with
/// per-segment array cardinality.
async fn build_annotated_paths_for_param(
    param: &octofhir_search::parameters::SearchParameter,
    resource_type: &str,
    resolver: &dyn ElementTypeResolver,
) -> Vec<AnnotatedPath> {
    let Some(expression) = param.expression.as_deref() else {
        return Vec::new();
    };
    let segments = fhirpath_to_jsonb_path(expression, resource_type);
    let paths = extraction_paths(&segments, &param.element_type_hint);
    let mut annotated = Vec::with_capacity(paths.len());
    for path in &paths {
        annotated.push(annotate_path(path, resource_type, resolver).await);
    }
    annotated
}
