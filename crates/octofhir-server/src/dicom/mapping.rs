//! DICOM JSON to FHIR study and endpoint projection.
//!
//! # Contents
//! Strict scalar/UID decoding, assigning-authority checks, stable identities and
//! deterministic study/series/instance grouping for FHIR R4, R4B and R5.
//!
//! # Invariants
//! Every instance belongs to the requested study and the selected existing patient.
//! No raw datasets, bulk-data URIs, inline binaries or pixel attributes are persisted.
//! Connection-scoped identities never merge unrelated archive representations.

use std::collections::{BTreeMap, BTreeSet};

use octofhir_core::fhir::FhirVersion;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{DicomConnection, DicomError, validate_uid};

pub(crate) const SOURCE_TAG: &str = "https://octofhir.org/CodeSystem/dicom-source";

pub(crate) fn resource_id(connection: &str, kind: &str, uid: &str) -> String {
    let mut hash = Sha256::new();
    for part in [connection, kind, uid] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!("dicom-{}", hex::encode(hash.finalize()))[..60].to_owned()
}

pub(super) fn scalar<'a>(
    dataset: &'a Value,
    tag: &str,
    vr: &str,
) -> Result<Option<&'a str>, DicomError> {
    let Some(attribute) = dataset.get(tag) else {
        return Ok(None);
    };
    if attribute["vr"].as_str() != Some(vr)
        || attribute.get("InlineBinary").is_some()
        || attribute.get("BulkDataURI").is_some()
    {
        return Err(DicomError::Metadata(format!(
            "invalid representation for {tag}"
        )));
    }
    let Some(values) = attribute.get("Value") else {
        return Ok(None);
    };
    let values = values
        .as_array()
        .ok_or_else(|| DicomError::Metadata(format!("invalid values for {tag}")))?;
    match values.as_slice() {
        [] => Ok(None),
        [Value::String(value)] => Ok((!value.trim().is_empty()).then_some(value.as_str())),
        _ => Err(DicomError::Metadata(format!(
            "expected one string for {tag}"
        ))),
    }
}

pub(super) fn required<'a>(dataset: &'a Value, tag: &str, vr: &str) -> Result<&'a str, DicomError> {
    scalar(dataset, tag, vr)?.ok_or_else(|| DicomError::Metadata(format!("missing {tag}")))
}

fn uid<'a>(dataset: &'a Value, tag: &str) -> Result<&'a str, DicomError> {
    let value = required(dataset, tag, "UI")?;
    validate_uid(value)?;
    Ok(value)
}

pub(crate) fn patient_identifier(
    dataset: &Value,
    config: &DicomConnection,
) -> Result<(String, String), DicomError> {
    let id = required(dataset, "00100020", "LO")?;
    let issuer = scalar(dataset, "00100021", "LO")?
        .or(config.default_patient_issuer.as_deref())
        .ok_or_else(|| DicomError::Metadata("patient assigning authority is unavailable".into()))?;
    let system = config
        .patient_identifier_systems
        .get(issuer)
        .ok_or_else(|| {
            DicomError::Metadata("patient assigning authority is not configured".into())
        })?;
    Ok((system.clone(), id.to_owned()))
}

pub(crate) fn patient_matches(
    dataset: &Value,
    patient: &Value,
    config: &DicomConnection,
) -> Result<(), DicomError> {
    let (system, id) = patient_identifier(dataset, config)?;
    let matches = patient["identifier"].as_array().is_some_and(|identifiers| {
        identifiers
            .iter()
            .any(|identifier| identifier["system"] == system && identifier["value"] == id)
    });
    if !matches {
        return Err(DicomError::Metadata(
            "study does not match the selected patient identifier".into(),
        ));
    }
    Ok(())
}

/// FHIR represents the SOP Class as a URI Coding; DICOMweb requires its raw UID.
/// R4, R4B and R5 all use Coding for this particular instance field.
pub(crate) fn sop_class(instance: &Value) -> Result<&str, DicomError> {
    let coding = &instance["sopClass"];
    if coding["system"] != "urn:ietf:rfc:3986" {
        return Err(DicomError::Metadata(
            "Invalid SOP Class coding system".into(),
        ));
    }
    let uid = coding["code"]
        .as_str()
        .and_then(|code| code.strip_prefix("urn:oid:"))
        .ok_or_else(|| DicomError::Metadata("SOP Class requires an OID URI".into()))?;
    validate_uid(uid)?;
    Ok(uid)
}

