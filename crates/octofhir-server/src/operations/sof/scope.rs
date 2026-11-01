//! The data filters of the SQL-on-FHIR operations.
//!
//! `patient`, `group` and `_since` constrain the FHIR resources that feed a
//! view before projection (SQL on FHIR, Common Operation Behavior,
//! "Filtering"). They become search parameters and a Patient-compartment
//! constraint, so the SQL path and the in-memory path apply them alike, on top
//! of the caller's own grants.

use octofhir_storage::{SearchConstraint, SearchParams};
use serde_json::Value;

use crate::operations::{OperationContext, OperationError};
use crate::server::AppState;

/// The filters of one request.
#[derive(Debug, Default)]
pub(crate) struct ViewScope {
    patients: Vec<String>,
    groups: Vec<String>,
    since: Option<String>,
}

impl ViewScope {
    /// Read `patient` (0..*), `group` (0..*) and `_since` (0..1) from a
    /// Parameters resource; a GET request carries them as strings.
    pub(crate) fn from_parameters(params: &Value) -> Result<Self, OperationError> {
        let mut scope = Self::default();
        let mut since = Vec::new();
        for param in params["parameter"].as_array().into_iter().flatten() {
            match param["name"].as_str() {
                Some("patient") => scope.patients.push(local_id(param, "Patient")?),
                Some("group") => scope.groups.push(local_id(param, "Group")?),
                Some("_since") => since.push(instant(param)?),
                _ => {}
            }
        }
        if since.len() > 1 {
            return Err(OperationError::InvalidParameters(
                "_since must occur at most once".into(),
            ));
        }
        scope.since = since.pop();
        Ok(scope)
    }

    /// Whether any filter is present.
    pub(crate) fn is_empty(&self) -> bool {
        self.patients.is_empty() && self.groups.is_empty() && self.since.is_none()
    }

    /// The search selection for `resource_type`: `_since` as `_lastUpdated`,
    /// and the Patient compartments of the named patients and of the members
    /// of the named groups. A resource type outside the Patient compartment
    /// belongs to no patient's compartment, so the patient filters leave it
    /// whole. A patient or group the caller cannot find is a 400, naming the
    /// parameter (Common Operation Behavior, "Status code for a value that
    /// cannot be resolved").
    pub(crate) async fn search_params(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
    ) -> Result<SearchParams, OperationError> {
        let mut params = SearchParams::new();
        if let Some(since) = &self.since {
            params
                .parameters
                .insert("_lastUpdated".into(), vec![format!("gt{since}")]);
        }
        if self.patients.is_empty() && self.groups.is_empty() {
            return Ok(params);
        }

        for id in &self.patients {
            if context.read(state, "Patient", id).await?.is_none() {
                return Err(OperationError::InvalidParameters(format!(
                    "patient: Patient/{id} cannot be found"
                )));
            }
        }
        let now = time::OffsetDateTime::now_utc();
        let mut group_members = Vec::new();
        for id in &self.groups {
            let membership = crate::operations::group::resolve_membership(state, context, id, now)
                .await
                .map_err(|error| match error {
                    OperationError::NotFound(_) => OperationError::InvalidParameters(format!(
                        "group: Group/{id} cannot be found"
                    )),
                    other => other,
                })?;
            group_members.push(membership.patient_ids);
        }

        let compartment = state
            .compartment_registry
            .get("Patient")
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        let parameters = compartment
            .get_inclusion_params(resource_type)
            .unwrap_or_default()
            .to_vec();
        if resource_type != "Patient" && parameters.is_empty() {
            return Ok(params);
        }
        let members = |patient_ids: Vec<String>| SearchConstraint::LinkedPatients {
            patient_ids,
            fhir_base_url: state.fhir_base_url(),
            parameters: parameters.clone(),
        };
        // Each filter must hold: the named patients, and each named group.
        let mut constraints = Vec::new();
        if !self.patients.is_empty() {
            constraints.push(members(self.patients.clone()));
        }
        constraints.extend(group_members.into_iter().map(members));
        params.constraint = Some(match constraints.len() {
            1 => constraints.remove(0),
            _ => SearchConstraint::And(constraints),
        });
        Ok(params)
    }
}

/// The id of a local `resource_type` reference: `valueReference.reference`
/// (or a GET string) of `Type/id` or a bare id.
fn local_id(param: &Value, resource_type: &str) -> Result<String, OperationError> {
    let name = param["name"].as_str().unwrap_or_default();
    let reference = param["valueReference"]["reference"]
        .as_str()
        .or_else(|| param["valueString"].as_str())
        .ok_or_else(|| {
            OperationError::InvalidParameters(format!(
                "{name} requires a {resource_type} reference"
            ))
        })?;
    let id = reference
        .strip_prefix(&format!("{resource_type}/"))
        .unwrap_or(reference);
    octofhir_core::validate_id(id).map_err(|_| {
        OperationError::InvalidParameters(format!(
            "{name} requires a local {resource_type}/id reference"
        ))
    })?;
    Ok(id.to_owned())
}

/// An instant (`valueInstant`, or `valueDateTime`/`valueString` from GET),
/// checked as RFC 3339 and passed on as written.
fn instant(param: &Value) -> Result<String, OperationError> {
    let text = param["valueInstant"]
        .as_str()
        .or_else(|| param["valueDateTime"].as_str())
        .or_else(|| param["valueString"].as_str())
        .ok_or_else(|| OperationError::InvalidParameters("_since requires an instant".into()))?;
    chrono::DateTime::parse_from_rfc3339(text).map_err(|_| {
        OperationError::InvalidParameters(format!(
            "_since requires an instant with a time zone, not {text}"
        ))
    })?;
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filters_are_read_from_parameters_and_get_strings() {
        let scope = ViewScope::from_parameters(&json!({"resourceType":"Parameters","parameter":[
            {"name":"patient","valueReference":{"reference":"Patient/p1"}},
            {"name":"patient","valueString":"p2"},
            {"name":"group","valueReference":{"reference":"Group/g1"}},
            {"name":"_since","valueInstant":"2026-01-01T00:00:00Z"}
        ]}))
        .unwrap();
        assert_eq!(scope.patients, ["p1", "p2"]);
        assert_eq!(scope.groups, ["g1"]);
        assert_eq!(scope.since.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert!(!scope.is_empty());
    }

    #[test]
    fn a_since_without_a_time_zone_or_repeated_is_rejected() {
        for params in [
            json!({"parameter":[{"name":"_since","valueString":"2026-01-01"}]}),
            json!({"parameter":[
                {"name":"_since","valueInstant":"2026-01-01T00:00:00Z"},
                {"name":"_since","valueInstant":"2026-02-01T00:00:00Z"}
            ]}),
        ] {
            assert!(ViewScope::from_parameters(&params).is_err(), "{params}");
        }
    }

    #[test]
    fn a_reference_to_another_type_is_rejected() {
        let params = json!({"parameter":[{"name":"patient","valueString":"Group/g1"}]});
        assert!(ViewScope::from_parameters(&params).is_err());
    }
}
