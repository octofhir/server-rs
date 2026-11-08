//! Source-service DICOM control plane: discovery, ordered notifications and status.
//!
//! # Contents
//! Strict FHIR Parameters and operation registration for configured sync clients.
//!
//! # Invariants
//! A caller must be the explicitly selected backend client with the configured
//! scopes. PACS credentials and anonymous access never authorize these operations.
//! Patient discovery verifies every returned identity before exposing a study UID.

use super::{
    OperationContext, OperationDefinition, OperationError, OperationHandler, OperationKind,
    OperationParameter, OperationRegistry, ParameterUse, handler::OperationLevel,
};
use crate::{
    dicom::{DicomConnection, client::DicomWebClient, sync::store},
    server::AppState,
};
use async_trait::async_trait;
use octofhir_api::ApiError;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{collections::BTreeMap, sync::Arc};

pub struct DicomNotify;
pub struct DicomDiscover;
pub struct DicomSyncStatus;

pub(crate) fn register(registry: &mut OperationRegistry, config: &crate::dicom::DicomConfig) {
    if !config.connections.values().any(|c| c.sync.is_some()) {
        return;
    }
    for (code, resources, affects, parameters) in [
        (
            "dicom-notify",
            Vec::new(),
            true,
            vec!["connection", "study", "change", "sequence"],
        ),
        ("dicom-sync-status", Vec::new(), false, vec!["connection"]),
        (
            "dicom-discover",
            vec!["Patient".into()],
            false,
            vec!["connection", "issuer", "offset", "limit"],
        ),
    ] {
        if code == "dicom-discover"
            && !config
                .connections
                .values()
                .any(|c| c.qido_url.is_some() && c.sync.is_some())
        {
            continue;
        }
        let system = resources.is_empty();
        registry.replace(OperationDefinition {
            code: code.into(),
            url: format!("https://octofhir.org/OperationDefinition/{code}"),
            kind: OperationKind::Operation,
            system,
            type_level: false,
            instance: !system,
            resource: resources,
            affects_state: affects,
            parameters: parameters
                .into_iter()
                .map(|name| OperationParameter {
                    name: name.into(),
                    use_: ParameterUse::In,
                    min: if matches!(name, "issuer" | "offset" | "limit") {
                        0
                    } else {
                        1
                    },
                    max: "1".into(),
                    param_type: Some("string".into()),
                    search_type: None,
                    target_profile: vec![],
                    parts: vec![],
                })
                .collect(),
        });
    }
}

fn strings<'a>(
    params: &'a Value,
    allowed: &[&str],
) -> Result<BTreeMap<&'a str, &'a str>, OperationError> {
    let invalid = || {
        OperationError::InvalidParameters("Expected unique supported valueString Parameters".into())
    };
    if params["resourceType"] != "Parameters" {
        return Err(invalid());
    }
    let entries = params["parameter"]
        .as_array()
        .filter(|a| a.len() <= allowed.len())
        .ok_or_else(invalid)?;
    let mut result = BTreeMap::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(invalid)?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "name" | "valueString"))
        {
            return Err(invalid());
        }
        let name = entry["name"]
            .as_str()
            .filter(|name| allowed.contains(name))
            .ok_or_else(invalid)?;
        let value = entry["valueString"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 1024)
            .ok_or_else(invalid)?;
        if result.insert(name, value).is_some() {
            return Err(invalid());
        }
    }
    Ok(result)
}
fn required<'a>(params: &BTreeMap<&str, &'a str>, name: &str) -> Result<&'a str, OperationError> {
    params
        .get(name)
        .copied()
        .ok_or_else(|| OperationError::InvalidParameters(format!("Missing {name}")))
}
fn source<'a>(
    state: &'a AppState,
    context: &OperationContext,
    name: &str,
) -> Result<&'a DicomConnection, OperationError> {
    let source = state
        .config
        .dicom
        .connections
        .get(name)
        .filter(|c| c.sync.is_some())
        .ok_or_else(|| OperationError::NotFound("DICOM sync source is not configured".into()))?;
    let sync = source.sync.as_ref().unwrap();
    let auth = context
        .auth()
        .filter(|auth| {
            auth.client.client_id == sync.client_id
                && auth.user.is_none()
                && auth.patient.is_none()
                && sync.scopes.iter().all(|scope| auth.has_scope(scope))
        })
        .ok_or_else(|| {
            ApiError::forbidden(
                "DICOM control-plane operations require the configured backend identity and scopes",
            )
        })?;
    if !auth.client.active {
        return Err(ApiError::forbidden("DICOM backend client is inactive").into());
    }
    Ok(source)
}