fn coding(modality: &str, release: FhirVersion) -> Value {
    let coding = json!({"system":"http://dicom.nema.org/resources/ontology/DCM", "code":modality});
    if release == FhirVersion::R5 {
        json!({"coding":[coding]})
    } else {
        coding
    }
}

pub(crate) fn endpoint(connection: &str, config: &DicomConnection, release: FhirVersion) -> Value {
    let protocol = json!({"system":"http://terminology.hl7.org/CodeSystem/endpoint-connection-type", "code":"dicom-wado-rs"});
    let mut endpoint = json!({
        "resourceType":"Endpoint", "id":resource_id(connection,"endpoint",""),
        "meta":{"tag":[{"system":SOURCE_TAG, "code":connection}]},
        "status":"active", "name":connection,
        "address":config.wado_url.trim_end_matches('/')
    });
    if release == FhirVersion::R5 {
        endpoint["connectionType"] = json!([{"coding":[protocol]}]);
        endpoint["payload"] = json!([{"mimeType":["application/dicom"]}]);
    } else {
        endpoint["connectionType"] = protocol;
        endpoint["payloadType"] = json!([{"text":"DICOM instances"}]);
        endpoint["payloadMimeType"] = json!(["application/dicom"]);
    }
    endpoint
}

pub(crate) fn study(
    connection: &str,
    config: &DicomConnection,
    release: FhirVersion,
    study_uid: &str,
    patient_id: &str,
    patient: &Value,
    datasets: &[Value],
) -> Result<Value, DicomError> {
    validate_uid(study_uid)?;
    if datasets.is_empty() {
        return Err(DicomError::Metadata("study metadata is empty".into()));
    }
    if datasets.len() > config.max_instances {
        return Err(DicomError::Limit);
    }
    let first = &datasets[0];
    let description = scalar(first, "00081030", "LO")?;
    let date = scalar(first, "00080020", "DA")?;
    let mut groups: BTreeMap<String, (String, BTreeMap<String, Value>)> = BTreeMap::new();
    let mut seen_instances = BTreeSet::new();
    let mut modalities = BTreeSet::new();
    for dataset in datasets {
        if !dataset.is_object() {
            return Err(DicomError::Metadata(
                "instance dataset must be an object".into(),
            ));
        }
        if uid(dataset, "0020000D")? != study_uid {
            return Err(DicomError::Metadata(
                "response contains another study".into(),
            ));
        }
        patient_matches(dataset, patient, config)?;
        if scalar(dataset, "00081030", "LO")? != description
            || scalar(dataset, "00080020", "DA")? != date
        {
            return Err(DicomError::Metadata("inconsistent study attributes".into()));
        }
        let series_uid = uid(dataset, "0020000E")?;
        let instance_uid = uid(dataset, "00080018")?;
        let sop_class = uid(dataset, "00080016")?;
        if !seen_instances.insert(instance_uid.to_owned()) {
            return Err(DicomError::Metadata("duplicate SOP Instance UID".into()));
        }
        let modality = required(dataset, "00080060", "CS")?;
        if modality.len() > 16
            || !modality
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ' || b == b'_')
        {
            return Err(DicomError::Metadata("invalid modality".into()));
        }
        modalities.insert(modality.to_owned());
        let (series_modality, instances) = groups
            .entry(series_uid.to_owned())
            .or_insert_with(|| (modality.to_owned(), BTreeMap::new()));
        if series_modality != modality {
            return Err(DicomError::Metadata("inconsistent series modality".into()));
        }
        instances.insert(
            instance_uid.to_owned(),
            json!({
                "uid":instance_uid,
                "sopClass":{"system":"urn:ietf:rfc:3986", "code":format!("urn:oid:{sop_class}")}
            }),
        );
    }
    let series: Vec<Value> = groups.into_iter().map(|(uid,(modality,instances))| json!({
        "uid":uid, "modality":coding(&modality,release),
        "numberOfInstances":instances.len(), "instance":instances.into_values().collect::<Vec<_>>()
    })).collect();
    let mut study = json!({
        "resourceType":"ImagingStudy", "id":resource_id(connection,"study",study_uid),
        "meta":{"tag":[{"system":SOURCE_TAG, "code":connection}]},
        "identifier":[{"system":"urn:dicom:uid", "value":format!("urn:oid:{study_uid}")}],
        "status":"available", "subject":{"reference":format!("Patient/{patient_id}")},
        "endpoint":[{"reference":format!("Endpoint/{}",resource_id(connection,"endpoint",""))}],
        "numberOfSeries":series.len(), "numberOfInstances":datasets.len(),
        "modality":modalities.iter().map(|m| coding(m,release)).collect::<Vec<_>>(), "series":series
    });
    if let Some(description) = description {
        study["description"] = json!(description);
    }
    // A date is truthful without inventing a timezone for a DICOM local clock.
    if let Some(date) = date {
        if date.len() != 8 || !date.bytes().all(|b| b.is_ascii_digit()) {
            return Err(DicomError::Metadata("invalid Study Date".into()));
        }
        let parsed = chrono::NaiveDate::parse_from_str(date, "%Y%m%d")
            .map_err(|_| DicomError::Metadata("invalid Study Date".into()))?;
        study["started"] = json!(parsed.format("%Y-%m-%d").to_string());
    }
    Ok(study)
}

