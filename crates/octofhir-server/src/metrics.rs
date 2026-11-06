//! Prometheus metrics for OctoFHIR server.
//!
//! This module provides:
//! - HTTP request metrics (count, latency, active connections)
//! - Database pool metrics (connections, utilization)
//! - Cache metrics (hit/miss rates, entries)
//! - FHIR-specific metrics (resources by type)

use metrics::{
    Unit, counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use metrics_process::Collector;
use octofhir_storage::metered::STORAGE_OPERATION_DURATION_SECONDS;
use std::sync::OnceLock;
use std::time::Duration;

/// Global Prometheus handle for rendering metrics.
static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Metric names as constants for consistency.
pub mod names {
    // HTTP metrics
    pub const HTTP_REQUESTS_TOTAL: &str = "octofhir_http_requests_total";
    pub const HTTP_REQUEST_DURATION_SECONDS: &str = "octofhir_http_request_duration_seconds";
    pub const HTTP_ACTIVE_CONNECTIONS: &str = "octofhir_http_active_connections";

    // Database pool metrics
    pub const DB_POOL_CONNECTIONS_MAX: &str = "octofhir_db_pool_connections_max";
    pub const DB_POOL_CONNECTIONS_IDLE: &str = "octofhir_db_pool_connections_idle";
    pub const DB_POOL_CONNECTIONS_ACTIVE: &str = "octofhir_db_pool_connections_active";

    // Cache metrics
    pub const CACHE_HITS_TOTAL: &str = "octofhir_cache_hits_total";
    pub const CACHE_MISSES_TOTAL: &str = "octofhir_cache_misses_total";
    pub const CACHE_ENTRIES: &str = "octofhir_cache_entries";

    // FHIR metrics
    pub const FHIR_RESOURCES: &str = "octofhir_fhir_resources";

    // FHIR REST interactions (read, search, create, …) by resource type
    pub const FHIR_INTERACTIONS_TOTAL: &str = "octofhir_fhir_interactions_total";
    pub const FHIR_INTERACTION_DURATION_SECONDS: &str =
        "octofhir_fhir_interaction_duration_seconds";

    // Tokio runtime (stable metrics API)
    pub const TOKIO_WORKERS: &str = "octofhir_tokio_workers";
    pub const TOKIO_ALIVE_TASKS: &str = "octofhir_tokio_alive_tasks";
    pub const TOKIO_GLOBAL_QUEUE_DEPTH: &str = "octofhir_tokio_global_queue_depth";
    pub const TOKIO_WORKER_BUSY_SECONDS_TOTAL: &str = "octofhir_tokio_worker_busy_seconds_total";
    pub const TOKIO_WORKER_PARKS_TOTAL: &str = "octofhir_tokio_worker_parks_total";

    // Process metrics (CPU, memory, threads, fds come from `metrics-process` with this prefix)
    pub const BUILD_INFO: &str = "octofhir_build_info";
    pub const PROCESS_PREFIX: &str = "octofhir_";
}

/// Upper bounds (seconds) of the request latency histogram: 1 ms to 10 s, dense below 100 ms
/// where most FHIR requests land.
const HTTP_LATENCY_BUCKETS: &[f64] = &[
    0.001, 0.0025, 0.005, 0.0075, 0.01, 0.015, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
    10.0,
];

/// Upper bounds (seconds) of the storage-call histogram: database calls are mostly sub-millisecond
/// to tens of milliseconds, so it starts at 250 µs.
const STORAGE_LATENCY_BUCKETS: &[f64] = &[
    0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
];

/// Initialize the Prometheus metrics exporter.
///
/// This should be called once at server startup.
/// Returns `true` if initialization succeeded, `false` if already initialized.
pub fn init_metrics() -> bool {
    if PROMETHEUS_HANDLE.get().is_some() {
        tracing::debug!("Prometheus metrics already initialized");
        return false;
    }

    // Request latency is exported as a histogram, not the default summary: bucket counts add up
    // across routes and over any time window, so the console and Prometheus can compute exact
    // overall p50/p95/p99 (summary quantiles can't be combined).
    let builder = match PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full(names::HTTP_REQUEST_DURATION_SECONDS.to_string()),
            HTTP_LATENCY_BUCKETS,
        )
        .and_then(|builder| {
            builder.set_buckets_for_metric(
                Matcher::Full(names::FHIR_INTERACTION_DURATION_SECONDS.to_string()),
                HTTP_LATENCY_BUCKETS,
            )
        })
        .and_then(|builder| {
            builder.set_buckets_for_metric(
                Matcher::Full(STORAGE_OPERATION_DURATION_SECONDS.to_string()),
                STORAGE_LATENCY_BUCKETS,
            )
        }) {
        Ok(builder) => builder,
        Err(e) => {
            tracing::error!(error = %e, "Invalid latency histogram buckets");
            return false;
        }
    };

    // Use install_recorder() for pull-based metrics (we serve /metrics ourselves)
    match builder.install_recorder() {
        Ok(handle) => {
            if PROMETHEUS_HANDLE.set(handle).is_err() {
                tracing::warn!("Failed to store Prometheus handle (already set)");
                return false;
            }

            describe_metrics();
            record_process_info();
            tracing::info!("Prometheus metrics initialized");
            true
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to install Prometheus recorder");
            false
        }
    }
}

