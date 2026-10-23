//! Current, explicit Patient membership shared by patient-record operations.

use octofhir_core::fhir_reference::{FhirReference, parse_reference};
use serde_json::Value;
use time::OffsetDateTime;

use super::OperationError;

pub(crate) fn active_members(
    group: &Value,
    fhir_base: &str,
    now: OffsetDateTime,
) -> Result<Vec<FhirReference>, OperationError> {
    if group["type"] != "person" || group["actual"] != true {
        return Err(OperationError::NotSupported(
            "Patient records require an actual person Group with explicit members".into(),
        ));
    }
    if group["active"] == false {
        return Ok(Vec::new());
    }
    let mut patients = Vec::new();
    for member in group["member"].as_array().into_iter().flatten() {
        if member["inactive"] == true {
            continue;
        }
        let boundary = |name: &str| -> Result<
            Option<octofhir_search::types::date::DateRange>,
            OperationError,
        > {
            member["period"][name]
                .as_str()
                .map(|value| {
                    octofhir_search::types::date::parse_date_range(value).map_err(|error| {
                        OperationError::InvalidParameters(format!(
                            "Invalid Group member period: {error}"
                        ))
                    })
                })
                .transpose()
        };
        if boundary("start")?.is_some_and(|start| now < start.start)
            || boundary("end")?.is_some_and(|end| now >= end.end)
        {
            continue;
        }
        let Some(reference) = member["entity"]["reference"].as_str() else {
            return Err(OperationError::NotSupported(
                "Group members require literal Patient references".into(),
            ));
        };
        let parsed = parse_reference(reference, Some(fhir_base)).map_err(|_| {
            OperationError::NotSupported("Group member is not a local Patient reference".into())
        })?;
        if !matches!(parsed.resource_type.as_str(), "Patient" | "Group") {
            return Err(OperationError::NotSupported(
                "Person Group members must reference Patient or Group".into(),
            ));
        }
        // A member reference identifies a person, including a version-specific
        // reference. Record retrieval uses that person's current authorized record.
        patients.push(parsed);
    }
    patients.sort_by(|left, right| {
        (&left.resource_type, &left.id).cmp(&(&right.resource_type, &right.id))
    });
    patients
        .dedup_by(|left, right| left.resource_type == right.resource_type && left.id == right.id);
    Ok(patients)
}

pub(crate) struct GroupMembership {
    pub groups: Vec<octofhir_storage::StoredResource>,
    pub patient_ids: Vec<String>,
}

/// Resolve explicit nested membership in grouped reads. Membership only selects
/// records; the export/everything queries still enforce each resource's grants.
pub(crate) async fn resolve_membership(
    state: &crate::server::AppState,
    context: &super::OperationContext,
    group_id: &str,
    now: OffsetDateTime,
) -> Result<GroupMembership, OperationError> {
    use std::collections::HashSet;

    let root = context
        .read(state, "Group", group_id)
        .await?
        .ok_or_else(|| OperationError::NotFound(format!("Group/{group_id} not found")))?;
    let limits = state.config.search.include_limits;
    let mut bytes = serde_json::to_vec(&root.resource)
        .map_err(|error| OperationError::Internal(error.to_string()))?
        .len();
    let mut groups = vec![root];
    let mut visited = HashSet::from([group_id.to_owned()]);
    let mut members = HashSet::new();
    let mut frontier = 0..1;
    let mut depth = 0usize;
    loop {
        let mut nested = Vec::new();
        for group in &groups[frontier] {
            for member in active_members(&group.resource, &state.fhir_base_url(), now)? {
                if member.resource_type == "Patient" {
                    members.insert(member.id);
                } else if visited.insert(member.id.clone()) {
                    nested.push(member.id);
                }
            }
        }
        if members.len().saturating_add(visited.len()) > limits.max_resources
            || bytes > limits.max_bytes
        {
            return Err(OperationError::InvalidParameters(
                "Group membership exceeds the configured resource budget".into(),
            ));
        }
        if nested.is_empty() {
            break;
        }
        if depth >= limits.max_depth {
            return Err(OperationError::InvalidParameters(
                "Group membership exceeds the configured depth budget".into(),
            ));
        }
        let first = groups.len();
        nested.sort();
        for ids in nested.chunks(1000) {
            let found = context.read_many(state, "Group", ids).await?;
            if found.len() != ids.len() {
                return Err(OperationError::NotFound(
                    "A nested Group is missing or unavailable".into(),
                ));
            }
            for group in found {
                bytes = bytes.saturating_add(
                    serde_json::to_vec(&group.resource)
                        .map_err(|error| OperationError::Internal(error.to_string()))?
                        .len(),
                );
                if bytes > limits.max_bytes {
                    return Err(OperationError::InvalidParameters(
                        "Group membership exceeds the configured byte budget".into(),
                    ));
                }
                groups.push(group);
            }
        }
        frontier = first..groups.len();
        depth += 1;
    }
    let mut patient_ids: Vec<_> = members.into_iter().collect();
    patient_ids.sort();
    Ok(GroupMembership {
        groups,
        patient_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn active_members_use_local_identity_and_period_precision() {
        let group = json!({"type":"person","actual":true,"member":[
            {"entity":{"reference":"Patient/one"}},
            {"entity":{"reference":"https://example.test/fhir/Patient/one/_history/7"}},
            {"entity":{"reference":"Patient/inactive"},"inactive":true},
            {"entity":{"reference":"Patient/expired"},"period":{"end":"2024"}},
            {"entity":{"reference":"Patient/future"},"period":{"start":"2026"}},
            {"entity":{"reference":"Patient/current"},"period":{"start":"2025","end":"2025"}}
        ]});
        let now = octofhir_search::types::date::parse_date_range("2025-07-01")
            .unwrap()
            .start;
        assert_eq!(
            active_members(&group, "http://example.test/fhir", now)
                .unwrap()
                .into_iter()
                .map(|member| member.id)
                .collect::<Vec<_>>(),
            vec!["current", "one"]
        );
        let external = json!({"type":"person","actual":true,"member":[{"entity":{"reference":"http://foreign.test/fhir/Patient/one"}}]});
        assert!(active_members(&external, "http://example.test/fhir", now).is_err());
        let nested = json!({"type":"person","actual":true,"member":[{"entity":{"reference":"Group/nested"}}]});
        assert_eq!(
            active_members(&nested, "http://example.test/fhir", now).unwrap()[0].resource_type,
            "Group"
        );
    }
}
