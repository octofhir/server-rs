//! `Measure.supplementalData` in a MeasureReport.
//!
//! In an individual report a resource is referenced and any other value goes into a contained
//! Observation. In a summary each supplemental data element is one contained Observation with
//! a component per value, holding how many members of the population had that value. R5 has
//! `MeasureReport.supplementalData`; before that the same reference goes into the cross-version
//! extension.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde_json::{Value, json};

use super::super::OperationError;

const EXTENSION: &str =
    "http://hl7.org/fhir/5.0/StructureDefinition/extension-MeasureReport.supplementalData";

/// One `Measure.supplementalData`.
pub(super) struct Supplemental<'m> {
    define: &'m str,
    code: Value,
}

impl<'m> Supplemental<'m> {
    pub(super) fn parse(measure: &'m Value) -> Result<Vec<Self>, OperationError> {
        measure["supplementalData"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|element| {
                let define = element["criteria"]["expression"].as_str().ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "A supplementalData element has no expression".into(),
                    )
                })?;
                let code = match element.get("code") {
                    Some(code) => code.clone(),
                    None => json!({"text": define}),
                };
                Ok(Self { define, code })
            })
            .collect()
    }

    fn values<'d>(
        &self,
        defines: &'d IndexMap<String, Value>,
    ) -> Result<Vec<&'d Value>, OperationError> {
        match defines.get(self.define) {
            None => Err(OperationError::InvalidParameters(format!(
                "Supplemental data {} is not defined in the library",
                self.define
            ))),
            Some(Value::Array(items)) => Ok(items.iter().filter(|item| !item.is_null()).collect()),
            Some(Value::Null) => Ok(Vec::new()),
            Some(value) => Ok(vec![value]),
        }
    }
}

/// What a report gains: references, and the Observations they point at.
#[derive(Debug, Default)]
pub(super) struct Reported {
    references: Vec<Value>,
    pub(super) contained: Vec<Value>,
}

impl Reported {
    /// Adds the references to `report`, as the element or, before R5, the extension.
    pub(super) fn attach(self, report: &mut Value, element: bool) {
        if self.references.is_empty() {
            return;
        }
        if element {
            report["supplementalData"] = json!(self.references);
        } else {
            report["extension"] = self
                .references
                .into_iter()
                .map(|reference| json!({"url": EXTENSION, "valueReference": reference}))
                .collect();
        }
    }
}

fn resource_reference(value: &Value) -> Option<String> {
    Some(format!(
        "{}/{}",
        value["resourceType"].as_str()?,
        value["id"].as_str()?
    ))
}

/// A value as `Observation.value[x]`.
fn observation_value(value: &Value) -> (&'static str, Value) {
    match value {
        Value::Bool(_) => ("valueBoolean", value.clone()),
        Value::Number(number) if number.is_f64() => ("valueQuantity", json!({"value": value})),
        Value::Number(_) => ("valueInteger", value.clone()),
        Value::String(_) => ("valueString", value.clone()),
        Value::Object(fields) if fields.contains_key("coding") || fields.contains_key("code") => {
            ("valueCodeableConcept", concept(value))
        }
        Value::Object(fields) if fields.contains_key("value") => ("valueQuantity", value.clone()),
        other => ("valueString", json!(other.to_string())),
    }
}

/// A value as a CodeableConcept, for the code of a summary component.
fn concept(value: &Value) -> Value {
    match value {
        Value::Object(fields) if fields.contains_key("coding") => value.clone(),
        Value::Object(fields) if fields.contains_key("code") => json!({"coding": [value]}),
        Value::String(text) => json!({"text": text}),
        other => json!({"text": resource_reference(other).unwrap_or_else(|| other.to_string())}),
    }
}

