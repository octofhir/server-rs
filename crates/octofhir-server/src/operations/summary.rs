//! `Patient/$summary`: a patient summary document, by default an International Patient
//! Summary (IPS IG 2.0.1), or the HL7 Europe Patient Summary through the `profile` parameter.
//!
//! The document is built on request from the patient's current data. The IG leaves what
//! is relevant to the implementer beyond current allergies, medications and problems; the
//! selection here is the table in [`SECTIONS`]. Every search goes through the caller's
//! grants, so a summary holds only what the caller may read.

use std::collections::{BTreeMap, HashSet};

use async_trait::async_trait;
use chrono::Utc;
use futures_util::future::try_join_all;
use octofhir_storage::{SearchConstraint, SearchParams, StoredResource};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::OperationContext;
use super::handler::{OperationError, OperationHandler, OperationLevel};
use crate::server::AppState;

const LOINC: &str = "http://loinc.org";
const EMPTY_REASON: &str = "http://terminology.hl7.org/CodeSystem/list-empty-reason";

/// Resources per query of a section, newest first where the type has a date.
const SECTION_LIMIT: u32 = 100;

/// One search feeding a section.
struct SectionQuery {
    resource_type: &'static str,
    criteria: &'static [(&'static str, &'static str)],
    sort: Option<&'static str>,
    /// `status` to keep, for types that have no such search parameter.
    status: Option<&'static str>,
}

const fn query(
    resource_type: &'static str,
    criteria: &'static [(&'static str, &'static str)],
    sort: Option<&'static str>,
) -> SectionQuery {
    SectionQuery {
        resource_type,
        criteria,
        sort,
        status: None,
    }
}

const fn active_only(resource_type: &'static str) -> SectionQuery {
    SectionQuery {
        resource_type,
        criteria: &[],
        sort: None,
        status: Some("active"),
    }
}

struct Section {
    title: &'static str,
    code: &'static str,
    display: &'static str,
    /// Every Composition profile requires this section even with nothing to say.
    required: bool,
    queries: &'static [SectionQuery],
}

const ACTIVE_CONDITION: &[(&str, &str)] = &[("clinical-status", "active,recurrence,relapse")];
const PAST_CONDITION: &[(&str, &str)] = &[("clinical-status", "inactive,remission,resolved")];
const ACTIVE: &[(&str, &str)] = &[("status", "active")];
/// The pregnancy status code and the `pregnancies-summary-uv-ips` value set.
const PREGNANCY: &[(&str, &str)] = &[(
    "code",
    "http://loinc.org|82810-3,http://loinc.org|11636-8,http://loinc.org|11637-6,\
http://loinc.org|11638-4,http://loinc.org|11639-2,http://loinc.org|11640-0,\
http://loinc.org|11612-9,http://loinc.org|11613-7,http://loinc.org|11614-5,\
http://loinc.org|33065-4",
)];

