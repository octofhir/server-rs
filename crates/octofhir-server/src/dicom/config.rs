//! Configuration for named DICOMweb connections.
//!
//! # Invariants
//! Connections are administrator-configured, URLs contain no secrets, and credentials
//! are resolved from environment variables. Empty connections disable integration.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::egress::EgressPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DicomConfig {
    /// Bounds the dedicated advisory-lock sessions; never consumes the FHIR pool.
    #[serde(default = "default_concurrency")]
    pub max_concurrent_imports: u32,
    #[serde(default = "default_streams")]
    pub max_concurrent_streams: u32,
    #[serde(default)]
    pub connections: BTreeMap<String, DicomConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DicomConnection {
    /// WADO-RS service root, also published in the FHIR Endpoint.
    pub wado_url: String,
    /// Separate QIDO-RS root; enables PACS matching in the gateway and scan discovery.
    pub qido_url: Option<String>,
    pub sync: Option<super::sync::DicomSyncConfig>,
    /// Enables only the explicitly supported, authorized retrieval routes.
    pub gateway: Option<super::DicomGatewayConfig>,
    pub ups: Option<super::DicomUpsConfig>,
    /// DICOM Issuer of Patient ID -> FHIR Patient.identifier.system.
    pub patient_identifier_systems: BTreeMap<String, String>,
    /// Explicit assigning authority for instances without an issuer.
    pub default_patient_issuer: Option<String>,
    /// Environment variable containing the PACS bearer credential.
    pub bearer_token_env: Option<String>,
    /// Permit HTTP for a deployment with an explicitly trusted network.
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_bytes")]
    pub max_metadata_bytes: usize,
    #[serde(default = "default_instances")]
    pub max_instances: usize,
}

fn default_streams() -> u32 {
    16
}
fn default_concurrency() -> u32 {
    2
}

impl Default for DicomConfig {
    fn default() -> Self {
        Self {
            max_concurrent_imports: default_concurrency(),
            max_concurrent_streams: default_streams(),
            connections: BTreeMap::new(),
        }
    }
}

fn default_timeout() -> u64 {
    30
}
fn default_bytes() -> usize {
    64 * 1024 * 1024
}
fn default_instances() -> usize {
    10_000
}

impl DicomConnection {
    pub(crate) fn service_url(&self, address: &str) -> Result<Url, String> {
        let url = Url::parse(address).map_err(|_| "invalid DICOMweb URL")?;
        if !(url.scheme() == "https" || (self.allow_http && url.scheme() == "http"))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("DICOMweb needs an HTTPS service root without credentials, query or fragment; HTTP requires allow_http".into());
        }
        Ok(url)
    }
    pub(crate) fn base_url(&self) -> Result<Url, String> {
        self.service_url(&self.wado_url)
    }
}

pub(crate) fn validate_service_identity(client_id: &str, scopes: &[String]) -> Result<(), String> {
    if client_id.is_empty()
        || client_id.len() > 128
        || scopes.is_empty()
        || scopes.len() > 32
        || scopes.iter().any(|scope| {
            !scope.starts_with("system/")
                || octofhir_auth::smart::scopes::SmartScope::parse(scope).is_err()
        })
    {
        return Err("DICOM background work requires a backend client_id and explicit valid system SMART scopes".into());
    }
    Ok(())
}