#[async_trait]
impl OperationHandler for DicomNotify {
    fn code(&self) -> &str {
        "dicom-notify"
    }
    fn supports_level(&self, level: OperationLevel, _: Option<&str>) -> bool {
        level == OperationLevel::System
    }
    async fn handle_system(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if context.is_get() {
            return Err(OperationError::InvalidParameters(
                "Use POST for DICOM notifications".into(),
            ));
        }
        let params = strings(params, &["connection", "study", "change", "sequence"])?;
        let connection = required(&params, "connection")?;
        let config = source(state, context, connection)?;
        let study = required(&params, "study")?;
        crate::dicom::validate_uid(study).map_err(super::dicom::dicom_error)?;
        let sequence = required(&params, "sequence")?
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| {
                OperationError::InvalidParameters(
                    "sequence must be a nonnegative decimal integer string".into(),
                )
            })?;
        let change: store::Change = serde_json::from_value(json!(required(&params, "change")?))
            .map_err(|_| {
                OperationError::InvalidParameters(
                    "change must be changed, deleted, rejected or restored".into(),
                )
            })?;
        store::initialize(&state.db_pool, connection, &store::fingerprint(config))
            .await
            .map_err(OperationError::Internal)?;
        let mut tx =
            state.db_pool.begin().await.map_err(|_| {
                OperationError::Internal("Cannot persist DICOM notification".into())
            })?;
        let accepted = store::enqueue(&mut tx, connection, study, None, Some(sequence), change)
            .await
            .map_err(OperationError::Internal)?;
        tx.commit()
            .await
            .map_err(|_| OperationError::Internal("Cannot commit DICOM notification".into()))?;
        Ok(
            json!({"resourceType":"Parameters","parameter":[{"name":"accepted","valueBoolean":accepted},{"name":"study","valueString":study}]}),
        )
    }
}

#[async_trait]
impl OperationHandler for DicomSyncStatus {
    fn code(&self) -> &str {
        "dicom-sync-status"
    }
    fn supports_level(&self, level: OperationLevel, _: Option<&str>) -> bool {
        level == OperationLevel::System
    }
    async fn handle_system(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let params = strings(params, &["connection"])?;
        let connection = required(&params, "connection")?;
        source(state, context, connection)?;
        let row=query("SELECT COUNT(*)::bigint AS studies,COUNT(*) FILTER(WHERE desired_revision>applied_revision)::bigint AS pending,COUNT(*) FILTER(WHERE last_error IS NOT NULL)::bigint AS failed FROM dicom_sync_studies WHERE connection_name=$1")
            .bind(connection).fetch_one(state.db_pool.as_ref()).await.map_err(|_|OperationError::Internal("Cannot read DICOM sync status".into()))?;
        let scan=query("SELECT scan_offset,last_error,last_scan_at FROM dicom_sync_sources WHERE connection_name=$1").bind(connection).fetch_optional(state.db_pool.as_ref()).await.map_err(|_|OperationError::Internal("Cannot read DICOM scan status".into()))?;
        let mut parameters = vec![];
        for key in ["studies", "pending", "failed"] {
            parameters.push(json!({"name":key,"valueInteger":row.get::<i64,_>(key)}));
        }
        if let Some(scan) = scan {
            parameters.push(json!({"name":"scanOffset","valueString":scan.get::<i64,_>("scan_offset").to_string()}));
            if let Some(error) = scan.get::<Option<String>, _>("last_error") {
                parameters.push(json!({"name":"scanError","valueString":error}));
            }
            if let Some(at) = scan.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_scan_at") {
                parameters.push(json!({"name":"lastScan","valueInstant":at.to_rfc3339()}));
            }
        }
        Ok(json!({"resourceType":"Parameters","parameter":parameters}))
    }
}