/// `# HELP` and unit for every metric: Prometheus keeps them as metadata, and the console's
/// PromQL editor shows them in completions and hovers.
fn describe_metrics() {
    describe_counter!(
        names::HTTP_REQUESTS_TOTAL,
        "HTTP requests served, by method, normalized path and status class"
    );
    describe_histogram!(
        names::HTTP_REQUEST_DURATION_SECONDS,
        Unit::Seconds,
        "HTTP request latency, by method and normalized path"
    );
    describe_gauge!(
        names::HTTP_ACTIVE_CONNECTIONS,
        "HTTP requests currently being served"
    );
    describe_gauge!(
        names::DB_POOL_CONNECTIONS_MAX,
        "Maximum size of the PostgreSQL connection pool"
    );
    describe_gauge!(
        names::DB_POOL_CONNECTIONS_IDLE,
        "Idle connections in the PostgreSQL pool"
    );
    describe_gauge!(
        names::DB_POOL_CONNECTIONS_ACTIVE,
        "Connections in use in the PostgreSQL pool"
    );
    describe_counter!(names::CACHE_HITS_TOTAL, "Cache hits, by tier");
    describe_counter!(names::CACHE_MISSES_TOTAL, "Cache misses");
    describe_gauge!(names::CACHE_ENTRIES, "Entries held in the cache, by tier");
    describe_gauge!(
        names::FHIR_RESOURCES,
        "Stored FHIR resources, by resource type"
    );
    describe_gauge!(
        names::BUILD_INFO,
        "Always 1; the version label names the running build"
    );
    describe_counter!(
        names::FHIR_INTERACTIONS_TOTAL,
        "FHIR REST interactions, by interaction, resource type and status class"
    );
    describe_histogram!(
        names::FHIR_INTERACTION_DURATION_SECONDS,
        Unit::Seconds,
        "FHIR REST interaction latency, by interaction and resource type"
    );
    describe_gauge!(names::TOKIO_WORKERS, "Worker threads of the Tokio runtime");
    describe_gauge!(
        names::TOKIO_ALIVE_TASKS,
        "Tasks alive in the Tokio runtime (spawned and not yet finished)"
    );
    describe_gauge!(
        names::TOKIO_GLOBAL_QUEUE_DEPTH,
        "Tasks waiting in the runtime's global queue for a free worker"
    );
    describe_gauge!(
        names::TOKIO_WORKER_BUSY_SECONDS_TOTAL,
        Unit::Seconds,
        "Time each worker spent running tasks; rate() of it is the worker's utilization"
    );
    describe_gauge!(
        names::TOKIO_WORKER_PARKS_TOTAL,
        "Times each worker parked for lack of work"
    );
    describe_histogram!(
        STORAGE_OPERATION_DURATION_SECONDS,
        Unit::Seconds,
        "Storage (database) call latency, by operation, resource type and outcome; tx_* calls run inside a transaction"
    );
    crate::db_metrics::describe();
    process_collector().describe();
}

