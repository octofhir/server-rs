//! FHIR Bulk Data Export Implementation
//!
//! This module implements the FHIR Bulk Data Access IG ($export) specification
//! for exporting large datasets in NDJSON format.
//!
//! ## Supported Operations
//!
//! - `GET /$export` - System-level export (all resources)
//! - `GET /Patient/$export` - Patient-level export (patient compartment data)
//! - `GET /Group/{id}/$export` - Group-level export (group member data)
//!
//! ## Parameters
//!
//! - `_outputFormat` - Output format (only `application/fhir+ndjson` supported)
//! - `_since`, `_until` - Only resources updated in this window
//! - `_type` - Resource types to include
//! - `_typeFilter` - FHIR search queries per resource type
//! - `_elements` - Root elements to keep
//! - `patient` - Limits Patient and Group exports to these patients
//! - `includeAssociatedData` - Which Provenance resources to include
//! - `organizeOutputBy` - `Patient`: output files of per-patient resource blocks
//! - `allowPartialManifests` - Manifest pages, available while the job runs
//!
//! `bulk_export.spec_version` selects the IG version these follow; see
//! `docs/bulk-data-spec.md` for what each one requires of the server.
//!
//! ## Response Flow
//!
//! 1. Client requests `$export` with `Prefer: respond-async` header
//! 2. Server returns `202 Accepted` with `Content-Location` header pointing to status URL
//! 3. Client polls status endpoint (`GET /fhir/_async-status/{job-id}`)
//! 4. When complete, status returns manifest with file URLs
//! 5. Client downloads NDJSON files from manifest URLs
//!
//! ## References
//!
//! - [Bulk Data Access IG](http://hl7.org/fhir/uv/bulkdata/)
//! - [FHIR Asynchronous Request Pattern](http://hl7.org/fhir/async.html)

use std::time::{Duration, Instant};

use dashmap::{DashMap, mapref::entry::Entry};
use uuid::Uuid;

mod export;
mod import;
pub(crate) mod manifest;
mod status;
pub(crate) mod submit;
mod writer;

pub use export::{ExportOperation, execute_bulk_export};
pub use import::{ImportOperation, execute_bulk_import};
pub use status::{
    BulkExportJob, BulkExportLevel, BulkExportManifest, BulkExportOutput, BulkExportStatus,
};
pub use submit::{
    BulkSubmitOperation, BulkSubmitStatusOperation, execute_bulk_submit, execute_bulk_submit_revert,
};
pub use writer::{NdjsonWriter, cleanup_expired_exports};

/// Remembers when each job was last polled, for the IG's advice to answer too frequent
/// status requests with 429. State per instance: a stricter limit than needed is the
/// worst a replica can cause, and a client that honours `Retry-After` never sees it.
#[derive(Default)]
pub struct PollGate(DashMap<Uuid, Instant>);

/// Jobs remembered before finished ones are forgotten.
const POLL_GATE_CAPACITY: usize = 4096;

impl PollGate {
    /// Records a poll. Returns the seconds to wait when it comes too soon.
    pub(crate) fn check(&self, job: Uuid, interval_ms: u64) -> Option<u64> {
        if interval_ms == 0 {
            return None;
        }
        let interval = Duration::from_millis(interval_ms);
        let now = Instant::now();
        if self.0.len() >= POLL_GATE_CAPACITY {
            self.0
                .retain(|_, polled| now.duration_since(*polled) < interval);
        }
        match self.0.entry(job) {
            Entry::Occupied(mut entry) => {
                if now.duration_since(*entry.get()) < interval {
                    return Some(interval.as_secs().max(1));
                }
                entry.insert(now);
                None
            }
            Entry::Vacant(entry) => {
                entry.insert(now);
                None
            }
        }
    }
}

/// NDJSON content type as per Bulk Data specification
pub const NDJSON_CONTENT_TYPE: &str = "application/fhir+ndjson";

/// Default output format
pub const DEFAULT_OUTPUT_FORMAT: &str = "application/fhir+ndjson";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polls_inside_the_interval_are_refused_per_job() {
        let gate = PollGate::default();
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(gate.check(first, 60_000), None);
        assert_eq!(gate.check(first, 60_000), Some(60));
        assert_eq!(gate.check(second, 60_000), None);
        assert_eq!(gate.check(first, 0), None, "0 turns the check off");
    }
}