impl DicomConfig {
    pub(crate) fn validate(&self, policy: &EgressPolicy, release: &str) -> Result<(), String> {
        if !(1..=256).contains(&self.max_concurrent_streams) {
            return Err("dicom.max_concurrent_streams must be in 1..=256".into());
        }
        if !(1..=16).contains(&self.max_concurrent_imports) {
            return Err("dicom.max_concurrent_imports must be in 1..=16".into());
        }
        if !self.connections.is_empty() {
            let release: octofhir_core::fhir::FhirVersion =
                release.parse().map_err(|_| "invalid FHIR release")?;
            if release == octofhir_core::fhir::FhirVersion::R6 {
                return Err("DICOM import currently supports FHIR R4, R4B and R5".into());
            }
        }
        let mut watchers = std::collections::BTreeSet::new();
        for (name, connection) in &self.connections {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                return Err(
                    "DICOM connection names require 1..64 ASCII letters, digits, '-' or '_'".into(),
                );
            }
            policy.check_url(&connection.base_url()?)?;
            if let Some(qido) = &connection.qido_url {
                policy.check_url(&connection.service_url(qido)?)?;
            }
            if let Some(ups) = &connection.ups {
                ups.validate(connection, policy)?;
                if !watchers.insert((
                    connection
                        .service_url(&ups.url)?
                        .to_string()
                        .trim_end_matches('/')
                        .to_owned(),
                    ups.ae_title.clone(),
                )) {
                    return Err(
                        "DICOM UPS connections to the same Task Manager require distinct AE titles"
                            .into(),
                    );
                }
            }
            if let Some(gateway) = &connection.gateway {
                gateway.validate()?;
            }
            if let Some(sync) = &connection.sync {
                sync.validate()?;
                if sync.discovery == super::sync::DiscoveryMode::Scan
                    && connection.qido_url.is_none()
                {
                    return Err("DICOM scan discovery requires qido_url".into());
                }
            }
            if connection.timeout_seconds == 0
                || connection.timeout_seconds > 300
                || connection.max_metadata_bytes == 0
                || connection.max_metadata_bytes > 64 * 1024 * 1024
                || connection.max_instances == 0
                || connection.max_instances > 100_000
            {
                return Err(format!(
                    "DICOM connection {name}: invalid request/metadata limits"
                ));
            }
            if connection.patient_identifier_systems.is_empty()
                || connection
                    .patient_identifier_systems
                    .iter()
                    .any(|(issuer, system)| {
                        issuer.trim().is_empty()
                            || system.trim().is_empty()
                            || Url::parse(system).is_err()
                    })
            {
                return Err(format!(
                    "DICOM connection {name}: assigning authorities must map to absolute FHIR identifier system URIs"
                ));
            }
            if let Some(issuer) = &connection.default_patient_issuer
                && !connection.patient_identifier_systems.contains_key(issuer)
            {
                return Err(format!(
                    "DICOM connection {name}: default patient issuer must have an identifier-system mapping"
                ));
            }
            if let Some(variable) = &connection.bearer_token_env
                && (variable.is_empty()
                    || !variable
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_'))
            {
                return Err(format!(
                    "DICOM connection {name}: invalid credential environment variable name"
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn connection() -> DicomConnection {
        DicomConnection {
            wado_url: "https://pacs.example/dicom-web".into(),
            qido_url: None,
            sync: None,
            gateway: None,
            ups: None,
            patient_identifier_systems: BTreeMap::from([(
                "HOSPITAL".into(),
                "urn:oid:1.2.3".into(),
            )]),
            default_patient_issuer: None,
            bearer_token_env: None,
            allow_http: false,
            timeout_seconds: 30,
            max_metadata_bytes: default_bytes(),
            max_instances: default_instances(),
        }
    }

    #[test]
    fn metadata_defaults_and_overrides_fit_connection_and_gateway_budgets() {
        let value = serde_json::json!({
            "wado_url": "https://pacs.example/dicom-web",
            "patient_identifier_systems": {"HOSPITAL": "urn:test:mrn"},
            "gateway": {}
        });
        for (limit, accepted) in [
            (None, true),
            (Some(8 * 1024 * 1024), true),
            (Some(64 * 1024 * 1024), true),
            (Some(64 * 1024 * 1024 + 1), false),
        ] {
            let mut value = value.clone();
            if let Some(limit) = limit {
                value["max_metadata_bytes"] = limit.into();
            }
            let connection: DicomConnection = serde_json::from_value(value).unwrap();
            assert_eq!(
                connection.max_metadata_bytes,
                limit.unwrap_or(64 * 1024 * 1024)
            );
            let gateway = connection.gateway.as_ref().unwrap();
            assert_eq!(gateway.max_query_studies, 100);
            assert_eq!(gateway.max_query_bytes, 128 * 1024 * 1024);
            let config = DicomConfig {
                connections: BTreeMap::from([("pacs".into(), connection)]),
                ..Default::default()
            };
            assert_eq!(
                config.validate(&EgressPolicy::default(), "R4").is_ok(),
                accepted
            );
        }
    }

    #[test]
    fn rejects_credentials_in_urls_and_unmapped_default_issuer() {
        for address in [
            "http://pacs.example",
            "https://u:p@pacs.example",
            "https://pacs.example?token=secret",
            "file:///tmp/pacs",
            "https://pacs.example/#x",
        ] {
            let mut c = connection();
            c.wado_url = address.into();
            assert!(c.base_url().is_err(), "{address}");
        }
        let mut c = connection();
        c.default_patient_issuer = Some("OTHER".into());
        let config = DicomConfig {
            connections: BTreeMap::from([("pacs".into(), c)]),
            ..Default::default()
        };
        assert!(config.validate(&EgressPolicy::default(), "R4").is_err());
    }
}