/// Collector of CPU, memory, thread and file-descriptor metrics for this process.
fn process_collector() -> &'static Collector {
    static COLLECTOR: OnceLock<Collector> = OnceLock::new();
    COLLECTOR.get_or_init(|| Collector::new(names::PROCESS_PREFIX))
}

/// Publishes the build version, so dashboards show which version each instance runs.
fn record_process_info() {
    gauge!(names::BUILD_INFO, "version" => env!("CARGO_PKG_VERSION")).set(1.0);
}

/// Samples the process metrics; called on every scrape so they are current.
pub fn collect_process_metrics() {
    process_collector().collect();
}

/// Samples the Tokio runtime the scrape runs on: workers, live tasks, the global queue, and per
/// worker the cumulative busy time and park count. Only the stable metrics API is used, so no
/// `tokio_unstable` build flag is needed. Busy time is a float gauge (like CPU seconds) so
/// `rate()` gives exact utilization.
pub fn collect_runtime_metrics() {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let metrics = handle.metrics();
    let workers = metrics.num_workers();
    gauge!(names::TOKIO_WORKERS).set(workers as f64);
    gauge!(names::TOKIO_ALIVE_TASKS).set(metrics.num_alive_tasks() as f64);
    gauge!(names::TOKIO_GLOBAL_QUEUE_DEPTH).set(metrics.global_queue_depth() as f64);
    for worker in 0..workers {
        let label = worker.to_string();
        gauge!(names::TOKIO_WORKER_BUSY_SECONDS_TOTAL, "worker" => label.clone())
            .set(metrics.worker_total_busy_duration(worker).as_secs_f64());
        gauge!(names::TOKIO_WORKER_PARKS_TOTAL, "worker" => label)
            .set(metrics.worker_park_count(worker) as f64);
    }
}

/// Render all metrics in Prometheus text format.
///
/// Returns `None` if metrics were not initialized.
pub fn render_metrics() -> Option<String> {
    PROMETHEUS_HANDLE.get().map(|handle| handle.render())
}

// =============================================================================
// HTTP Metrics
// =============================================================================

/// Record an HTTP request.
pub fn record_http_request(method: &str, path: &str, status: u16, duration: Duration) {
    let status_class = match status {
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    };

    // Normalize path to avoid high cardinality
    let normalized_path = normalize_path(path);
    let method_owned = method.to_string();

    counter!(
        names::HTTP_REQUESTS_TOTAL,
        "method" => method_owned.clone(),
        "path" => normalized_path.clone(),
        "status_class" => status_class.to_string()
    )
    .increment(1);

    histogram!(
        names::HTTP_REQUEST_DURATION_SECONDS,
        "method" => method_owned.clone(),
        "path" => normalized_path
    )
    .record(duration.as_secs_f64());

    if let Some((interaction, resource_type)) = fhir_interaction(&method_owned, path) {
        counter!(
            names::FHIR_INTERACTIONS_TOTAL,
            "interaction" => interaction,
            "resource_type" => resource_type.clone(),
            "status_class" => status_class
        )
        .increment(1);
        histogram!(
            names::FHIR_INTERACTION_DURATION_SECONDS,
            "interaction" => interaction,
            "resource_type" => resource_type
        )
        .record(duration.as_secs_f64());
    }
}

