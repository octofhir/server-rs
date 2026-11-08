//! Explicit UPS-RS roles, service identity and result publication policy.
//!
//! # Invariants
//! No performer or inference engine runs here. Coding schemes and clinical result
//! interpretations are administrator-selected; UPS completion alone is not a
//! clinical report's final status.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DicomUpsConfig {
    pub url: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub ae_title: String,
    pub bearer_token_env: Option<String>,
    #[serde(default = "enabled")]
    pub notifications: bool,
    /// Optional same-origin WebSocket endpoint, including the configured AE title.
    pub notification_url: Option<String>,
    #[serde(default = "poll")]
    pub poll_seconds: u64,
    #[serde(default = "retry")]
    pub retry_seconds: u64,
    #[serde(default = "references")]
    pub max_references: usize,
    /// FHIR Coding.system URL -> DICOM Coding Scheme Designator.
    pub coding_schemes: BTreeMap<String, String>,
    #[serde(default)]
    pub result_mappings: BTreeMap<String, ResultMapping>,
    pub output_stow_url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultMapping {
    pub kind: ResultKind,
    pub system: String,
    pub code: String,
    pub display: Option<String>,
    /// Required for DiagnosticReport; never inferred from UPS COMPLETED.
    pub report_status: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    DocumentReference,
    DiagnosticReport,
}
fn enabled() -> bool {
    true
}
fn poll() -> u64 {
    30
}
fn retry() -> u64 {
    5
}
fn references() -> usize {
    10000
}
impl DicomUpsConfig {
    pub(crate) fn notification_url(
        &self,
        connection: &super::super::DicomConnection,
    ) -> Result<url::Url, String> {
        let base = connection.service_url(&self.url)?;
        let mut url = if let Some(url) = &self.notification_url {
            url::Url::parse(url).map_err(|_| "Invalid UPS notification URL")?
        } else {
            let mut url = base.clone();
            url.path_segments_mut()
                .map_err(|_| "Invalid UPS URL")?
                .pop_if_empty()
                .extend(["subscribers", &self.ae_title]);
            url.set_scheme(if base.scheme() == "https" {
                "wss"
            } else {
                "ws"
            })
            .map_err(|_| "Invalid UPS WebSocket scheme")?;
            url
        };
        if !matches!(url.scheme(), "ws" | "wss") {
            return Err("UPS notification URL requires ws/wss".into());
        }
        let scheme = url.scheme().to_owned();
        url.set_scheme(if scheme == "wss" { "https" } else { "http" })
            .map_err(|_| "Invalid UPS scheme")?;
        let checked = connection.service_url(url.as_str())?;
        if checked.origin() != base.origin() {
            return Err("UPS notification URL must share the configured UPS origin".into());
        }
        url.set_scheme(&scheme).map_err(|_| "Invalid UPS scheme")?;
        Ok(url)
    }

    pub(crate) fn validate(
        &self,
        connection: &super::super::DicomConnection,
        policy: &crate::egress::EgressPolicy,
    ) -> Result<(), String> {
        policy.check_url(&connection.service_url(&self.url)?)?;
        if let Some(url) = &self.output_stow_url {
            policy.check_url(&connection.service_url(url)?)?;
        }
        super::super::config::validate_service_identity(&self.client_id, &self.scopes)?;
        let notification = self.notification_url(connection)?;
        let mut transport = notification;
        transport
            .set_scheme(if transport.scheme() == "wss" {
                "https"
            } else {
                "http"
            })
            .map_err(|_| "Invalid UPS notification scheme")?;
        policy.check_url(&transport)?;
        if !(1..=3600).contains(&self.poll_seconds)
            || !(1..=100000).contains(&self.max_references)
            || !text(&self.ae_title, 16)
            || self.coding_schemes.is_empty()
            || self.coding_schemes.len() > 64
        {
            return Err(
                "Invalid UPS-RS polling, reference, AE title or coding-scheme configuration".into(),
            );
        }
        if self
            .coding_schemes
            .iter()
            .any(|(system, scheme)| url::Url::parse(system).is_err() || !text(scheme, 16))
        {
            return Err("UPS coding schemes require absolute FHIR systems and 1..16-character DICOM designators".into());
        }
        if self.bearer_token_env.as_ref().is_some_and(|v| {
            v.is_empty() || !v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        }) {
            return Err("Invalid UPS credential environment reference".into());
        }
        for (sop, mapping) in &self.result_mappings {
            super::super::validate_uid(sop).map_err(|_| "Invalid UPS result SOP Class UID")?;
            if url::Url::parse(&mapping.system).is_err()
                || mapping.code.is_empty()
                || mapping.code.len() > 256
                || mapping.display.as_ref().is_some_and(|d| d.len() > 1024)
            {
                return Err("Invalid UPS clinical result mapping".into());
            }
            match mapping.kind {
                ResultKind::DiagnosticReport
                    if !mapping.report_status.as_deref().is_some_and(|s| {
                        matches!(
                            s,
                            "registered"
                                | "partial"
                                | "preliminary"
                                | "final"
                                | "amended"
                                | "corrected"
                                | "appended"
                                | "cancelled"
                                | "entered-in-error"
                                | "unknown"
                        )
                    }) =>
                {
                    return Err(
                        "UPS DiagnosticReport mappings require an explicit FHIR report_status"
                            .into(),
                    );
                }
                ResultKind::DocumentReference if mapping.report_status.is_some() => {
                    return Err("report_status applies only to DiagnosticReport mappings".into());
                }
                _ => {}
            }
        }
        Ok(())
    }
}
pub(super) fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= max
        && value.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
        && !value.contains('\\')
}
