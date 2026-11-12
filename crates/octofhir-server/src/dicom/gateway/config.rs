//! Opt-in streaming limits for one administrator-configured DICOMweb gateway.
//!
//! # Contents
//! Metadata-query budgets, response byte limits and stream timeout validation.
//!
//! # Invariants
//! Stream limits do not relax bounded metadata reads or the egress policy.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DicomGatewayConfig {
    /// Maximum authorized FHIR studies considered by one QIDO query.
    #[serde(default = "studies")]
    pub max_query_studies: u32,
    /// Bounds live metadata used to evaluate one QIDO request.
    #[serde(default = "query_bytes")]
    pub max_query_bytes: usize,
    #[serde(default = "bytes")]
    pub max_stream_bytes: u64,
    #[serde(default = "total")]
    pub stream_seconds: u64,
    #[serde(default = "idle")]
    pub idle_seconds: u64,
}
fn studies() -> u32 {
    100
}
fn query_bytes() -> usize {
    128 * 1024 * 1024
}
fn bytes() -> u64 {
    8 * 1024 * 1024 * 1024
}
fn total() -> u64 {
    300
}
fn idle() -> u64 {
    30
}
impl DicomGatewayConfig {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(1..=1000).contains(&self.max_query_studies)
            || !(1024..=256 * 1024 * 1024).contains(&self.max_query_bytes)
            || self.max_stream_bytes == 0
            || self.max_stream_bytes > 1024 * 1024 * 1024 * 1024
            || !(1..=3600).contains(&self.stream_seconds)
            || !(1..=300).contains(&self.idle_seconds)
            || self.idle_seconds > self.stream_seconds
        {
            return Err("Invalid DICOM gateway stream limits".into());
        }
        Ok(())
    }
}