/// Preserve user-owned fields, metadata/security labels and unrelated identifiers.
/// Only explicitly marked resources can be updated by this importer.
pub(crate) fn merge(
    previous: &Value,
    incoming: &Value,
    connection: &str,
) -> Result<Value, DicomError> {
    if !previous["meta"]["tag"].as_array().is_some_and(|tags| {
        tags.iter()
            .any(|tag| tag["system"] == SOURCE_TAG && tag["code"] == connection)
    }) {
        return Err(DicomError::Metadata(
            "target resource is not owned by this DICOM connection".into(),
        ));
    }
    if previous["resourceType"] != incoming["resourceType"]
        || previous["id"] != incoming["id"]
        || (incoming["resourceType"] == "ImagingStudy"
            && previous["subject"] != incoming["subject"])
    {
        return Err(DicomError::Metadata(
            "import would change the resource or patient identity".into(),
        ));
    }
    if incoming["resourceType"] == "ImagingStudy" {
        let previous_uids: Vec<_> = previous["identifier"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|identifier| identifier["system"] == "urn:dicom:uid")
            .collect();
        if previous_uids.len() != 1
            || previous_uids[0]["value"] != incoming["identifier"][0]["value"]
        {
            return Err(DicomError::Metadata(
                "import would change the DICOM study identity".into(),
            ));
        }
    }
    let fields: &[&str] = if incoming["resourceType"] == "ImagingStudy" {
        &[
            "status",
            "subject",
            "endpoint",
            "numberOfSeries",
            "numberOfInstances",
            "modality",
            "series",
            "description",
            "started",
        ]
    } else {
        &[
            "status",
            "name",
            "address",
            "connectionType",
            "payloadType",
            "payloadMimeType",
            "payload",
        ]
    };
    let mut merged = previous.clone();
    let object = merged
        .as_object_mut()
        .ok_or_else(|| DicomError::Metadata("invalid existing resource".into()))?;
    for key in fields {
        if let Some(value) = incoming.get(*key) {
            object.insert((*key).into(), value.clone());
        } else {
            object.remove(*key);
        }
    }
    if incoming["resourceType"] == "ImagingStudy" {
        let mut extensions = merged["extension"].as_array().cloned().unwrap_or_default();
        extensions.retain(|extension| extension["url"] != super::import::AVAILABILITY);
        extensions.extend(
            incoming["extension"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|extension| extension["url"] == super::import::AVAILABILITY)
                .cloned(),
        );
        if !extensions.is_empty() {
            merged["extension"] = json!(extensions);
        } else {
            merged.as_object_mut().unwrap().remove("extension");
        }
    }
    if incoming["resourceType"] == "ImagingStudy" {
        super::security::inherit_security(&mut merged, &[incoming]);
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn dataset() -> Value {
        json!({
            "0020000D":{"vr":"UI","Value":["1.2.3"]},
            "0020000E":{"vr":"UI","Value":["1.2.3.1"]},
            "00080018":{"vr":"UI","Value":["1.2.3.1.1"]},
            "00080016":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.2"]},
            "00080060":{"vr":"CS","Value":["CT"]},
            "00100020":{"vr":"LO","Value":["MRN-1"]},
            "00100021":{"vr":"LO","Value":["HOSPITAL"]},
            "00080020":{"vr":"DA","Value":["20261108"]},
            "7FE00010":{"vr":"OW","BulkDataURI":"https://pacs.example/pixels"}
        })
    }
    fn connection() -> DicomConnection {
        serde_json::from_value(json!({"wado_url":"https://pacs.example/dicom",
            "patient_identifier_systems":{"HOSPITAL":"urn:oid:1.2.3"}}))
        .unwrap()
    }
    fn patient() -> Value {
        json!({"identifier":[{"system":"urn:oid:1.2.3","value":"MRN-1"}]})
    }

    #[test]
    fn release_mapping_is_deterministic_and_never_copies_bulk_data() {
        let mut second = dataset();
        second["00080018"]["Value"] = json!(["1.2.3.1.2"]);
        for release in [FhirVersion::R4, FhirVersion::R4B, FhirVersion::R5] {
            let config = connection();
            let result = study(
                "pacs",
                &config,
                release,
                "1.2.3",
                "p1",
                &patient(),
                &[dataset(), second.clone()],
            )
            .unwrap();
            let reverse = study(
                "pacs",
                &config,
                release,
                "1.2.3",
                "p1",
                &patient(),
                &[second.clone(), dataset()],
            )
            .unwrap();
            assert_eq!(result, reverse);
            assert_eq!(result["numberOfInstances"], 2);
            assert_eq!(result["started"], "2026-11-08");
            assert_eq!(
                result["series"][0]["instance"][0]["sopClass"]["code"],
                "urn:oid:1.2.840.10008.5.1.4.1.1.2"
            );
            assert!(!result.to_string().contains("BulkDataURI"));
            assert!(!result.to_string().contains("pixels"));
            let ep = endpoint("pacs", &config, release);
            if release == FhirVersion::R5 {
                assert_eq!(result["series"][0]["modality"]["coding"][0]["code"], "CT");
                assert!(ep["connectionType"].is_array());
                assert!(ep["payload"].is_array());
            } else {
                assert_eq!(result["series"][0]["modality"]["code"], "CT");
                assert!(ep["connectionType"].is_object());
                assert!(ep["payloadType"].is_array());
            }
        }
    }

    #[test]
    fn refuses_wrong_patients_mixed_studies_and_duplicate_instances() {
        for (tag, value) in [
            ("00100020", "OTHER"),
            ("00100021", "OTHER"),
            ("0020000D", "9.8.7"),
            ("0020000E", "../x"),
        ] {
            let mut bad = dataset();
            bad[tag]["Value"] = json!([value]);
            assert!(
                study(
                    "pacs",
                    &connection(),
                    FhirVersion::R4,
                    "1.2.3",
                    "p1",
                    &patient(),
                    &[bad]
                )
                .is_err()
            );
        }
        assert!(
            study(
                "pacs",
                &connection(),
                FhirVersion::R4,
                "1.2.3",
                "p1",
                &patient(),
                &[dataset(), dataset()]
            )
            .is_err()
        );
        let mut bad = dataset();
        bad.as_object_mut().unwrap().remove("00100021");
        assert!(
            study(
                "pacs",
                &connection(),
                FhirVersion::R4,
                "1.2.3",
                "p1",
                &patient(),
                &[bad.clone()]
            )
            .is_err()
        );
        let mut configured = connection();
        configured.default_patient_issuer = Some("HOSPITAL".into());
        assert!(
            study(
                "pacs",
                &configured,
                FhirVersion::R4,
                "1.2.3",
                "p1",
                &patient(),
                &[bad]
            )
            .is_ok()
        );
    }

    #[test]
    fn merge_preserves_clinical_annotations_and_rejects_patient_reassignment() {
        let config = connection();
        let incoming = study(
            "pacs",
            &config,
            FhirVersion::R4,
            "1.2.3",
            "p1",
            &patient(),
            &[dataset()],
        )
        .unwrap();
        let mut previous = incoming.clone();
        previous["note"] = json!([{"text":"clinical note"}]);
        previous["meta"]["security"] = json!([{"code":"R"}]);
        assert_eq!(merge(&previous, &incoming, "pacs").unwrap(), previous);
        let mut changed = incoming.clone();
        changed["subject"]["reference"] = json!("Patient/p2");
        assert!(merge(&previous, &changed, "pacs").is_err());
        assert!(merge(&json!({"id":"occupied"}), &incoming, "pacs").is_err());
    }
}
