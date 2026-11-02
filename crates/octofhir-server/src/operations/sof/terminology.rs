//! Terminology dependencies of an SQLQuery or SQLView as relations (SQL on
//! FHIR, "Terminology in SQL").
//!
//! A ValueSet dependency is exposed as `(system, version, code, display,
//! inactive)`: one row per distinct `(system, version, code)` among the
//! expansion's `contains` entries at any depth whose `abstract` is not true.
//! A ConceptMap dependency is exposed as nine columns, one row per
//! `group.element.target` plus one per `noMap` element; content no flat row
//! can represent (`dependsOn`, `product`, `valueSet`, a group without
//! `source`) fails the request, and `group.unmapped` never produces rows.
//!
//! The functions here are pure: they turn an expansion or a ConceptMap into
//! rows, or say why its content cannot be determined (the operation answers
//! 422). Where the artifacts come from is [`super::term_source`]'s concern,
//! and how a ValueSet's definition becomes an expansion is
//! [`super::expansion`]'s.

use std::collections::HashSet;

use serde_json::{Value, json};

/// Rows a terminology relation may hold; a larger value set or concept map
/// is refused rather than materialized for one query.
pub(crate) const MAX_RELATION_ROWS: usize = 250_000;

/// Columns of a value set relation, in order, with their PostgreSQL types.
pub(crate) const VALUE_SET_COLUMNS: [(&str, &str); 5] = [
    ("system", "varchar"),
    ("version", "varchar"),
    ("code", "varchar"),
    ("display", "varchar"),
    ("inactive", "boolean"),
];

/// Columns of a concept map relation, in order, with their PostgreSQL types.
pub(crate) const CONCEPT_MAP_COLUMNS: [(&str, &str); 9] = [
    ("source_system", "varchar"),
    ("source_version", "varchar"),
    ("source_code", "varchar"),
    ("source_display", "varchar"),
    ("target_system", "varchar"),
    ("target_version", "varchar"),
    ("target_code", "varchar"),
    ("target_display", "varchar"),
    ("relationship", "varchar"),
];

/// Extension marking an expansion that is not closed (R4 `valueset-unclosed`).
const UNCLOSED: &str = "http://hl7.org/fhir/StructureDefinition/valueset-unclosed";

/// The column list of a relation, as the job's temporary table declares it.
pub(crate) fn columns(spec: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
    spec.iter()
        .map(|(name, sql_type)| ((*name).to_owned(), *sql_type))
        .collect()
}

/// Why an expansion does not determine a membership, if it does not: it is
/// a page (`offset`), it counts more members than it lists, or it says it
/// is not closed.
pub(crate) fn incomplete_expansion(expansion: &Value) -> Option<String> {
    if expansion["offset"]
        .as_u64()
        .is_some_and(|offset| offset > 0)
    {
        return Some("the expansion is a page (it carries an offset)".into());
    }
    let unclosed = expansion["extension"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|extension| extension["url"] == UNCLOSED && extension["valueBoolean"] == true);
    if unclosed {
        return Some("the expansion is marked unclosed".into());
    }
    if let Some(total) = expansion["total"].as_u64() {
        let listed = count_entries(&expansion["contains"]);
        if total > listed as u64 {
            return Some(format!("the expansion lists {listed} of {total} entries"));
        }
    }
    None
}

fn count_entries(contains: &Value) -> usize {
    let mut count = 0;
    let mut pending: Vec<&Value> = vec![contains];
    while let Some(list) = pending.pop() {
        for entry in list.as_array().into_iter().flatten() {
            count += 1;
            pending.push(&entry["contains"]);
        }
    }
    count
}

/// The value set relation of `expansion` (`ValueSet.expansion`), or why the
/// expansion does not determine the membership.
pub(crate) fn value_set_rows(expansion: &Value) -> Result<Vec<Value>, String> {
    if let Some(reason) = incomplete_expansion(expansion) {
        return Err(format!("its membership cannot be determined: {reason}"));
    }
    let mut seen: HashSet<(String, Option<String>, String)> = HashSet::new();
    let mut rows = Vec::new();
    // Depth-first in document order: the first listing of a member supplies
    // its display and inactive flag.
    let mut pending: Vec<std::slice::Iter<'_, Value>> = Vec::new();
    if let Some(top) = expansion["contains"].as_array() {
        pending.push(top.iter());
    }
    while let Some(iter) = pending.last_mut() {
        let Some(entry) = iter.next() else {
            pending.pop();
            continue;
        };
        if let Some(nested) = entry["contains"].as_array() {
            // Visit the entry first, its children next.
            let children = nested.iter();
            add_member(entry, &mut seen, &mut rows)?;
            pending.push(children);
        } else {
            add_member(entry, &mut seen, &mut rows)?;
        }
        if rows.len() > MAX_RELATION_ROWS {
            return Err(format!(
                "it has more than {MAX_RELATION_ROWS} members, more than a query relation may hold"
            ));
        }
    }
    Ok(rows)
}

