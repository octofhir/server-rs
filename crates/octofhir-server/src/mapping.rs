use std::collections::HashMap;

use octofhir_core::{ResourceEnvelope, ResourceMeta, ResourceType, generate_id, now_utc};
use serde_json::{Map, Value};

/// ID handling policy for mapping incoming JSON into an envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdPolicy {
    /// Create: body `id` is optional; generate one if missing.
    Create,
    /// Update: URL id is authoritative; if body `id` is present, it must match.
    Update { path_id: String },
}

/// Convert a FHIR JSON object into a ResourceEnvelope, applying minimal rules.
/// - Validates `resourceType` presence and exact match with `expected_type`
/// - Applies `IdPolicy` for `id` handling
/// - Updates `meta.lastUpdated` to now; preserves other meta fields if present
/// - Flattens remaining JSON members into `data`
pub fn envelope_from_json(
    expected_type: &str,
    json: &Value,
    policy: IdPolicy,
) -> Result<ResourceEnvelope, String> {
    let obj = json
        .as_object()
        .ok_or_else(|| "body must be a JSON object".to_string())?;

    // resourceType
    let rt_str = obj
        .get("resourceType")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing resourceType".to_string())?;
    if rt_str != expected_type {
        return Err(format!(
            "resourceType '{rt_str}' does not match path type '{expected_type}'"
        ));
    }
    let resource_type = rt_str
        .parse::<ResourceType>()
        .map_err(|_| format!("invalid resourceType '{rt_str}'"))?;

    // id resolution
    let body_id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let id = match policy {
        IdPolicy::Create => body_id.unwrap_or_else(generate_id),
        IdPolicy::Update { path_id } => {
            if let Some(bid) = body_id
                && bid != path_id
            {
                return Err(format!(
                    "id in body '{bid}' does not match URL id '{path_id}'"
                ));
            }
            path_id
        }
    };

    // meta handling: merge if present; always update lastUpdated
    let mut meta = if let Some(meta_val) = obj.get("meta").and_then(|v| v.as_object()) {
        let mut m = ResourceMeta::default();
        if let Some(ver) = meta_val.get("versionId").and_then(|v| v.as_str()) {
            m.version_id = Some(ver.to_string());
        }
        if let Some(profile) = meta_val.get("profile").and_then(|v| v.as_array()) {
            m.profile = profile
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(src) = meta_val.get("source").and_then(|v| v.as_str()) {
            m.source = Some(src.to_string());
        }
        // security/tag are left as raw values when present
        if let Some(sec) = meta_val.get("security").and_then(|v| v.as_array()) {
            m.security = sec.clone();
        }
        if let Some(tag) = meta_val.get("tag").and_then(|v| v.as_array()) {
            m.tag = tag.clone();
        }
        m
    } else {
        ResourceMeta::default()
    };
    meta.last_updated = now_utc();

    // Flatten remaining fields into data (exclude reserved keys)
    let mut data: HashMap<String, Value> = HashMap::new();
    for (k, v) in obj.iter() {
        if matches!(k.as_str(), "resourceType" | "id" | "meta") {
            continue;
        }
        data.insert(k.clone(), v.clone());
    }

    let env = ResourceEnvelope::new(id, resource_type)
        .with_meta(meta)
        .with_data(data);
    Ok(env)
}

/// Validate payload structure without allocating a full ResourceEnvelope.
///
/// This is a zero-allocation check that verifies:
/// - body is a JSON object
/// - `resourceType` is present and matches `expected_type`
/// - For Update policy: body `id` (if present) matches the path id
///
/// Use this instead of `envelope_from_json` when you only need validation
/// (the result ResourceEnvelope is discarded anyway).
pub fn validate_payload_structure(
    expected_type: &str,
    json: &Value,
    policy: &IdPolicy,
) -> Result<(), String> {
    let obj = json
        .as_object()
        .ok_or_else(|| "body must be a JSON object".to_string())?;

    let rt_str = obj
        .get("resourceType")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing resourceType".to_string())?;
    if rt_str != expected_type {
        return Err(format!(
            "resourceType '{rt_str}' does not match path type '{expected_type}'"
        ));
    }

    // Validate resourceType is parseable
    rt_str
        .parse::<ResourceType>()
        .map_err(|_| format!("invalid resourceType '{rt_str}'"))?;

    // Validate id policy (no allocation for common case)
    if let IdPolicy::Update { path_id } = policy {
        if let Some(bid) = obj.get("id").and_then(|v| v.as_str()) {
            if bid != path_id {
                return Err(format!(
                    "id in body '{bid}' does not match URL id '{path_id}'"
                ));
            }
        }
    }

    Ok(())
}

/// Convert a ResourceEnvelope back into FHIR JSON object.
pub fn json_from_envelope(env: &ResourceEnvelope) -> Value {
    let mut map = Map::new();
    map.insert(
        "resourceType".to_string(),
        Value::String(env.resource_type.to_string()),
    );
    map.insert("id".to_string(), Value::String(env.id.clone()));
    // Serialize meta via serde; ensure lastUpdated is present
    if let Ok(meta_json) = serde_json::to_value(&env.meta) {
        map.insert("meta".to_string(), meta_json);
    }
    // Flatten data
    for (k, v) in env.data.iter() {
        // avoid clobbering reserved keys if present in data
        if matches!(k.as_str(), "resourceType" | "id" | "meta") {
            continue;
        }
        map.insert(k.clone(), v.clone());
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_generates_id_and_sets_meta() {
        let body = json!({
            "resourceType": "Patient",
            "name": [{"family": "Doe", "given": ["Jane"]}],
        });
        let env = envelope_from_json("Patient", &body, IdPolicy::Create).expect("map");
        assert_eq!(env.resource_type.to_string(), "Patient");
        assert!(!env.id.is_empty());
        assert!(env.meta.last_updated.timestamp() > 0);
        assert!(env.get_field("name").is_some());

        let out = json_from_envelope(&env);
        assert_eq!(out["resourceType"], "Patient");
        assert_eq!(out["id"].as_str().unwrap(), env.id);
        assert!(out.get("meta").is_some());
    }

    #[test]
    fn update_uses_path_id_and_validates_body_id() {
        let body = json!({
            "resourceType": "Patient",
            "id": "abc",
            "active": true
        });
        // mismatch should error
        let err = envelope_from_json(
            "Patient",
            &body,
            IdPolicy::Update {
                path_id: "xyz".into(),
            },
        )
        .unwrap_err();
        assert!(err.contains("does not match"));

        // match should succeed and set id=path
        let env = envelope_from_json(
            "Patient",
            &body,
            IdPolicy::Update {
                path_id: "abc".into(),
            },
        )
        .expect("map");
        assert_eq!(env.id, "abc");
        assert_eq!(
            env.get_field("active").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn wrong_resource_type_is_rejected() {
        let body = json!({"resourceType": "Observation"});
        let err = envelope_from_json("Patient", &body, IdPolicy::Create).unwrap_err();
        assert!(err.contains("does not match"));
    }

    #[test]
    fn validate_payload_structure_accepts_valid_create() {
        let body = json!({"resourceType": "Patient", "name": [{"family": "Doe"}]});
        assert!(validate_payload_structure("Patient", &body, &IdPolicy::Create).is_ok());
    }

    #[test]
    fn validate_payload_structure_rejects_wrong_type() {
        let body = json!({"resourceType": "Observation"});
        let err = validate_payload_structure("Patient", &body, &IdPolicy::Create).unwrap_err();
        assert!(err.contains("does not match"));
    }

    #[test]
    fn validate_payload_structure_rejects_id_mismatch() {
        let body = json!({"resourceType": "Patient", "id": "abc"});
        let err = validate_payload_structure(
            "Patient",
            &body,
            &IdPolicy::Update {
                path_id: "xyz".into(),
            },
        )
        .unwrap_err();
        assert!(err.contains("does not match"));
    }

    #[test]
    fn validate_payload_structure_accepts_matching_id() {
        let body = json!({"resourceType": "Patient", "id": "abc"});
        assert!(
            validate_payload_structure(
                "Patient",
                &body,
                &IdPolicy::Update {
                    path_id: "abc".into()
                },
            )
            .is_ok()
        );
    }
}