/// The FHIR REST interaction a request performs and the resource type it targets ("system" for
/// whole-server interactions), from its method and path under `/fhir`. `None` outside the FHIR
/// API. Interaction names follow the FHIR spec's (`search-type`, `history-instance`, …).
pub fn fhir_interaction(method: &str, path: &str) -> Option<(&'static str, String)> {
    let rest = path.strip_prefix("/fhir")?;
    if !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let system = || "system".to_string();
    let interaction = match (method, segments.as_slice()) {
        ("POST", []) => ("batch", system()),
        ("GET", ["metadata"]) => ("capabilities", system()),
        ("GET", ["_history"]) => ("history-system", system()),
        (_, [op]) if op.starts_with('$') => ("operation", system()),
        ("GET", []) | ("POST", ["_search"]) => ("search-system", system()),
        (_, [rt, ..]) if !is_resource_type_segment(rt) => return None,
        ("GET", [rt]) | ("POST", [rt, "_search"]) => ("search-type", rt.to_string()),
        ("POST", [rt]) => ("create", rt.to_string()),
        ("PUT", [rt]) => ("conditional-update", rt.to_string()),
        ("PATCH", [rt]) => ("conditional-patch", rt.to_string()),
        ("DELETE", [rt]) => ("conditional-delete", rt.to_string()),
        ("GET", [rt, "_history"]) => ("history-type", rt.to_string()),
        (_, [rt, op]) if op.starts_with('$') => ("operation", rt.to_string()),
        ("GET", [rt, _]) => ("read", rt.to_string()),
        ("PUT", [rt, _]) => ("update", rt.to_string()),
        ("PATCH", [rt, _]) => ("patch", rt.to_string()),
        ("DELETE", [rt, _]) => ("delete", rt.to_string()),
        ("GET", [rt, _, "_history"]) => ("history-instance", rt.to_string()),
        ("GET", [rt, _, "_history", _]) => ("vread", rt.to_string()),
        ("DELETE", [rt, _, "_history"]) => ("delete-history", rt.to_string()),
        ("DELETE", [rt, _, "_history", _]) => ("delete-history-version", rt.to_string()),
        (_, [rt, _, op]) if op.starts_with('$') => ("operation", rt.to_string()),
        // Compartment search: /Patient/123/Observation
        ("GET", [rt, _, _]) => ("search-compartment", rt.to_string()),
        _ => return None,
    };
    Some(interaction)
}

/// Increment active HTTP connections.
pub fn increment_active_connections() {
    gauge!(names::HTTP_ACTIVE_CONNECTIONS).increment(1.0);
}

/// Decrement active HTTP connections.
pub fn decrement_active_connections() {
    gauge!(names::HTTP_ACTIVE_CONNECTIONS).decrement(1.0);
}

// =============================================================================
// Database Pool Metrics
// =============================================================================

/// Record database pool statistics.
pub fn record_db_pool_stats(total: u32, idle: u32, active: u32) {
    gauge!(names::DB_POOL_CONNECTIONS_MAX).set(total as f64);
    gauge!(names::DB_POOL_CONNECTIONS_IDLE).set(idle as f64);
    gauge!(names::DB_POOL_CONNECTIONS_ACTIVE).set(active as f64);
}

// =============================================================================
// Cache Metrics
// =============================================================================

/// Record a cache hit.
pub fn record_cache_hit(tier: &str) {
    counter!(names::CACHE_HITS_TOTAL, "tier" => tier.to_string()).increment(1);
}

/// Record a cache miss.
pub fn record_cache_miss() {
    counter!(names::CACHE_MISSES_TOTAL).increment(1);
}

/// Set the number of cache entries.
pub fn set_cache_entries(tier: &str, count: usize) {
    gauge!(names::CACHE_ENTRIES, "tier" => tier.to_string()).set(count as f64);
}

// =============================================================================
// FHIR Metrics
// =============================================================================

/// Record FHIR resource count by type.
pub fn set_fhir_resource_count(resource_type: &str, count: u64) {
    gauge!(names::FHIR_RESOURCES, "resource_type" => resource_type.to_string()).set(count as f64);
}

// =============================================================================
// Helpers
// =============================================================================

