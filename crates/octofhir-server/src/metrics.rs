//! Prometheus metrics for OctoFHIR server.
//!
//! This module provides:
//! - HTTP request metrics (count, latency, active connections)
//! - Database pool metrics (connections, utilization)
//! - Cache metrics (hit/miss rates, entries)
//! - FHIR-specific metrics (resources by type)

use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;
use std::time::Duration;

/// Global Prometheus handle for rendering metrics.
static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Metric names as constants for consistency.
pub mod names {
    // HTTP metrics
    pub const HTTP_REQUESTS_TOTAL: &str = "http_requests_total";
    pub const HTTP_REQUEST_DURATION_SECONDS: &str = "http_request_duration_seconds";
    pub const HTTP_ACTIVE_CONNECTIONS: &str = "http_active_connections";

    // Database pool metrics
    pub const DB_POOL_CONNECTIONS_TOTAL: &str = "db_pool_connections_total";
    pub const DB_POOL_CONNECTIONS_IDLE: &str = "db_pool_connections_idle";
    pub const DB_POOL_CONNECTIONS_ACTIVE: &str = "db_pool_connections_active";
    pub const DB_POOL_ACQUIRE_DURATION_SECONDS: &str = "db_pool_acquire_duration_seconds";

    // Cache metrics
    pub const CACHE_HITS_TOTAL: &str = "cache_hits_total";
    pub const CACHE_MISSES_TOTAL: &str = "cache_misses_total";
    pub const CACHE_ENTRIES: &str = "cache_entries";

    // FHIR metrics
    pub const FHIR_RESOURCES_TOTAL: &str = "fhir_resources_total";
    pub const FHIR_OPERATIONS_TOTAL: &str = "fhir_operations_total";
}

/// Initialize the Prometheus metrics exporter.
///
/// This should be called once at server startup.
/// Returns `true` if initialization succeeded, `false` if already initialized.
pub fn init_metrics() -> bool {
    if PROMETHEUS_HANDLE.get().is_some() {
        tracing::debug!("Prometheus metrics already initialized");
        return false;
    }

    // Use install_recorder() for pull-based metrics (we serve /metrics ourselves)
    match PrometheusBuilder::new().install_recorder() {
        Ok(handle) => {
            if PROMETHEUS_HANDLE.set(handle).is_err() {
                tracing::warn!("Failed to store Prometheus handle (already set)");
                return false;
            }

            tracing::info!("Prometheus metrics initialized");
            true
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to install Prometheus recorder");
            false
        }
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
        "status" => status.to_string(),
        "status_class" => status_class.to_string()
    )
    .increment(1);

    histogram!(
        names::HTTP_REQUEST_DURATION_SECONDS,
        "method" => method_owned,
        "path" => normalized_path
    )
    .record(duration.as_secs_f64());
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
    gauge!(names::DB_POOL_CONNECTIONS_TOTAL).set(total as f64);
    gauge!(names::DB_POOL_CONNECTIONS_IDLE).set(idle as f64);
    gauge!(names::DB_POOL_CONNECTIONS_ACTIVE).set(active as f64);
}

/// Record database connection acquire duration.
pub fn record_db_acquire_duration(duration: Duration) {
    histogram!(names::DB_POOL_ACQUIRE_DURATION_SECONDS).record(duration.as_secs_f64());
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
    gauge!(names::FHIR_RESOURCES_TOTAL, "resource_type" => resource_type.to_string())
        .set(count as f64);
}

/// Record a FHIR operation invocation.
pub fn record_fhir_operation(operation: &str, resource_type: Option<&str>) {
    let rt = resource_type.unwrap_or("system");
    counter!(
        names::FHIR_OPERATIONS_TOTAL,
        "operation" => operation.to_string(),
        "resource_type" => rt.to_string()
    )
    .increment(1);
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

        if is_likely_id(part) && i > 0 {
            if prev_segment == "_history" {
                result.push_str("{vid}");
            } else if prev_segment == "_async-status" {
                result.push_str("{job_id}");
            } else {
                result.push_str("{id}");
            }
        } else {
            result.push_str(part);
        }

        prev_segment = part;
    }

    result
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
    fn test_is_likely_id() {
        assert!(is_likely_id("12345"));
        assert!(is_likely_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!is_likely_id("Patient"));
        assert!(!is_likely_id(""));
        assert!(!is_likely_id("abc"));
    }
}