const SECTIONS: &[Section] = &[
    Section {
        title: "Problems",
        code: "11450-4",
        display: "Problem list - Reported",
        required: true,
        queries: &[query("Condition", ACTIVE_CONDITION, None)],
    },
    Section {
        title: "Allergies and Intolerances",
        code: "48765-2",
        display: "Allergies and adverse reactions Document",
        required: true,
        queries: &[query(
            "AllergyIntolerance",
            &[("clinical-status", "active")],
            None,
        )],
    },
    Section {
        title: "Medication Summary",
        code: "10160-0",
        display: "History of Medication use Narrative",
        required: true,
        queries: &[
            query("MedicationStatement", ACTIVE, None),
            query("MedicationRequest", ACTIVE, None),
        ],
    },
    Section {
        title: "Immunizations",
        code: "11369-6",
        display: "History of Immunization note",
        required: false,
        queries: &[query(
            "Immunization",
            &[("status", "completed")],
            Some("-date"),
        )],
    },
    Section {
        title: "Results",
        code: "30954-2",
        display: "Relevant diagnostic tests/laboratory data note",
        required: false,
        queries: &[
            query("DiagnosticReport", &[], Some("-date")),
            query("Observation", &[("category", "laboratory")], Some("-date")),
        ],
    },
    Section {
        title: "Vital Signs",
        code: "8716-3",
        display: "Vital signs note",
        required: false,
        queries: &[query(
            "Observation",
            &[("category", "vital-signs")],
            Some("-date"),
        )],
    },
    Section {
        title: "History of Procedures",
        code: "47519-4",
        display: "History of Procedures Document",
        required: false,
        queries: &[query("Procedure", &[], Some("-date"))],
    },
    Section {
        title: "Medical Devices",
        code: "46264-8",
        display: "History of medical device use",
        required: false,
        queries: &[active_only("DeviceUseStatement")],
    },
    Section {
        title: "Past Problems",
        code: "11348-0",
        display: "History of Past illness note",
        required: false,
        queries: &[query("Condition", PAST_CONDITION, None)],
    },
    Section {
        title: "Plan of Care",
        code: "18776-5",
        display: "Plan of care note",
        required: false,
        queries: &[query("CarePlan", ACTIVE, None)],
    },
    Section {
        title: "Social History",
        code: "29762-2",
        display: "Social history note",
        required: false,
        queries: &[query(
            "Observation",
            &[("category", "social-history")],
            Some("-date"),
        )],
    },
    Section {
        title: "Alerts",
        code: "104605-1",
        display: "Alert",
        required: false,
        queries: &[active_only("Flag")],
    },
    Section {
        title: "Advance Directives",
        code: "42348-3",
        display: "Advance directives",
        required: false,
        queries: &[query("Consent", ACTIVE, None)],
    },
    Section {
        title: "Functional Status",
        code: "47420-5",
        display: "Functional status assessment note",
        required: false,
        queries: &[query(
            "ClinicalImpression",
            &[("status", "completed")],
            Some("-date"),
        )],
    },
    Section {
        title: "History of Pregnancy",
        code: "10162-6",
        display: "History of pregnancies Narrative",
        required: false,
        queries: &[query("Observation", PREGNANCY, Some("-date"))],
    },
];

const TRAVEL: &[Section] = &[Section {
    title: "Travel History",
    code: "10182-4",
    display: "History of Travel Note",
    required: false,
    queries: &[query(
        "Observation",
        &[("code", "http://loinc.org|94651-7")],
        Some("-date"),
    )],
}];

/// A Composition profile the document can be built for.
struct SummaryProfile {
    composition: &'static str,
    bundle: &'static str,
    title: &'static str,
    /// Sections of [`SECTIONS`] that this profile requires on top of the common ones.
    also_required: &'static [&'static str],
    /// Sections only this profile has.
    extra: &'static [Section],
}

const IPS: SummaryProfile = SummaryProfile {
    composition: "http://hl7.org/fhir/uv/ips/StructureDefinition/Composition-uv-ips",
    bundle: "http://hl7.org/fhir/uv/ips/StructureDefinition/Bundle-uv-ips",
    title: "International Patient Summary",
    also_required: &[],
    extra: &[],
};

const EPS: SummaryProfile = SummaryProfile {
    composition: "http://hl7.eu/fhir/eps/StructureDefinition/composition-eu-eps",
    bundle: "http://hl7.eu/fhir/eps/StructureDefinition/bundle-eu-eps",
    title: "Patient Summary",
    also_required: &["47519-4", "46264-8"],
    extra: TRAVEL,
};

impl SummaryProfile {
    fn sections(&self) -> impl Iterator<Item = &'static Section> + use<> {
        SECTIONS.iter().chain(self.extra)
    }

    fn requires(&self, section: &Section) -> bool {
        section.required || self.also_required.contains(&section.code)
    }

    /// The profile named by the `profile` parameter, a canonical with an optional version.
    fn requested(params: &Value) -> Result<&'static Self, OperationError> {
        let Some(canonical) = parameter(params, "profile") else {
            return Ok(&IPS);
        };
        let url = canonical.split('|').next().unwrap_or(canonical);
        [&IPS, &EPS]
            .into_iter()
            .find(|profile| profile.composition == url || profile.bundle == url)
            .ok_or_else(|| {
                OperationError::NotSupported(format!(
                    "$summary cannot build {url}; it builds {} and {}",
                    IPS.composition, EPS.composition
                ))
            })
    }
}