/// Add the row of one `contains` entry, unless it is abstract, has no code
/// (a grouping entry) or repeats a member.
fn add_member(
    entry: &Value,
    seen: &mut HashSet<(String, Option<String>, String)>,
    rows: &mut Vec<Value>,
) -> Result<(), String> {
    if entry["abstract"] == true {
        return Ok(());
    }
    let Some(code) = entry["code"].as_str() else {
        return Ok(());
    };
    let system = entry["system"].as_str().ok_or_else(|| {
        format!(
            "its membership cannot be determined: the expansion lists code {code} without a system"
        )
    })?;
    let version = entry["version"].as_str().map(str::to_owned);
    if !seen.insert((system.to_owned(), version.clone(), code.to_owned())) {
        return Ok(());
    }
    rows.push(json!({
        "system": system,
        "version": version,
        "code": code,
        "display": entry["display"].as_str(),
        "inactive": entry["inactive"].as_bool(),
    }));
    Ok(())
}

/// The concept map relation of `map` (a ConceptMap, R4 or later), or why
/// its mappings cannot be determined.
pub(crate) fn concept_map_rows(map: &Value) -> Result<Vec<Value>, String> {
    let undeterminable = |reason: String| format!("its mappings cannot be determined: {reason}");
    let mut seen: HashSet<[Option<String>; 7]> = HashSet::new();
    let mut rows = Vec::new();
    for (group_index, group) in map["group"].as_array().into_iter().flatten().enumerate() {
        let source = group["source"].as_str().ok_or_else(|| {
            undeterminable(format!(
                "group {} has no source, so its codes have no system",
                group_index + 1
            ))
        })?;
        let (source_system, source_version) = split_system(source, group["sourceVersion"].as_str());
        let (target_system, target_version) = match group["target"].as_str() {
            Some(target) => {
                let (system, version) = split_system(target, group["targetVersion"].as_str());
                (Some(system), version)
            }
            None => (None, None),
        };
        for element in group["element"].as_array().into_iter().flatten() {
            if element.get("valueSet").is_some() {
                return Err(undeterminable(format!(
                    "an element of group {} maps a value set, not a code",
                    group_index + 1
                )));
            }
            let source_code = element["code"].as_str().ok_or_else(|| {
                undeterminable(format!(
                    "an element of group {} has no code",
                    group_index + 1
                ))
            })?;
            let source_display = element["display"].as_str();
            let mut push = |target_code: Option<&str>,
                            target_display: Option<&str>,
                            relationship: Option<&str>|
             -> Result<(), String> {
                let key = [
                    Some(source_system.to_owned()),
                    source_version.map(str::to_owned),
                    Some(source_code.to_owned()),
                    target_system.map(str::to_owned),
                    target_version.map(str::to_owned),
                    target_code.map(str::to_owned),
                    relationship.map(str::to_owned),
                ];
                if !seen.insert(key) {
                    return Ok(());
                }
                if rows.len() >= MAX_RELATION_ROWS {
                    return Err(format!(
                        "it has more than {MAX_RELATION_ROWS} mappings, more than a query relation may hold"
                    ));
                }
                rows.push(json!({
                    "source_system": source_system,
                    "source_version": source_version,
                    "source_code": source_code,
                    "source_display": source_display,
                    "target_system": target_system,
                    "target_version": target_version,
                    "target_code": target_code,
                    "target_display": target_display,
                    "relationship": relationship,
                }));
                Ok(())
            };
            for target in element["target"].as_array().into_iter().flatten() {
                for unrepresentable in ["dependsOn", "product", "valueSet"] {
                    if target.get(unrepresentable).is_some() {
                        return Err(undeterminable(format!(
                            "the mapping of {source_code} in group {} carries {unrepresentable}",
                            group_index + 1
                        )));
                    }
                }
                match target_relationship(target)? {
                    Relationship::NoMapping => push(None, None, None)?,
                    Relationship::Code(relationship) => push(
                        target["code"].as_str(),
                        target["display"].as_str(),
                        relationship,
                    )?,
                }
            }
            if element["noMap"] == true {
                push(None, None, None)?;
            }
        }
    }
    Ok(rows)
}

/// How a target relates to its source, as a ConceptMapRelationship code.
enum Relationship<'a> {
    Code(Option<&'a str>),
    /// R4 `unmatched`: the source has no mapping (R5 `noMap`).
    NoMapping,
}

