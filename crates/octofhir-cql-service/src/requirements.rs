//! What a library needs in order to run, as a `module-definition` Library: the data it
//! retrieves, the terminology and libraries it depends on, and its parameters. This is the
//! result of the `$data-requirements` operations.

use octofhir_cql::CompiledLibrary;
use serde_json::{Value, json};

use crate::error::{CqlError, CqlResult};

const LIBRARY_TYPE: &str = "http://terminology.hl7.org/CodeSystem/library-type";

/// The ELM of a library and of everything it includes, by library name.
struct Libraries(Vec<(String, Value)>);

impl Libraries {
    fn of(library: &CompiledLibrary) -> CqlResult<Self> {
        let elm = |elm| {
            serde_json::to_value(elm).map_err(|error| CqlError::EvaluationError(error.to_string()))
        };
        let mut all = vec![(String::new(), elm(library.elm())?)];
        for (name, included) in library.included() {
            all.push((name.to_owned(), elm(included)?));
        }
        Ok(Self(all))
    }

    fn iter(&self) -> impl Iterator<Item = &Value> {
        self.0.iter().map(|(_, elm)| elm)
    }

    /// The definition a reference points at, with the library holding it. `current` is
    /// the library the reference appears in; its includes give aliases their meaning.
    fn resolve<'a>(
        &'a self,
        current: &'a Value,
        reference: &Value,
        section: &str,
    ) -> Option<(&'a Value, &'a Value)> {
        let owner = match reference["libraryName"].as_str() {
            None => current,
            Some(alias) => {
                let path = current["includes"]["def"]
                    .as_array()?
                    .iter()
                    .find(|include| include["localIdentifier"] == alias)?["path"]
                    .as_str()?;
                self.0
                    .iter()
                    .find(|(name, _)| name == path)
                    .map(|(_, elm)| elm)?
            }
        };
        let definition = owner[section]["def"]
            .as_array()?
            .iter()
            .find(|definition| definition["name"] == reference["name"])?;
        Some((owner, definition))
    }
}

/// Every object of an ELM type below `value`.
fn nodes_of<'v>(value: &'v Value, elm_type: &str, found: &mut Vec<&'v Value>) {
    match value {
        Value::Object(object) => {
            if object.get("type").is_some_and(|name| name == elm_type) {
                found.push(value);
            }
            for child in object.values() {
                nodes_of(child, elm_type, found);
            }
        }
        Value::Array(items) => {
            for child in items {
                nodes_of(child, elm_type, found);
            }
        }
        _ => {}
    }
}

/// `url|version`, or the url of a definition with no version.
fn canonical(definition: &Value) -> Option<String> {
    let url = definition["id"].as_str()?;
    Some(match definition["version"].as_str() {
        Some(version) => format!("{url}|{version}"),
        None => url.to_owned(),
    })
}

/// The code filters of a retrieve, from the value sets and codes it names.
fn code_filters(libraries: &Libraries, elm: &Value, retrieve: &Value) -> Vec<Value> {
    let Some(path) = retrieve["codeProperty"].as_str() else {
        return Vec::new();
    };
    let mut filters = Vec::new();
    let mut references = Vec::new();
    nodes_of(&retrieve["codes"], "ValueSetRef", &mut references);
    for reference in references.drain(..) {
        if let Some((_, definition)) = libraries.resolve(elm, reference, "valueSets") {
            filters.push(json!({"path": path, "valueSet": canonical(definition)}));
        }
    }
    nodes_of(&retrieve["codes"], "CodeRef", &mut references);
    let codes: Vec<Value> = references
        .into_iter()
        .filter_map(|reference| {
            let (owner, code) = libraries.resolve(elm, reference, "codes")?;
            let (_, system) = libraries.resolve(owner, &code["codeSystem"], "codeSystems")?;
            let mut coding = json!({"system": system["id"], "code": code["id"]});
            if !code["display"].is_null() {
                coding["display"] = code["display"].clone();
            }
            Some(coding)
        })
        .collect();
    if !codes.is_empty() {
        filters.push(json!({"path": path, "code": codes}));
    }
    filters
}

