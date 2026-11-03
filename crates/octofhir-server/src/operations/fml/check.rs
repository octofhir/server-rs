//! StructureMaps checked with `fml-check` when they are validated (on write and by `$validate`).
//!
//! The checker reads FML, so the StructureMap is printed as FML first; printing and compiling
//! give back the same StructureMap, so every problem can be placed at the group, rule, source or
//! target it is in (`StructureMap.group[0].rule[2].target[0]`). Problems are warnings: the
//! checker knows the core definitions, not every logical model a map may use.

use fml_check::{CoreDefinitions, check};
use fml_model::model::Rule;
use fml_model::{Locations, StructureMap};
use fml_syntax::TextRange;
use serde_json::Value;

use crate::validation::{IssueSeverity, ValidationIssue};

/// What `fml-check` finds in a StructureMap resource, as warnings.
pub(crate) fn structure_map_issues(
    resource: &Value,
    core: &CoreDefinitions,
) -> Vec<ValidationIssue> {
    // A resource that is not a StructureMap the model can read is the base validation's to
    // report.
    if resource["resourceType"] != "StructureMap" {
        return Vec::new();
    }
    let Ok(map) = fml_model::from_json(resource.clone()) else {
        return Vec::new();
    };
    let text = fml_model::print(&map);
    let parse = fml_syntax::parse(&text);
    let compiled = fml_model::compile_tree(&parse.tree());
    let positions = positions(&map);
    check(&parse.tree(), core)
        .into_iter()
        .map(|diagnostic| ValidationIssue {
            severity: IssueSeverity::Warning,
            code: "business-rule".into(),
            diagnostics: diagnostic.message,
            location: Some(
                innermost(&compiled.locations, &positions, diagnostic.range)
                    .unwrap_or_else(|| "StructureMap".to_owned()),
            ),
        })
        .collect()
}

/// Every group, rule, source and target of the map, by its location.
fn positions(map: &StructureMap) -> Vec<String> {
    fn rules(list: &[Rule], parent: &str, out: &mut Vec<String>) {
        for (index, rule) in list.iter().enumerate() {
            let position = format!("{parent}.rule[{index}]");
            out.extend((0..rule.source.len()).map(|k| format!("{position}.source[{k}]")));
            out.extend((0..rule.target.len()).map(|k| format!("{position}.target[{k}]")));
            rules(&rule.rule, &position, out);
            out.push(position);
        }
    }
    let mut out = Vec::new();
    for (index, group) in map.group.iter().enumerate() {
        let position = format!("StructureMap.group[{index}]");
        rules(&group.rule, &position, &mut out);
        out.push(position);
    }
    out
}

/// The smallest part of the map whose text holds `range`.
fn innermost(locations: &Locations, positions: &[String], range: TextRange) -> Option<String> {
    positions
        .iter()
        .filter_map(|position| Some((position, locations.range(position)?)))
        .filter(|(_, part)| part.contains_range(range) || part.contains(range.start()))
        .min_by_key(|(_, part)| part.len())
        .map(|(position, _)| position.clone())
}

#[cfg(test)]
mod tests {
    use fml_check::Release;
    use serde_json::json;

    use super::*;

    #[test]
    fn problems_are_placed_at_their_rule() {
        let compiled = fml_model::compile(
            "/// url = 'http://example.org/StructureMap/x'\n/// name = 'x'\nuses \"http://hl7.org/fhir/StructureDefinition/Patient\" alias P as source\nuses \"http://hl7.org/fhir/StructureDefinition/Basic\" alias B as target\n\ngroup G(source src : P, target tgt : B) {\n  src.id as v -> tgt.id = v \"ok\";\n  src.nmae as n -> tgt.code = n \"typo\";\n}\n",
        );
        let resource = serde_json::to_value(&compiled.map).unwrap();
        let issues = structure_map_issues(&resource, &CoreDefinitions::new(Release::R5));
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].diagnostics, "`Patient` has no element `nmae`");
        assert_eq!(
            issues[0].location.as_deref(),
            Some("StructureMap.group[0].rule[1].source[0]")
        );
        assert!(matches!(issues[0].severity, IssueSeverity::Warning));
        assert!(
            structure_map_issues(
                &json!({ "resourceType": "Patient" }),
                &CoreDefinitions::new(Release::R5)
            )
            .is_empty()
        );
    }
}