/// Normalize a path to reduce cardinality.
///
/// Replaces resource IDs with placeholders to avoid creating too many unique label values.
/// Uses direct string building to avoid per-segment String allocations.
fn normalize_path(path: &str) -> String {
    // Common patterns to normalize:
    // /fhir/Patient/123 -> /fhir/Patient/{id}
    // /fhir/Patient/123/_history/2 -> /fhir/Patient/{id}/_history/{vid}
    // /_async-status/uuid -> /_async-status/{job_id}

    let mut result = String::with_capacity(path.len());
    let mut prev_segment = "";

    for (i, part) in path.split('/').enumerate() {
        if i > 0 {
            result.push('/');
        }

        if i > 0 && prev_segment == "_history" {
            result.push_str("{vid}");
        } else if i > 0 && prev_segment == "_async-status" {
            result.push_str("{job_id}");
        } else if i > 0
            && !part.is_empty()
            && !part.starts_with('_')
            && !part.starts_with('$')
            && (is_likely_id(part) || is_resource_type_segment(prev_segment))
        {
            // The segment after a resource type is always an id, regardless of
            // its shape — normalizes short/slug ids that `is_likely_id` misses.
            result.push_str("{id}");
        } else {
            result.push_str(part);
        }

        prev_segment = part;
    }

    result
}

/// True if a path segment looks like a FHIR resource type (e.g. `Patient`,
/// `ValueSet`): starts with an uppercase ASCII letter and is otherwise
/// alphanumeric. Used so the following segment is treated as a resource id.
fn is_resource_type_segment(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    s.len() >= 2 && chars.all(|c| c.is_ascii_alphanumeric())
}

/// Check if a string looks like an ID (UUID or numeric).
fn is_likely_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    // UUID pattern (with or without dashes)
    if s.len() == 36 && s.chars().filter(|c| *c == '-').count() == 4 {
        return true;
    }
    if s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }

    // Numeric ID
    if s.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }

    // Long alphanumeric (likely an ID)
    if s.len() > 12
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("/fhir/Patient/12345"), "/fhir/Patient/{id}");
        assert_eq!(
            normalize_path("/fhir/Patient/550e8400-e29b-41d4-a716-446655440000"),
            "/fhir/Patient/{id}"
        );
        assert_eq!(
            normalize_path("/fhir/Patient/123/_history/2"),
            "/fhir/Patient/{id}/_history/{vid}"
        );
        assert_eq!(normalize_path("/metadata"), "/metadata");
        assert_eq!(normalize_path("/fhir/Patient"), "/fhir/Patient");
    }

    #[test]
    fn classifies_fhir_interactions() {
        let case = |method, path| fhir_interaction(method, path).map(|(i, rt)| format!("{i} {rt}"));
        assert_eq!(case("GET", "/fhir/Patient/1"), Some("read Patient".into()));
        assert_eq!(
            case("GET", "/fhir/Patient"),
            Some("search-type Patient".into())
        );
        assert_eq!(
            case("POST", "/fhir/Patient/_search"),
            Some("search-type Patient".into())
        );
        assert_eq!(case("POST", "/fhir/Patient"), Some("create Patient".into()));
        assert_eq!(
            case("PUT", "/fhir/Patient/1"),
            Some("update Patient".into())
        );
        assert_eq!(
            case("DELETE", "/fhir/Patient/1"),
            Some("delete Patient".into())
        );
        assert_eq!(
            case("GET", "/fhir/Patient/1/_history/2"),
            Some("vread Patient".into())
        );
        assert_eq!(
            case("GET", "/fhir/Patient/1/_history"),
            Some("history-instance Patient".into())
        );
        assert_eq!(
            case("GET", "/fhir/Patient/1/Observation"),
            Some("search-compartment Patient".into())
        );
        assert_eq!(
            case("POST", "/fhir/Patient/$validate"),
            Some("operation Patient".into())
        );
        assert_eq!(case("POST", "/fhir"), Some("batch system".into()));
        assert_eq!(
            case("GET", "/fhir/metadata"),
            Some("capabilities system".into())
        );
        assert_eq!(
            case("POST", "/fhir/$import"),
            Some("operation system".into())
        );
        assert_eq!(case("GET", "/fhir/_async-status"), None);
        assert_eq!(case("GET", "/fhirx/Patient"), None);
        assert_eq!(case("GET", "/api/metrics"), None);
    }

    #[test]
    fn test_is_likely_id() {
        assert!(is_likely_id("12345"));
        assert!(is_likely_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!is_likely_id("Patient"));
        assert!(!is_likely_id(""));
        assert!(!is_likely_id("abc"));
    }
}
