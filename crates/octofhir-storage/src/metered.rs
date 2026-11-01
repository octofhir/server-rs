//! Timing of storage operations, recorded by [`crate::EventedStorage`] around every call it
//! delegates, so dashboards can tell time spent in the database from the rest of a request.

use std::future::Future;
use std::time::Instant;

use serde_json::Value;

/// Histogram of storage call latency, by operation, resource type and outcome.
pub const STORAGE_OPERATION_DURATION_SECONDS: &str = "octofhir_storage_operation_duration_seconds";

/// Runs `operation` and records how long it took. Transactional calls are prefixed `tx_`.
/// Public so callers that query the database outside [`crate::FhirStorage`] (the search
/// executors) land in the same histogram.
pub async fn timed<T, E>(
    operation: &'static str,
    resource_type: &str,
    call: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let start = Instant::now();
    let result = call.await;
    metrics::histogram!(
        STORAGE_OPERATION_DURATION_SECONDS,
        "operation" => operation,
        "resource_type" => resource_type.to_owned(),
        "outcome" => if result.is_ok() { "ok" } else { "error" }
    )
    .record(start.elapsed().as_secs_f64());
    result
}

/// The `resourceType` of a resource about to be written; `*` when absent.
pub(crate) fn resource_type_of(resource: &Value) -> &str {
    resource
        .get("resourceType")
        .and_then(Value::as_str)
        .unwrap_or("*")
}