#[async_trait]
impl OperationHandler for DicomDiscover {
    fn code(&self) -> &str {
        "dicom-discover"
    }
    fn supports_level(&self, level: OperationLevel, kind: Option<&str>) -> bool {
        level == OperationLevel::Instance && kind == Some("Patient")
    }
    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        _: &str,
        patient_id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let params = strings(params, &["connection", "issuer", "offset", "limit"])?;
        let connection = required(&params, "connection")?;
        let config = source(state, context, connection)?;
        let patient = context
            .read_primary(state, "Patient", patient_id)
            .await?
            .ok_or_else(|| OperationError::NotFound("Patient is unavailable".into()))?;
        let issuer = params
            .get("issuer")
            .copied()
            .or(config.default_patient_issuer.as_deref())
            .or_else(|| {
                (config.patient_identifier_systems.len() == 1).then(|| {
                    config
                        .patient_identifier_systems
                        .keys()
                        .next()
                        .unwrap()
                        .as_str()
                })
            })
            .ok_or_else(|| {
                OperationError::InvalidParameters(
                    "Choose a configured issuer for patient discovery".into(),
                )
            })?;
        let system = config
            .patient_identifier_systems
            .get(issuer)
            .ok_or_else(|| OperationError::InvalidParameters("issuer is not configured".into()))?;
        let identifiers: Vec<_> = patient.resource["identifier"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|id| id["system"] == *system)
            .filter_map(|id| id["value"].as_str())
            .collect();
        if identifiers.len() != 1 {
            return Err(OperationError::InvalidParameters(
                "Patient must have one identifier for the selected assigning authority".into(),
            ));
        }
        let offset = params
            .get("offset")
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_| OperationError::InvalidParameters("Invalid offset".into()))?
            .unwrap_or(0);
        let limit = params
            .get("limit")
            .map(|value| value.parse::<u32>())
            .transpose()
            .map_err(|_| OperationError::InvalidParameters("Invalid limit".into()))?
            .unwrap_or(100);
        if !(1..=1000).contains(&limit) || offset > 10_000_000 {
            return Err(OperationError::InvalidParameters(
                "Discovery page exceeds configured bounds".into(),
            ));
        }
        let client = DicomWebClient::new(
            config,
            Arc::new(
                crate::egress::EgressPolicy::from_server_config(&state.config.server)
                    .map_err(OperationError::Internal)?,
            ),
        )
        .map_err(super::dicom::dicom_error)?;
        let records = client
            .query_studies(offset, limit, Some((identifiers[0], issuer)))
            .await
            .map_err(super::dicom::dicom_error)?;
        let mut parameters = vec![
            json!({"name":"nextOffset","valueString":(offset+records.len() as u64).to_string()}),
            json!({"name":"complete","valueBoolean":records.is_empty()}),
        ];
        for record in records {
            let identity = crate::dicom::mapping::patient_identifier(&record.dataset, config)
                .map_err(super::dicom::dicom_error)?;
            if identity.0 != *system || identity.1 != identifiers[0] {
                return Err(ApiError::forbidden(
                    "PACS discovery response does not match the authorized patient",
                )
                .into());
            }
            let mut parts = vec![json!({"name":"uid","valueString":record.uid})];
            if let Some(instances) = record.instances {
                parts.push(json!({"name":"instances","valueInteger":instances}));
            }
            parameters.push(json!({"name":"study","part":parts}));
        }
        Ok(json!({"resourceType":"Parameters","parameter":parameters}))
    }
}