/// The supplemental data of the one subject of an individual report.
pub(super) fn individual(
    elements: &[Supplemental<'_>],
    defines: &IndexMap<String, Value>,
) -> Result<Reported, OperationError> {
    let mut reported = Reported::default();
    for (index, element) in elements.iter().enumerate() {
        for (position, value) in element.values(defines)?.into_iter().enumerate() {
            if let Some(reference) = resource_reference(value) {
                reported.references.push(json!({"reference": reference}));
                continue;
            }
            let id = format!("supplemental-{index}-{position}");
            let (name, content) = observation_value(value);
            reported
                .references
                .push(json!({"reference": format!("#{id}")}));
            reported.contained.push(json!({
                "resourceType": "Observation", "id": id, "status": "final",
                "code": element.code, name: content,
            }));
        }
    }
    Ok(reported)
}

/// How often each value turned up among `members`.
pub(super) fn summary<'d>(
    elements: &[Supplemental<'_>],
    members: impl Iterator<Item = &'d IndexMap<String, Value>>,
) -> Result<Reported, OperationError> {
    let mut counts: Vec<BTreeMap<String, (&Value, i64)>> = vec![BTreeMap::new(); elements.len()];
    for defines in members {
        for (element, by_value) in elements.iter().zip(&mut counts) {
            for value in element.values(defines)? {
                by_value.entry(value.to_string()).or_insert((value, 0)).1 += 1;
            }
        }
    }
    let mut reported = Reported::default();
    for (index, (element, by_value)) in elements.iter().zip(counts).enumerate() {
        let id = format!("supplemental-{index}");
        let components: Vec<Value> = by_value
            .into_values()
            .map(|(value, count)| json!({"code": concept(value), "valueInteger": count}))
            .collect();
        let mut observation = json!({
            "resourceType": "Observation", "id": id, "status": "final", "code": element.code,
        });
        if !components.is_empty() {
            observation["component"] = json!(components);
        }
        reported
            .references
            .push(json!({"reference": format!("#{id}")}));
        reported.contained.push(observation);
    }
    Ok(reported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure() -> Value {
        json!({"supplementalData": [
            {"code": {"text": "sex"}, "criteria": {"expression": "SDE Sex"}},
            {"criteria": {"expression": "SDE Payer"}},
        ]})
    }

    fn defines(sex: Value, payer: Value) -> IndexMap<String, Value> {
        IndexMap::from([("SDE Sex".to_owned(), sex), ("SDE Payer".to_owned(), payer)])
    }

    #[test]
    fn an_individual_report_references_resources_and_contains_the_rest() {
        let measure = measure();
        let elements = Supplemental::parse(&measure).unwrap();
        let coverage = json!({"resourceType": "Coverage", "id": "c1"});
        let reported = individual(&elements, &defines(json!("female"), json!([coverage]))).unwrap();

        assert_eq!(
            reported.references,
            [
                json!({"reference": "#supplemental-0-0"}),
                json!({"reference": "Coverage/c1"})
            ]
        );
        assert_eq!(reported.contained.len(), 1);
        assert_eq!(reported.contained[0]["code"]["text"], "sex");
        assert_eq!(reported.contained[0]["valueString"], "female");
    }

    #[test]
    fn a_summary_counts_each_value_among_the_members() {
        let measure = measure();
        let elements = Supplemental::parse(&measure).unwrap();
        let members = [
            defines(json!("female"), Value::Null),
            defines(json!("female"), Value::Null),
            defines(json!("male"), Value::Null),
        ];
        let reported = summary(&elements, members.iter()).unwrap();

        let sex = &reported.contained[0];
        assert_eq!(sex["code"]["text"], "sex");
        assert_eq!(
            sex["component"],
            json!([
                {"code": {"text": "female"}, "valueInteger": 2},
                {"code": {"text": "male"}, "valueInteger": 1},
            ])
        );
        // Without a code, the element goes by its expression.
        assert_eq!(reported.contained[1]["code"]["text"], "SDE Payer");
        assert!(reported.contained[1].get("component").is_none());
    }

    #[test]
    fn a_define_the_library_lacks_is_an_error() {
        let measure = measure();
        let elements = Supplemental::parse(&measure).unwrap();
        let defines = IndexMap::from([("SDE Sex".to_owned(), json!("female"))]);
        assert!(matches!(
            individual(&elements, &defines),
            Err(OperationError::InvalidParameters(_))
        ));
    }

    #[test]
    fn the_extension_carries_the_reference_before_r5() {
        let reported = || Reported {
            references: vec![json!({"reference": "#s"})],
            contained: Vec::new(),
        };
        let mut r5 = json!({});
        reported().attach(&mut r5, true);
        assert_eq!(r5["supplementalData"][0]["reference"], "#s");
        let mut r4 = json!({});
        reported().attach(&mut r4, false);
        assert_eq!(r4["extension"][0]["url"], EXTENSION);
        assert_eq!(r4["extension"][0]["valueReference"]["reference"], "#s");
    }

    #[test]
    fn values_take_the_type_of_their_kind() {
        assert_eq!(observation_value(&json!(true)).0, "valueBoolean");
        assert_eq!(observation_value(&json!(3)).0, "valueInteger");
        assert_eq!(observation_value(&json!(1.5)).0, "valueQuantity");
        let coding = json!({"system": "urn:s", "code": "a"});
        assert_eq!(
            observation_value(&coding),
            ("valueCodeableConcept", json!({"coding": [coding]}))
        );
    }
}