/// The dotted path of a property of the query's alias, such as `period.start`.
fn alias_path(node: &Value, alias: &str) -> Option<String> {
    if node["type"] != "Property" {
        return None;
    }
    let path = node["path"].as_str()?;
    if node["source"]["type"] == "AliasRef" {
        return (node["source"]["name"] == alias).then(|| path.to_owned());
    }
    Some(format!("{}.{path}", alias_path(&node["source"], alias)?))
}

fn string_literal(node: &Value) -> Option<&Value> {
    let literal = node["type"] == "Literal"
        && node["valueType"]
            .as_str()
            .is_some_and(|name| name.ends_with("String"));
    literal.then(|| &node["value"])
}

/// What a `where` clause narrows a retrieve to, as filters on the data requirement.
struct WhereFilters<'a> {
    libraries: &'a Libraries,
    elm: &'a Value,
    alias: &'a str,
    codes: Vec<Value>,
    dates: Vec<Value>,
}

impl WhereFilters<'_> {
    fn value_set(&mut self, path: String, reference: &Value) {
        if let Some((_, definition)) = self.libraries.resolve(self.elm, reference, "valueSets") {
            self.codes
                .push(json!({"path": path, "valueSet": canonical(definition)}));
        }
    }

    /// The filter of comparing `path` with `value`, if a filter can say it.
    fn compare(&mut self, path: String, value: &Value) {
        if let Some(code) = string_literal(value) {
            self.codes
                .push(json!({"path": path, "code": [{"code": code}]}));
            return;
        }
        match value["type"].as_str() {
            Some("CodeRef") => {
                let coding =
                    self.libraries
                        .resolve(self.elm, value, "codes")
                        .and_then(|(owner, code)| {
                            let (_, system) = self.libraries.resolve(
                                owner,
                                &code["codeSystem"],
                                "codeSystems",
                            )?;
                            Some(json!({"system": system["id"], "code": code["id"]}))
                        });
                if let Some(coding) = coding {
                    self.codes.push(json!({"path": path, "code": [coding]}));
                }
            }
            Some("ValueSetRef") => self.value_set(path, value),
            Some("List") => {
                let codes: Option<Vec<Value>> = value["element"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|item| Some(json!({"code": string_literal(item)?})))
                    .collect();
                if let Some(codes) = codes.filter(|codes| !codes.is_empty()) {
                    self.codes.push(json!({"path": path, "code": codes}));
                }
            }
            // The interval is only known at run time, so the filter names just the element.
            Some("ParameterRef") => self.dates.push(json!({"path": path})),
            _ => {}
        }
    }

    /// Only conjunctions narrow the retrieve as a whole; any other shape is left alone.
    fn visit(&mut self, condition: &Value) {
        let operands = condition["operand"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        match condition["type"].as_str() {
            Some("And") => operands.iter().for_each(|operand| self.visit(operand)),
            Some("Equal" | "Equivalent" | "In" | "IncludedIn") => {
                if let [left, right] = operands
                    && let Some(path) = alias_path(left, self.alias)
                {
                    self.compare(path, right);
                }
            }
            Some("InValueSet") => {
                if let Some(path) = alias_path(&condition["code"], self.alias) {
                    self.value_set(path, &condition["valueset"]);
                }
            }
            _ => {}
        }
    }
}

fn data_requirement(
    libraries: &Libraries,
    elm: &Value,
    retrieve: &Value,
    query: Option<&Value>,
) -> Option<Value> {
    let data_type = retrieve["dataType"].as_str()?;
    let name = data_type.rsplit('}').next().unwrap_or(data_type);
    let mut requirement = json!({"type": name});
    if let Some(profile) = retrieve["templateId"].as_str() {
        requirement["profile"] = json!([profile]);
    }
    let mut filters = WhereFilters {
        libraries,
        elm,
        alias: query.map_or("", |query| {
            query["source"][0]["alias"].as_str().unwrap_or_default()
        }),
        codes: code_filters(libraries, elm, retrieve),
        dates: Vec::new(),
    };
    if let Some(path) = retrieve["dateProperty"].as_str() {
        filters.dates.push(json!({"path": path}));
    }
    if let Some(query) = query {
        filters.visit(&query["where"]);
    }
    if !filters.codes.is_empty() {
        requirement["codeFilter"] = json!(filters.codes);
    }
    if !filters.dates.is_empty() {
        requirement["dateFilter"] = json!(filters.dates);
    }
    Some(requirement)
}

