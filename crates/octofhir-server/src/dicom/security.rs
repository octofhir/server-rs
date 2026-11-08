//! Preserve explicit FHIR security labels across connector-derived metadata.
//!
//! # Contents
//! Stable union of source and operator labels for studies, workflow and results.
//!
//! # Invariants
//! Labels are never inferred from DICOM data or removed automatically. Current
//! resource policies still decide whether each read and derived write is allowed.
use serde_json::{Value, json};
pub(crate) fn inherit_security(resource: &mut Value, sources: &[&Value]) {
    let mut labels = resource["meta"]["security"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let key = |value: &Value| {
        let mut value = value.clone();
        value.sort_all_objects();
        serde_json::to_string(&value).expect("Coding JSON")
    };
    let mut known: std::collections::BTreeSet<_> = labels.iter().map(key).collect();
    for source in sources {
        for label in source["meta"]["security"].as_array().into_iter().flatten() {
            if known.insert(key(label)) {
                labels.push(label.clone());
            }
        }
    }
    if !labels.is_empty() {
        resource["meta"]["security"] = json!(labels);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_operator_labels_and_does_not_reorder_or_duplicate_on_repeat() {
        let mut resource = json!({"meta":{"security":[{"system":"urn:labels","code":"existing"}]}});
        let source = json!({"meta":{"security":[{"system":"urn:labels","code":"restricted"},{"system":"urn:labels","code":"existing"}]}});
        inherit_security(&mut resource, &[&source]);
        let first = resource.clone();
        inherit_security(&mut resource, &[&source]);
        assert_eq!(resource, first);
        assert_eq!(resource["meta"]["security"].as_array().unwrap().len(), 2);
        assert_eq!(resource["meta"]["security"][0]["code"], "existing");
    }
}