/// `relationship` (R5 and later), or the R4 `equivalence` converted as the
/// FHIR R4 to R5 transforms convert it.
fn target_relationship(target: &Value) -> Result<Relationship<'_>, String> {
    if let Some(relationship) = target["relationship"].as_str() {
        return Ok(Relationship::Code(Some(relationship)));
    }
    let Some(equivalence) = target["equivalence"].as_str() else {
        return Ok(Relationship::Code(None));
    };
    Ok(match equivalence {
        "relatedto" | "inexact" => Relationship::Code(Some("related-to")),
        "equivalent" | "equal" => Relationship::Code(Some("equivalent")),
        "wider" | "subsumes" => Relationship::Code(Some("source-is-narrower-than-target")),
        "narrower" | "specializes" => Relationship::Code(Some("source-is-broader-than-target")),
        "disjoint" => Relationship::Code(Some("not-related-to")),
        "unmatched" => Relationship::NoMapping,
        other => {
            return Err(format!(
                "its mappings cannot be determined: unknown equivalence {other}"
            ));
        }
    })
}

/// `system|version` → (system, version); R4 carries the version apart.
fn split_system<'a>(canonical: &'a str, version: Option<&'a str>) -> (&'a str, Option<&'a str>) {
    match canonical.split_once('|') {
        Some((system, suffix)) if !suffix.is_empty() => (system, Some(suffix)),
        Some((system, _)) => (system, version),
        None => (canonical, version),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The value set of the specification's worked example.
    fn cvd_expansion() -> Value {
        json!({
            "identifier": "urn:uuid:635ad7e4-4f98-48ef-a3c2-1e3af6cc03ec",
            "timestamp": "2026-09-16T00:00:00Z",
            "total": 3,
            "parameter": [
                {"name": "version", "valueUri": "http://snomed.info/sct|http://snomed.info/sct/900000000000207008/version/20260201"},
                {"name": "version", "valueUri": "http://hl7.org/fhir/sid/icd-10|2019"}
            ],
            "contains": [{
                "abstract": true,
                "display": "Ischaemic heart disease",
                "contains": [
                    {"system": "http://snomed.info/sct",
                     "version": "http://snomed.info/sct/900000000000207008/version/20260201",
                     "code": "22298006", "display": "Myocardial infarction"},
                    {"system": "http://hl7.org/fhir/sid/icd-10", "version": "2019",
                     "code": "I21", "display": "Acute myocardial infarction"}
                ]
            }]
        })
    }

    #[test]
    fn the_worked_value_set_yields_its_two_members() {
        let rows = value_set_rows(&cvd_expansion()).unwrap();
        assert_eq!(
            rows,
            [
                json!({"system": "http://snomed.info/sct",
                       "version": "http://snomed.info/sct/900000000000207008/version/20260201",
                       "code": "22298006", "display": "Myocardial infarction", "inactive": null}),
                json!({"system": "http://hl7.org/fhir/sid/icd-10", "version": "2019",
                       "code": "I21", "display": "Acute myocardial infarction", "inactive": null}),
            ]
        );
    }

    #[test]
    fn members_are_unique_by_system_version_and_code() {
        let expansion = json!({"contains": [
            {"system": "s", "code": "a", "display": "first", "contains": [
                {"system": "s", "code": "b"},
                {"system": "s", "code": "a", "display": "again"}
            ]},
            {"system": "s", "code": "b", "inactive": true},
            {"system": "s", "version": "2", "code": "a"},
            {"system": "s", "code": "c", "abstract": true},
            {"system": "s", "code": "d", "inactive": true}
        ]});
        let rows = value_set_rows(&expansion).unwrap();
        let keys: Vec<(String, Value, String)> = rows
            .iter()
            .map(|row| {
                (
                    row["system"].as_str().unwrap().to_owned(),
                    row["version"].clone(),
                    row["code"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            keys,
            [
                ("s".into(), Value::Null, "a".into()),
                ("s".into(), Value::Null, "b".into()),
                ("s".into(), json!("2"), "a".into()),
                ("s".into(), Value::Null, "d".into()),
            ]
        );
        assert_eq!(rows[0]["display"], "first");
        assert_eq!(rows[3]["inactive"], true);
        assert!(value_set_rows(&json!({"total": 0})).unwrap().is_empty());
    }

    #[test]
    fn a_partial_expansion_does_not_determine_membership() {
        let mut paged = cvd_expansion();
        paged["offset"] = json!(10);
        assert!(value_set_rows(&paged).unwrap_err().contains("offset"));
        let mut short = cvd_expansion();
        short["total"] = json!(4);
        assert!(value_set_rows(&short).unwrap_err().contains("3 of 4"));
        let mut unclosed = cvd_expansion();
        unclosed["extension"] = json!([{"url": UNCLOSED, "valueBoolean": true}]);
        assert!(value_set_rows(&unclosed).unwrap_err().contains("unclosed"));
        let mut first_page = cvd_expansion();
        first_page["offset"] = json!(0);
        assert_eq!(value_set_rows(&first_page).unwrap().len(), 2);
        let systemless = json!({"contains": [{"code": "x"}]});
        assert!(
            value_set_rows(&systemless)
                .unwrap_err()
                .contains("without a system")
        );
    }

    /// The concept map of the specification's worked example.
    fn sct_to_icd10() -> Value {
        json!({
            "resourceType": "ConceptMap",
            "url": "http://example.org/ConceptMap/sct-to-icd10",
            "version": "2026",
            "status": "active",
            "group": [{
                "source": "http://snomed.info/sct",
                "target": "http://hl7.org/fhir/sid/icd-10|2019",
                "element": [
                    {"code": "22298006", "display": "Myocardial infarction", "target": [
                        {"code": "I21", "display": "Acute myocardial infarction",
                         "relationship": "equivalent"}]},
                    {"code": "73211009", "display": "Diabetes mellitus", "target": [
                        {"code": "E14", "display": "Unspecified diabetes mellitus",
                         "relationship": "source-is-broader-than-target",
                         "comment": "The source covers every type of diabetes"}]},
                    {"code": "102499006", "display": "Fit and well", "noMap": true}
                ],
                "unmapped": {"mode": "use-source-code"}
            }]
        })
    }

    #[test]
    fn the_worked_concept_map_yields_three_rows() {
        let rows = concept_map_rows(&sct_to_icd10()).unwrap();
        let row = |code: &str, target: Value, display: Value, relationship: Value| {
            json!({
                "source_system": "http://snomed.info/sct", "source_version": null,
                "source_code": code, "source_display": match code {
                    "22298006" => "Myocardial infarction",
                    "73211009" => "Diabetes mellitus",
                    _ => "Fit and well",
                },
                "target_system": "http://hl7.org/fhir/sid/icd-10", "target_version": "2019",
                "target_code": target, "target_display": display, "relationship": relationship
            })
        };
        assert_eq!(
            rows,
            [
                row(
                    "22298006",
                    json!("I21"),
                    json!("Acute myocardial infarction"),
                    json!("equivalent")
                ),
                row(
                    "73211009",
                    json!("E14"),
                    json!("Unspecified diabetes mellitus"),
                    json!("source-is-broader-than-target")
                ),
                row("102499006", Value::Null, Value::Null, Value::Null),
            ]
        );
    }

    #[test]
    fn content_no_row_can_represent_is_rejected() {
        let mut no_source = sct_to_icd10();
        no_source["group"][0]
            .as_object_mut()
            .unwrap()
            .remove("source");
        assert!(
            concept_map_rows(&no_source)
                .unwrap_err()
                .contains("no source")
        );
        for (path, field, value) in [
            (
                "target",
                "dependsOn",
                json!([{"attribute": "a", "valueCode": "x"}]),
            ),
            (
                "target",
                "product",
                json!([{"attribute": "a", "valueCode": "x"}]),
            ),
            ("target", "valueSet", json!("http://x/vs")),
        ] {
            let mut map = sct_to_icd10();
            map["group"][0]["element"][0][path][0][field] = value;
            assert!(
                concept_map_rows(&map).unwrap_err().contains(field),
                "{field}"
            );
        }
        let mut element_value_set = sct_to_icd10();
        element_value_set["group"][0]["element"][0]["valueSet"] = json!("http://x/vs");
        assert!(
            concept_map_rows(&element_value_set)
                .unwrap_err()
                .contains("value set")
        );
    }

    #[test]
    fn r4_maps_convert_equivalence_and_versions() {
        let map = json!({
            "resourceType": "ConceptMap",
            "group": [{
                "source": "http://loinc.org", "sourceVersion": "2.77",
                "target": "http://snomed.info/sct",
                "element": [
                    {"code": "a", "target": [{"code": "x", "equivalence": "wider"}]},
                    {"code": "b", "target": [{"equivalence": "unmatched"}]},
                    {"code": "c", "target": [{"code": "y", "equivalence": "disjoint"},
                                             {"code": "y", "equivalence": "disjoint"}]}
                ]
            }]
        });
        let rows = concept_map_rows(&map).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["source_version"], "2.77");
        assert_eq!(rows[0]["relationship"], "source-is-narrower-than-target");
        assert_eq!(rows[1]["target_code"], Value::Null);
        assert_eq!(rows[1]["relationship"], Value::Null);
        assert_eq!(rows[1]["target_system"], "http://snomed.info/sct");
        assert_eq!(rows[2]["relationship"], "not-related-to");
        assert!(
            concept_map_rows(&json!({"resourceType": "ConceptMap"}))
                .unwrap()
                .is_empty()
        );
    }
}