fn data_type(specifier: &Value) -> Option<&'static str> {
    let named = |specifier: &Value| -> Option<String> {
        let name = specifier["name"].as_str()?;
        Some(name.rsplit('}').next().unwrap_or(name).to_owned())
    };
    if specifier["type"] == "IntervalTypeSpecifier" {
        return match named(&specifier["pointType"])?.as_str() {
            "Date" | "DateTime" => Some("Period"),
            "Quantity" => Some("Range"),
            _ => None,
        };
    }
    match named(specifier)?.as_str() {
        "Boolean" => Some("boolean"),
        "Integer" => Some("integer"),
        "Decimal" => Some("decimal"),
        "String" => Some("string"),
        "Date" => Some("date"),
        "DateTime" => Some("dateTime"),
        "Time" => Some("time"),
        "Quantity" => Some("Quantity"),
        "Code" => Some("Coding"),
        "Concept" => Some("CodeableConcept"),
        _ => None,
    }
}

/// The parameters of the main library. FHIR has no type for the rest, such as a tuple, and
/// `ParameterDefinition.type` is required, so those are left out.
fn parameters(main: &Value) -> Vec<Value> {
    main["parameters"]["def"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|parameter| {
            let fhir_type = data_type(&parameter["parameterTypeSpecifier"])?;
            Some(
                json!({"name": parameter["name"], "use": "in", "min": 0, "max": "1",
                "type": fhir_type}),
            )
        })
        .collect()
}

