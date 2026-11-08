//! Explicit service identity and bounded DICOM reconciliation scheduling.
//!
//! # Contents
//! Scan/notification selection, SMART delegation and retry/page budgets.
//!
//! # Invariants
//! Synchronization requires an active registered backend client and administrator-
//! selected system scopes. PACS credentials never grant access to FHIR resources.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DicomSyncConfig {
    pub client_id: String,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub discovery: DiscoveryMode,
    #[serde(default = "rescan")]
    pub rescan_seconds: u64,
    #[serde(default = "page_size")]
    pub page_size: u32,
    #[serde(default = "pages")]
    pub pages_per_run: u32,
    #[serde(default = "max_studies")]
    pub max_studies_per_scan: u32,
    #[serde(default = "retry")]
    pub retry_seconds: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    #[default]
    Scan,
    Notifications,
}
fn rescan() -> u64 {
    300
}
fn page_size() -> u32 {
    100
}
fn pages() -> u32 {
    4
}
fn max_studies() -> u32 {
    1_000_000
}
fn retry() -> u64 {
    5
}

impl DicomSyncConfig {
    pub(crate) fn validate(&self) -> Result<(), String> {
        super::super::config::validate_service_identity(&self.client_id, &self.scopes)?;
        if !(1..=86400).contains(&self.rescan_seconds)
            || !(1..=1000).contains(&self.page_size)
            || !(1..=32).contains(&self.pages_per_run)
            || !(self.page_size..=10_000_000).contains(&self.max_studies_per_scan)
            || !(1..=3600).contains(&self.retry_seconds)
        {
            return Err("DICOM sync scan/retry limits are invalid".into());
        }
        Ok(())
    }
}
