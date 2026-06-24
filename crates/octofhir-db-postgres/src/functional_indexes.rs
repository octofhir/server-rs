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
use octofhir_search::loader::ElementTypeResolver;
use octofhir_search::parameters::{ElementTypeHint, SearchParameterType};
use octofhir_search::sql_builder::{
    AnnotatedPath, build_jsonb_accessor, build_typed_extract_fn, date_lower_paths,
    date_upper_paths, extraction_paths, fhirpath_to_jsonb_path, paths_to_json,
    paths_to_jsonpath_array,
};
use sqlx_core::sql_str::AssertSqlSafe;
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

/// Numeric min/max hull, as a single `numrange`, over every `.value` scalar a quantity
/// jsonpath matches (e.g. all `component[*].valueQuantity.value`). The quantity-array
/// predicate overlaps this against the query range (`hull && q`) and ONE GiST index
/// backs it — half the write cost of separate min/max btrees (one `jsonb_path_query_array`
/// pass, one index). Returns NULL when there are no values, so a row with no matching
/// quantity is excluded (`NULL && q` is NULL/false) instead of matching an unbounded range.
/// IMMUTABLE so it is index-expression usable.
const FHIR_QTY_HULL_RANGE_DDL: &str = "CREATE OR REPLACE FUNCTION fhir_qty_hull_range(res jsonb, jp jsonpath) RETURNS numrange \
     LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$ \
       SELECT CASE WHEN min(x::numeric) IS NULL THEN NULL \
                   ELSE numrange(min(x::numeric), max(x::numeric), '[]') END \
       FROM jsonb_array_elements_text(jsonb_path_query_array(res, jp)) AS x; \
     $$;";

/// Global numeric max (and min, below) of every `.value` scalar matched by ANY of
/// `jps` — folding a quantity param's top-level and `component[*]` value locations
/// into ONE scalar. Backs a single btree that the search predicate hits as
/// `fhir_qty_extract_max_numeric(resource, <paths>) > N` (exact for `some value > N`),
/// replacing the old per-location OR of a scalar btree and a component hull the
/// planner could not combine. Returns NULL when no value matches (row excluded).
const FHIR_QTY_EXTRACT_MAX_DDL: &str = "CREATE OR REPLACE FUNCTION fhir_qty_extract_max_numeric(res jsonb, jps jsonpath[]) RETURNS numeric \
     LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$ \
       SELECT max((x)::numeric) \
       FROM unnest(jps) AS jp \
       CROSS JOIN LATERAL jsonb_array_elements_text(jsonb_path_query_array(res, jp)) AS x; \
     $$;";

const FHIR_QTY_EXTRACT_MIN_DDL: &str = "CREATE OR REPLACE FUNCTION fhir_qty_extract_min_numeric(res jsonb, jps jsonpath[]) RETURNS numeric \
     LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$ \
       SELECT min((x)::numeric) \
       FROM unnest(jps) AS jp \
       CROSS JOIN LATERAL jsonb_array_elements_text(jsonb_path_query_array(res, jp)) AS x; \
     $$;";

/// Canonicalizes a single literal reference for indexing/matching: strips any
/// `/_history/<version>` suffix so a versioned reference (`Patient/1/_history/2`)
/// indexes and matches as its current-version form (`Patient/1`), per the FHIR
/// rule that search operates against the current version. Deliberately BASE-FREE:
/// the server base URL is NOT folded in here, so changing `base_url` or migrating
/// the database never invalidates the index (relative refs are base-independent;
/// absolute refs are kept verbatim). Relative/absolute equivalence is handled at
/// query time against the *current* base, not baked into stored index entries.
const FHIR_NORM_REF_DDL: &str = "CREATE OR REPLACE FUNCTION public.fhir_norm_ref(s text) RETURNS text \
     LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$ \
       SELECT regexp_replace(s, '/_history/.*$', ''); \
     $$;";