/// The first value of an input parameter, as read from a query string or a `Parameters` body.
fn parameter<'a>(params: &'a Value, name: &str) -> Option<&'a str> {
    let found = params["parameter"]
        .as_array()?
        .iter()
        .find(|parameter| parameter["name"] == name)?;
    ["valueCanonical", "valueUri", "valueString"]
        .into_iter()
        .find_map(|key| found[key].as_str())
}

pub struct SummaryOperation;

async fn run_query(
    state: &AppState,
    context: &OperationContext,
    patient: &str,
    section: &SectionQuery,
) -> Result<Vec<StoredResource>, OperationError> {
    if !state
        .resource_type_set
        .load()
        .contains(section.resource_type)
    {
        return Ok(Vec::new());
    }
    let compartment = state
        .compartment_registry
        .get("Patient")
        .map_err(|error| OperationError::Internal(error.to_string()))?;
    let parameters = compartment
        .get_inclusion_params(section.resource_type)
        .unwrap_or_default()
        .to_vec();
    let mut search = SearchParams::new().with_count(SECTION_LIMIT);
    search.constraint = Some(SearchConstraint::LinkedPatient {
        patient_id: patient.to_owned(),
        fhir_base_url: state.fhir_base_url(),
        parameters,
    });
    for (name, value) in section.criteria {
        search = search.with_param(*name, *value);
    }
    if let Some(sort) = section.sort {
        search = match sort.strip_prefix('-') {
            Some(field) => search.with_sort(field, true),
            None => search.with_sort(sort, false),
        };
    }
    match context.search(state, section.resource_type, &search).await {
        Ok(mut result) => {
            if let Some(status) = section.status {
                result
                    .entries
                    .retain(|entry| entry.resource["status"] == status);
            }
            Ok(result.entries)
        }
        // A section the caller may not read stays out, as with any other summary gap.
        Err(error) if error.is_forbidden() => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The text a person would call this CodeableConcept.
fn concept_text(concept: &Value) -> Option<&str> {
    concept["text"]
        .as_str()
        .or_else(|| concept["coding"][0]["display"].as_str())
        .or_else(|| concept["coding"][0]["code"].as_str())
}

/// A short human label for a resource in a section's narrative.
fn label(resource: &Value) -> String {
    ["code", "medicationCodeableConcept", "vaccineCode", "type"]
        .iter()
        .find_map(|field| concept_text(&resource[*field]))
        .or_else(|| resource["medicationReference"]["display"].as_str())
        .or_else(|| resource["title"].as_str())
        .unwrap_or_else(|| resource["resourceType"].as_str().unwrap_or("Entry"))
        .to_owned()
}

fn narrative(items: &[String]) -> Value {
    let body = if items.is_empty() {
        "<p>No information available</p>".to_owned()
    } else {
        let rows: String = items
            .iter()
            .map(|item| format!("<li>{}</li>", escape(item)))
            .collect();
        format!("<ul>{rows}</ul>")
    };
    json!({"status": "generated",
        "div": format!("<div xmlns=\"http://www.w3.org/1999/xhtml\">{body}</div>")})
}

fn section_json(section: &Section, entries: &[StoredResource]) -> Value {
    let mut result = Map::new();
    result.insert("title".into(), json!(section.title));
    result.insert(
        "code".into(),
        json!({"coding": [{"system": LOINC, "code": section.code, "display": section.display}]}),
    );
    let labels: Vec<String> = entries.iter().map(|e| label(&e.resource)).collect();
    result.insert("text".into(), narrative(&labels));
    if entries.is_empty() {
        result.insert(
            "emptyReason".into(),
            json!({"coding": [{"system": EMPTY_REASON, "code": "unavailable",
                "display": "Unavailable"}]}),
        );
    } else {
        let references: Vec<Value> = entries
            .iter()
            .map(|e| json!({"reference": format!("{}/{}", e.resource_type, e.id)}))
            .collect();
        result.insert("entry".into(), json!(references));
    }
    Value::Object(result)
}

/// Local `Type/id` references held by `value`, skipping those in `known`.
fn collect_references(
    value: &Value,
    fhir_base: &str,
    known: &HashSet<String>,
    found: &mut BTreeMap<String, Vec<String>>,
) {
    match value {
        Value::Object(fields) => {
            for (name, field) in fields {
                if name == "reference"
                    && let Some(reference) = field.as_str()
                {
                    let local = reference
                        .strip_prefix(fhir_base)
                        .map_or(reference, |rest| rest.trim_start_matches('/'));
                    let target = local.split("/_history/").next().unwrap_or(local);
                    if let Some((resource_type, id)) = target.split_once('/')
                        && !id.contains('/')
                        && resource_type.starts_with(|c: char| c.is_ascii_uppercase())
                        && !known.contains(target)
                    {
                        found
                            .entry(resource_type.to_owned())
                            .or_default()
                            .push(id.to_owned());
                    }
                } else {
                    collect_references(field, fhir_base, known, found);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_references(item, fhir_base, known, found);
            }
        }
        _ => {}
    }
}

impl SummaryOperation {
    async fn build(
        &self,
        state: &AppState,
        context: &OperationContext,
        patient: StoredResource,
        profile: &SummaryProfile,
    ) -> Result<Value, OperationError> {
        let queries = profile
            .sections()
            .flat_map(|section| section.queries.iter());
        let results =
            try_join_all(queries.map(|section| run_query(state, context, &patient.id, section)))
                .await?;

        let mut results = results.into_iter();
        let mut known: HashSet<String> = HashSet::from([format!("Patient/{}", patient.id)]);
        let mut resources: Vec<StoredResource> = Vec::new();
        let mut sections = Vec::new();
        for section in profile.sections() {
            let mut entries = Vec::new();
            for _ in section.queries {
                entries.extend(results.next().unwrap_or_default());
            }
            if entries.is_empty() && !profile.requires(section) {
                continue;
            }
            sections.push(section_json(section, &entries));
            for entry in entries {
                if known.insert(format!("{}/{}", entry.resource_type, entry.id)) {
                    resources.push(entry);
                }
            }
        }

        // A document is read on its own, so what the entries point at comes along.
        let fhir_base = state.fhir_base_url();
        let mut missing = BTreeMap::new();
        for entry in &resources {
            collect_references(&entry.resource, &fhir_base, &known, &mut missing);
        }
        for (resource_type, ids) in missing {
            if !state.resource_type_set.load().contains(&resource_type) {
                continue;
            }
            match context.read_many(state, &resource_type, &ids).await {
                Ok(found) => {
                    for entry in found {
                        if known.insert(format!("{}/{}", entry.resource_type, entry.id)) {
                            resources.push(entry);
                        }
                    }
                }
                Err(error) if error.is_forbidden() => {}
                Err(error) => return Err(error),
            }
        }

        let now = Utc::now().to_rfc3339();
        let device = Uuid::new_v4();
        let composition = Uuid::new_v4();
        let mut entries = vec![
            json!({"fullUrl": format!("urn:uuid:{composition}"), "resource": {
                "resourceType": "Composition",
                "id": composition.to_string(),
                "meta": {"profile": [profile.composition]},
                "status": "final",
                "type": {"coding": [{"system": LOINC, "code": "60591-5",
                    "display": "Patient summary Document"}]},
                "subject": {"reference": format!("Patient/{}", patient.id)},
                "date": now,
                "author": [{"reference": format!("urn:uuid:{device}")}],
                "title": profile.title,
                "section": sections,
            }}),
            json!({"fullUrl": format!("{fhir_base}/Patient/{}", patient.id),
                "resource": patient.resource}),
            json!({"fullUrl": format!("urn:uuid:{device}"), "resource": {
                "resourceType": "Device",
                "id": device.to_string(),
                "deviceName": [{"name": "OctoFHIR", "type": "manufacturer-name"}],
            }}),
        ];
        entries.extend(resources.into_iter().map(|entry| {
            json!({
                "fullUrl": format!("{fhir_base}/{}/{}", entry.resource_type, entry.id),
                "resource": entry.resource,
            })
        }));
        Ok(json!({
            "resourceType": "Bundle",
            "identifier": {"system": "urn:ietf:rfc:3986",
                "value": format!("urn:uuid:{}", Uuid::new_v4())},
            "type": "document",
            "timestamp": now,
            "entry": entries,
        }))
    }
}

#[async_trait]
impl OperationHandler for SummaryOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        matches!(level, OperationLevel::Type | OperationLevel::Instance)
            && resource_type == Some("Patient")
    }

    fn code(&self) -> &str {
        "summary"
    }

    /// `Patient/$summary?identifier=`: the IG requires an identifier at this level.
    async fn handle_type(
        &self,
        state: &AppState,
        context: &OperationContext,
        _resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let profile = SummaryProfile::requested(params)?;
        let identifier = parameter(params, "identifier").ok_or_else(|| {
            OperationError::InvalidParameters("$summary requires an identifier here".into())
        })?;
        let search = SearchParams::new()
            .with_count(2)
            .with_param("identifier", identifier);
        let mut matches = context.search(state, "Patient", &search).await?.entries;
        match (matches.pop(), matches.is_empty()) {
            (Some(patient), true) => self.build(state, context, patient, profile).await,
            (None, _) => Err(OperationError::NotFound(format!(
                "No Patient has the identifier {identifier}"
            ))),
            _ => Err(OperationError::InvalidParameters(format!(
                "More than one Patient has the identifier {identifier}"
            ))),
        }
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        _resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let profile = SummaryProfile::requested(params)?;
        let patient = context
            .read(state, "Patient", id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("Patient/{id} not found")))?;
        self.build(state, context, patient, profile).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_sections_are_the_three_the_ig_requires() {
        let required: Vec<_> = SECTIONS
            .iter()
            .filter(|section| section.required)
            .map(|section| section.code)
            .collect();
        assert_eq!(required, ["11450-4", "48765-2", "10160-0"]);
    }

    #[test]
    fn the_europe_profile_requires_more_and_adds_travel() {
        let required: Vec<_> = EPS
            .sections()
            .filter(|section| EPS.requires(section))
            .map(|section| section.code)
            .collect();
        assert_eq!(
            required,
            ["11450-4", "48765-2", "10160-0", "47519-4", "46264-8"]
        );
        assert!(EPS.sections().any(|section| section.code == "10182-4"));
        assert!(!IPS.sections().any(|section| section.code == "10182-4"));
    }

    #[test]
    fn the_profile_parameter_selects_or_is_refused() {
        let with = |value: &str| json!({"parameter":[{"name":"profile","valueCanonical":value}]});
        let chosen = |params: &Value| SummaryProfile::requested(params).map(|p| p.title);
        assert_eq!(chosen(&json!({})).unwrap(), IPS.title);
        assert_eq!(chosen(&with(EPS.composition)).unwrap(), EPS.title);
        assert_eq!(chosen(&with(EPS.bundle)).unwrap(), EPS.title);
        let versioned = format!("{}|1.0.0-ballot", EPS.composition);
        assert_eq!(chosen(&with(&versioned)).unwrap(), EPS.title);
        assert!(matches!(
            chosen(&with("http://example.org/other")),
            Err(OperationError::NotSupported(_))
        ));
    }

    #[test]
    fn an_empty_section_says_why_and_a_full_one_has_no_empty_reason() {
        let section = &SECTIONS[0];
        let empty = section_json(section, &[]);
        assert_eq!(empty["emptyReason"]["coding"][0]["code"], "unavailable");
        assert!(empty.get("entry").is_none());
        assert!(
            empty["text"]["div"]
                .as_str()
                .unwrap()
                .contains("No information")
        );
    }

    #[test]
    fn narrative_is_escaped_and_labels_prefer_text() {
        let label = label(&json!({"code":{"coding":[{"code":"1"}],"text":"Asthma <mild>"}}));
        assert_eq!(label, "Asthma <mild>");
        let text = narrative(&[label]);
        assert!(
            text["div"]
                .as_str()
                .unwrap()
                .contains("Asthma &lt;mild&gt;")
        );
    }

    #[test]
    fn only_new_local_references_are_followed() {
        let known = HashSet::from(["Patient/p".to_owned()]);
        let mut found = BTreeMap::new();
        collect_references(
            &json!({
                "subject": {"reference": "Patient/p"},
                "medicationReference": {"reference": "https://a.example/fhir/Medication/m"},
                "performer": [{"actor": {"reference": "Practitioner/d/_history/2"}}],
                "basedOn": [{"reference": "https://other.example/fhir/CarePlan/c"}],
                "focus": {"reference": "#contained"},
            }),
            "https://a.example/fhir",
            &known,
            &mut found,
        );
        assert_eq!(found["Medication"], ["m"]);
        assert_eq!(found["Practitioner"], ["d"]);
        assert_eq!(found.len(), 2);
    }
}
