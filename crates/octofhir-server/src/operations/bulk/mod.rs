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
//! - `_since` - Only resources updated since this timestamp
//! - `_type` - Comma-separated list of resource types to include
//! - `_typeFilter` - FHIR search queries per resource type
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

mod export;
mod import;
mod status;
mod writer;

pub use export::{ExportOperation, execute_bulk_export};
pub use import::{ImportOperation, execute_bulk_import};
pub use status::{
    BulkExportJob, BulkExportLevel, BulkExportManifest, BulkExportOutput, BulkExportStatus,
};
pub use writer::{NdjsonWriter, cleanup_expired_exports};

/// NDJSON content type as per Bulk Data specification
pub const NDJSON_CONTENT_TYPE: &str = "application/fhir+ndjson";

/// Default output format
pub const DEFAULT_OUTPUT_FORMAT: &str = "application/fhir+ndjson";
