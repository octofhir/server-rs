//! Opt-in streaming limits for one administrator-configured DICOMweb gateway.
//!
//! # Invariants
//! Stream limits do not relax bounded metadata reads or the egress policy.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DicomGatewayConfig {
    #[serde(default = "bytes")]
    pub max_stream_bytes: u64,
    #[serde(default = "total")]
    pub stream_seconds: u64,
    #[serde(default = "idle")]
    pub idle_seconds: u64,
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
        if self.max_stream_bytes == 0
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