/// Reference flattener: like the `fhir_extract_text(jsonb, jsonpath[])` overload but
/// canonicalizes each matched `.reference` string via `fhir_norm_ref`. Used in BOTH
/// the reference functional GIN index and the query predicate, so the expressions are
/// identical and the planner uses the index. Base-free (see `fhir_norm_ref`).
const FHIR_EXTRACT_REF_DDL: &str = "CREATE OR REPLACE FUNCTION public.fhir_extract_ref(resource jsonb, paths jsonpath[]) \
     RETURNS text[] LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT AS $$ \
       SELECT nullif(array_agg(public.fhir_norm_ref(value #>> '{}')), '{}') \
       FROM unnest(paths) AS p \
       CROSS JOIN LATERAL jsonb_path_query(resource, p) AS value; \
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
    if let Err(e) = sqlx_core::raw_sql::raw_sql(AssertSqlSafe((FHIR_ARR_DDL).to_string()))
        .execute(pool)
        .await
    {
        warn!(error = %e, "failed to create fhir_arr helper; typed string extraction disabled");
    }
    // Quantity hull helper — required by the quantity-array predicate at query time,
    // so create it unconditionally (not only when a quantity param is indexed).
    if let Err(e) = sqlx_core::raw_sql::raw_sql(AssertSqlSafe(FHIR_QTY_HULL_RANGE_DDL.to_string()))
        .execute(pool)
        .await
    {
        warn!(error = %e, "failed to create quantity hull helper");
    }
    // Quantity union min/max extractors — back the single-btree quantity predicate and
    // its functional indexes; used at query time even for non-indexed quantity params.
    for ddl in [FHIR_QTY_EXTRACT_MAX_DDL, FHIR_QTY_EXTRACT_MIN_DDL] {
        if let Err(e) = sqlx_core::raw_sql::raw_sql(AssertSqlSafe(ddl.to_string()))
            .execute(pool)
            .await
        {
            warn!(error = %e, "failed to create quantity extract helper");
        }
    }
    // Reference canonicalization helpers — used by BOTH the reference functional index
    // and the query predicate, so create them unconditionally (a query may target a
    // non-indexed reference param and still call fhir_extract_ref). fhir_norm_ref must
    // exist before fhir_extract_ref references it.
    if let Err(e) = sqlx_core::raw_sql::raw_sql(AssertSqlSafe(FHIR_NORM_REF_DDL.to_string()))
        .execute(pool)
        .await
    {
        warn!(error = %e, "failed to create fhir_norm_ref helper");
    }
    if let Err(e) = sqlx_core::raw_sql::raw_sql(AssertSqlSafe(FHIR_EXTRACT_REF_DDL.to_string()))
        .execute(pool)
        .await
    {
        warn!(error = %e, "failed to create fhir_extract_ref helper");
    }

    let mut created = 0usize;
    // Dedup identical index bodies across params. Overlapping params build the same
    // functional index on the same expression (e.g. value-quantity, combo-value-quantity
    // and component-value-quantity all derive a `valueQuantity.value` btree and a shared
    // `component[*].valueQuantity.value` hull). The planner matches by EXPRESSION not name,
    // so one physical index serves all of them — building the duplicates only taxes writes.
    // Key = the `ON "table" ...` tail (everything after the index name).
    let mut seen_bodies: std::collections::HashSet<String> = std::collections::HashSet::new();
    let index_body =
        |ddl: &str| -> Option<String> { ddl.rfind(" ON ").map(|i| ddl[i..].to_string()) };
    for spec in params {
        let Some((resource_type, code)) = spec.split_once('.') else {
            warn!(
                spec,
                "ignoring malformed indexed_params entry (want ResourceType.code)"
            );
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
            let annotated = build_annotated_paths_for_param(&param, resource_type, resolver).await;
            if let Some((fn_name, fn_ddl, _)) =
                build_typed_extract_fn(resource_type, code, &annotated)
            {
                match sqlx_core::raw_sql::raw_sql(AssertSqlSafe((&fn_ddl).to_string()))
                    .execute(pool)
                    .await
                {
                    Ok(_) => {
                        let index_ddl = format!(
                            "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_str\" ON \"{table}\" \
                             USING gin (fhir_text_blob({fn_name}(resource)) gin_trgm_ops)"
                        );
                        match sqlx_core::raw_sql::raw_sql(AssertSqlSafe((&index_ddl).to_string()))
                            .execute(pool)
                            .await
                        {
                            Ok(_) => {
                                registry.upsert((*param).clone().with_typed_extract_fn(fn_name));
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
                // Cheap min/max hull GiST index — the indexable prefilter the in-place
                // date predicate ANDs with an exact per-occurrence multirange recheck.
                // The hull is far cheaper to maintain on write than the multirange and,
                // crucially, far cheaper to SCAN under load (a tstzrange GiST is smaller
                // and more selective than a tstzmultirange one); a multirange index was
                // measured slower end-to-end. Precompiled jsonpath[] (baked literals);
                // the predicate (types/date.rs) builds the identical expression so the
                // planner matches this functional GiST index.
                let lower_jpa = paths_to_jsonpath_array(&date_lower_paths(&segments));
                let upper_jpa = paths_to_jsonpath_array(&date_upper_paths(&segments));
                format!(
                    "DROP INDEX IF EXISTS \"idx_{table}_{code}_date\"; \
                     CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_date\" ON \"{table}\" \
                     USING gist (tstzrange(\
                       fhir_extract_date_min(resource, {lower_jpa}), \
                       fhir_extract_date_max(resource, {upper_jpa}), '[]'))"
                )
            }
            SearchParameterType::String => {
                let paths_json =
                    paths_to_json(&extraction_paths(&segments, &param.element_type_hint));
                format!(
                    "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_str\" ON \"{table}\" \
                     USING gin (fhir_text_blob(fhir_extract_text(resource, '{paths_json}'::jsonb)) gin_trgm_ops)"
                )
            }
            // CodeableConcept/Coding token params — repeating (e.g. Observation.category,
            // predicate `<subtree> @> '[...]'`) or scalar (e.g. Encounter.class, predicate
            // `<subtree> @> '{...}'`). A dedicated GIN on just that subtree is small and
            // selective, so the planner uses it — the whole-resource GIN is estimated too
            // non-selective and gets skipped for a Seq Scan under LIMIT (catastrophic at scale).
            SearchParameterType::Token
                if matches!(&param.element_type_hint, ElementTypeHint::Token)
                    || matches!(
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
            // Reference params (e.g. Observation.subject/encounter/performer). The
            // in-place predicate (types/mod.rs) flattens <segments>.reference into a
            // text[] via fhir_extract_ref and matches with `&&`. A GIN over the
            // identical expression turns the per-row jsonb_array_elements seq scan
            // into a Bitmap Index Scan. Lax jsonpath `[*]` handles single-object and
            // array references uniformly. fhir_extract_ref canonicalizes each ref
            // (version-stripped, base-free), matching the predicate's candidate forms.
            SearchParameterType::Reference => {
                let mut ref_segs = segments.clone();
                ref_segs.push("reference".to_string());
                let jpa = paths_to_jsonpath_array(&[ref_segs]);
                format!(
                    "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_ref\" ON \"{table}\" \
                     USING gin (fhir_extract_ref(resource, {jpa}))"
                )
            }
            // Quantity params (e.g. Observation.value-quantity, combo/component-*). A
            // `combo-*` quantity binds a union of co-located paths (top-level
            // `valueQuantity` AND `component[*].valueQuantity`); index every branch.
            //   - top-level scalar path: btree over `(<segs>.value)::numeric`, matching
            //     the in-place numeric-cast comparison.
            //   - repeating path under an array parent (component[*]): two btrees over
            //     the numeric min/max hull, matching the quantity-array predicate's
            //     indexable prefilter (the exact `@?` stays as a recheck).
            SearchParameterType::Quantity => {
                let mut all_paths = param
                    .expression
                    .as_deref()
                    .map(|e| {
                        octofhir_search::sql_builder::fhirpath_to_jsonb_paths(e, resource_type)
                    })
                    .filter(|p| !p.is_empty())
                    .unwrap_or_else(|| vec![segments.clone()]);
                // Match the predicate (types/mod.rs): no index for dead
                // `value.ofType(SampledData)` branches (no `.value` scalar).
                all_paths.retain(|p| !p.last().is_some_and(|s| s.ends_with("SampledData")));
                if all_paths.is_empty() {
                    continue;
                }
                // One min and one max btree over the UNION of every value location,
                // matching `render_quantity_union_clauses_as_or`'s single predicate:
                // `extract_max(resource, <paths>) > N` / `extract_min(...) < N`. The
                // array literal is built by the SAME helper the predicate uses, so the
                // expressions are textually identical and the planner uses the index.
                let arr = octofhir_search::sql_builder::quantity_value_jsonpath_array(&all_paths);
                let qddls = [
                    format!(
                        "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_qmax\" ON \"{table}\" \
                         ((fhir_qty_extract_max_numeric(resource, {arr})))"
                    ),
                    format!(
                        "CREATE INDEX IF NOT EXISTS \"idx_{table}_{code}_qmin\" ON \"{table}\" \
                         ((fhir_qty_extract_min_numeric(resource, {arr})))"
                    ),
                ];
                for ddl in &qddls {
                    if let Some(body) = index_body(ddl)
                        && !seen_bodies.insert(body)
                    {
                        debug!(resource_type, code, "skipped duplicate quantity index");
                        continue;
                    }
                    match sqlx_core::raw_sql::raw_sql(AssertSqlSafe(ddl.to_string()))
                        .execute(pool)
                        .await
                    {
                        Ok(_) => {
                            created += 1;
                            debug!(resource_type, code, "created quantity functional index");
                        }
                        Err(e) => {
                            warn!(resource_type, code, error = %e, "skipped quantity functional index");
                        }
                    }
                }
                continue;
            }
            // Other token predicates are not yet index-matched in-place; their indexes
            // are added with those predicate rewrites.
            _ => continue,
        };

        if let Some(body) = index_body(&ddl)
            && !seen_bodies.insert(body)
        {
            debug!(resource_type, code, "skipped duplicate functional index");
            continue;
        }
        match sqlx_core::raw_sql::raw_sql(AssertSqlSafe((&ddl).to_string()))
            .execute(pool)
            .await
        {
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

/// One targeted partial composite index request (the db-side mirror of the server's
/// `CompositeIndexSpec`, kept here so this crate need not depend on the server crate).
#[derive(Debug, Clone)]
pub struct CompositePartialIndex {
    pub resource_type: String,
    pub param: String,
    pub system: Option<String>,
    pub code: String,
}

/// Create targeted PARTIAL composite indexes: a small btree on the quantity
/// component's top-level value, restricted (`WHERE`) to rows whose token component
/// equals a pinned code (optionally within a system). The `WHERE` mirrors the INLINED
/// `@>` containment the composite token render emits, so the planner uses the partial
/// index for that exact token instead of BitmapAnd-ing two large bitmaps. Only
/// top-level-value composites (e.g. `code-value-quantity`) are served; a component-
/// array value (no scalar `->>'value'`) is skipped. Idempotent (`IF NOT EXISTS`).
pub async fn create_composite_partial_indexes(
    pool: &PgPool,
    registry: &SearchParameterRegistry,
    specs: &[CompositePartialIndex],
) -> usize {
    use octofhir_search::sql_builder::{
        composite_token_containment_sql, fhirpath_to_jsonb_path, fhirpath_to_jsonb_paths,
        quantity_value_jsonpath_array,
    };
    use std::hash::{Hash, Hasher};
    let mut created = 0usize;
    for spec in specs {
        let Some(param_def) = registry.get(&spec.resource_type, &spec.param) else {
            warn!(rt = %spec.resource_type, param = %spec.param, "composite partial: unknown param");
            continue;
        };
        let components = match octofhir_search::ir::resolve_composite_component_specs(
            registry,
            &spec.resource_type,
            &param_def,
        ) {
            Ok(c) => c,
            Err(e) => {
                warn!(param = %spec.param, error = %e, "composite partial: component resolve failed");
                continue;
            }
        };
        let (Some(token), Some(quant)) = (
            components
                .iter()
                .find(|c| c.search_type == SearchParameterType::Token),
            components
                .iter()
                .find(|c| c.search_type == SearchParameterType::Quantity),
        ) else {
            warn!(param = %spec.param, "composite partial: needs one token + one quantity component");
            continue;
        };
        // Value union (top-level AND component[*]) — exactly the array the composite
        // render's `fhir_qty_extract_*` predicate builds. Drop SampledData (no scalar).
        let quant_paths: Vec<Vec<String>> =
            fhirpath_to_jsonb_paths(&quant.expression, &spec.resource_type)
                .into_iter()
                .filter(|p| !p.last().is_some_and(|s| s.ends_with("SampledData")))
                .collect();
        if quant_paths.is_empty() {
            warn!(param = %spec.param, "composite partial: quantity component has no value path — skipped");
            continue;
        }
        let arr = quantity_value_jsonpath_array(&quant_paths);
        // WHERE = the SAME code containment the render emits (shared helper) so the
        // query predicate implies the partial-index predicate byte-for-byte.
        let token_paths = {
            let p = fhirpath_to_jsonb_paths(&token.expression, &spec.resource_type);
            if p.is_empty() {
                vec![fhirpath_to_jsonb_path(
                    &token.expression,
                    &spec.resource_type,
                )]
            } else {
                p
            }
        };
        let where_clause = composite_token_containment_sql(
            "resource",
            &token_paths,
            spec.system.as_deref(),
            &spec.code,
        );
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&spec.resource_type, &spec.param, &spec.system, &spec.code).hash(&mut hasher);
        let hash = hasher.finish();
        let table = spec.resource_type.to_lowercase();
        let pname: String = spec
            .param
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        // One min and one max partial btree on the value union, pinned to the code —
        // matching the render's `fhir_qty_extract_max/min_numeric(resource, <arr>) <op> N`.
        let qddls = [
            (
                "max",
                format!(
                    "CREATE INDEX IF NOT EXISTS \"idx_{table}_{pname}_ptmax_{hash:016x}\" ON \"{table}\" \
                     ((fhir_qty_extract_max_numeric(resource, {arr}))) WHERE {where_clause}"
                ),
            ),
            (
                "min",
                format!(
                    "CREATE INDEX IF NOT EXISTS \"idx_{table}_{pname}_ptmin_{hash:016x}\" ON \"{table}\" \
                     ((fhir_qty_extract_min_numeric(resource, {arr}))) WHERE {where_clause}"
                ),
            ),
        ];
        for (kind, ddl) in qddls {
            match sqlx_core::raw_sql::raw_sql(AssertSqlSafe(ddl))
                .execute(pool)
                .await
            {
                Ok(_) => {
                    created += 1;
                    debug!(param = %spec.param, code = %spec.code, kind, "created composite partial index");
                }
                Err(e) => {
                    warn!(param = %spec.param, code = %spec.code, kind, error = %e, "skipped composite partial index");
                }
            }
        }
    }
    if !specs.is_empty() {
        info!(created, "composite partial indexes ensured");
    }
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