/// The `module-definition` Library of a compiled library. `dependencies` are the canonicals
/// of the libraries it includes.
pub(crate) fn module_definition(
    library: &CompiledLibrary,
    dependencies: &[String],
) -> CqlResult<Value> {
    let libraries = Libraries::of(library)?;
    let mut requirements: Vec<Value> = Vec::new();
    let mut artifacts: Vec<String> = dependencies.to_vec();
    for elm in libraries.iter() {
        let mut retrieves = Vec::new();
        nodes_of(&elm["statements"], "Retrieve", &mut retrieves);
        let mut queries = Vec::new();
        nodes_of(&elm["statements"], "Query", &mut queries);
        // A query over one retrieve narrows it; with more sources a condition can relate them.
        queries.retain(|query| query["source"].as_array().is_some_and(|all| all.len() == 1));
        for retrieve in retrieves {
            let query = queries
                .iter()
                .find(|query| std::ptr::eq(&query["source"][0]["expression"], retrieve))
                .copied();
            if let Some(requirement) = data_requirement(&libraries, elm, retrieve, query)
                && !requirements.contains(&requirement)
            {
                requirements.push(requirement);
            }
        }
        for section in ["valueSets", "codeSystems"] {
            let definitions = elm[section]["def"].as_array().into_iter().flatten();
            artifacts.extend(definitions.filter_map(canonical));
        }
    }
    artifacts.sort_unstable();
    artifacts.dedup();
    let related: Vec<Value> = artifacts
        .iter()
        .map(|resource| json!({"type": "depends-on", "resource": resource}))
        .collect();
    let mut result = json!({
        "resourceType": "Library",
        "status": "active",
        "type": {"coding": [{"system": LIBRARY_TYPE, "code": "module-definition"}]},
    });
    if !related.is_empty() {
        result["relatedArtifact"] = json!(related);
    }
    let parameters = parameters(&libraries.0[0].1);
    if !parameters.is_empty() {
        result["parameter"] = json!(parameters);
    }
    if !requirements.is_empty() {
        result["dataRequirement"] = json!(requirements);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"library Diabetes version '1.0'
using FHIR version '4.0.1'
codesystem "LOINC": 'http://loinc.org'
valueset "Diabetes": 'http://example.org/ValueSet/diabetes' version '2'
code "HbA1c": '4548-4' from "LOINC" display 'Hemoglobin A1c'
parameter "Measurement Period" Interval<DateTime>
parameter "Threshold" Integer
context Patient
define "Conditions": [Condition: "Diabetes"]
define "Labs": [Observation: "HbA1c"]
define "Again": [Condition: "Diabetes"] C where C.id is not null
define "Encounters": [Encounter]
"#;

    fn definition() -> Value {
        let library = CompiledLibrary::compile(SOURCE).unwrap();
        module_definition(
            &library,
            &["http://example.org/Library/common|1".to_owned()],
        )
        .unwrap()
    }

    fn requirements_of(defines: &str) -> Value {
        let source = format!(
            r#"library Filters version '1'
using FHIR version '4.0.1'
codesystem "S": 'http://example.org/s'
valueset "V": 'http://example.org/v'
code "C": 'x' from "S"
parameter "Period" Interval<DateTime>
context Patient
{defines}
"#
        );
        let library = CompiledLibrary::compile(&source).unwrap();
        module_definition(&library, &[]).unwrap()["dataRequirement"].clone()
    }

    #[test]
    fn where_conditions_become_filters() {
        let requirements = requirements_of(
            r#"define "A": [Encounter] E where E.status = 'finished' and E.class ~ "C"
                and E.priority in "V" and E.status in { 'a', 'b' }
                and E.period.start during "Period""#,
        );
        let encounter = requirements
            .as_array()
            .unwrap()
            .iter()
            .find(|requirement| requirement["type"] == "Encounter")
            .unwrap();
        assert_eq!(
            encounter["codeFilter"],
            json!([
                {"path": "status", "code": [{"code": "finished"}]},
                {"path": "class", "code": [{"system": "http://example.org/s", "code": "x"}]},
                {"path": "priority", "valueSet": "http://example.org/v"},
                {"path": "status", "code": [{"code": "a"}, {"code": "b"}]},
            ])
        );
        assert_eq!(encounter["dateFilter"], json!([{"path": "period.start"}]));
    }

    #[test]
    fn a_condition_that_does_not_narrow_the_retrieve_stays_out() {
        let requirements = requirements_of(
            r#"define "A": [Encounter] E where E.status = 'a' or E.status = 'b'
define "B": [Condition] C where C.id is not null
define "D": [Observation] O where not (O.status = 'final')"#,
        );
        for requirement in requirements.as_array().unwrap() {
            assert!(requirement.get("codeFilter").is_none(), "{requirement}");
        }
    }

    #[test]
    fn retrieves_become_data_requirements_once() {
        let result = definition();
        assert_eq!(result["type"]["coding"][0]["code"], "module-definition");
        let requirements = result["dataRequirement"].as_array().unwrap();
        let of = |name: &str| -> Vec<&Value> {
            requirements
                .iter()
                .filter(|requirement| requirement["type"] == name)
                .collect()
        };
        assert_eq!(
            of("Condition"),
            [&json!({"type":"Condition","codeFilter":[{"path":"code",
                "valueSet":"http://example.org/ValueSet/diabetes|2"}]})]
        );
        assert_eq!(
            of("Observation"),
            [
                &json!({"type":"Observation","codeFilter":[{"path":"code","code":[
                {"system":"http://loinc.org","code":"4548-4","display":"Hemoglobin A1c"}]}]})
            ]
        );
        assert_eq!(of("Encounter"), [&json!({"type":"Encounter"})]);
    }

    #[test]
    fn terminology_libraries_and_parameters_are_declared() {
        let result = definition();
        let related: Vec<_> = result["relatedArtifact"]
            .as_array()
            .unwrap()
            .iter()
            .map(|artifact| artifact["resource"].as_str().unwrap())
            .collect();
        assert_eq!(
            related,
            [
                "http://example.org/Library/common|1",
                "http://example.org/ValueSet/diabetes|2",
                "http://loinc.org"
            ]
        );
        assert_eq!(
            result["parameter"],
            json!([
                {"name":"Measurement Period","use":"in","min":0,"max":"1","type":"Period"},
                {"name":"Threshold","use":"in","min":0,"max":"1","type":"integer"}
            ])
        );
    }
}
