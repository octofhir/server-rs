//! Typed, metadata-only UPS request projection from an existing FHIR Task.
//!
//! # Contents
//! Explicit coded work, bounded processing parameters, immutable patient/input
//! identity, standards-based creation and output-reference parsing.
//!
//! # Invariants
//! No DICOM dataset or pixel data is persisted in commands. The Task is bound to
//! one already attested ImagingStudy. Unknown codes/parameters fail explicitly.
//! All network URLs are constructed from administrator configuration.
use super::config::DicomUpsConfig;
use crate::dicom::{DicomConnection, mapping};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
    pub version: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(super) struct Instance {
    pub study: String,
    pub series: String,
    pub uid: String,
    pub class: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub task_id: String,
    pub task_hash: String,
    pub task_projection: Value,
    pub patient_id: String,
    pub study_id: String,
    pub study_uid: String,
    pub patient_identifier: String,
    pub issuer: String,
    pub code: Code,
    pub parameters: Vec<Parameter>,
    pub priority: String,
    pub label: String,
    pub start: String,
    pub deadline: Option<String>,
    pub instances: Vec<Instance>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameter {
    concept: Code,
    value: ParameterValue,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum ParameterValue {
    Text(String),
    Code(Code),
    DateTime(String),
    Numeric { number: String, units: Code },
}
fn has_modifiers(value: &Value) -> bool {
    value
        .get("modifierExtension")
        .is_some_and(|value| value.as_array().is_none_or(|values| !values.is_empty()))
}
fn invalid() -> String {
    "Task requires explicit supported UPS codes, parameters and one stable patient/study identity"
        .into()
}
fn dt(value: &str) -> Result<String, String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|v| v.format("%Y%m%d%H%M%S%.6f%z").to_string())
        .map_err(|_| invalid())
}
pub(super) fn task_projection(task: &Value) -> Value {
    json!({"id":task["id"],"for":task["for"],"focus":task["focus"],"code":task["code"],"input":task["input"],"priority":task["priority"],"restriction":task["restriction"],"requestedPeriod":task["requestedPeriod"],"doNotPerform":task["doNotPerform"],"_doNotPerform":task["_doNotPerform"],"modifierExtension":task["modifierExtension"],"owner":task["owner"],"performerType":task["performerType"],"requestedPerformer":task["requestedPerformer"],"location":task["location"],"intent":task["intent"]})
}
pub(super) fn task_hash(task: &Value) -> String {
    let mut projection = task_projection(task);
    projection.sort_all_objects();
    normalize_numbers(&mut projection);
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json::to_vec(&projection).expect("JSON Task projection"),
    ))
}
// PostgreSQL JSONB compares 1 and 1.0 as the same value. Keep the durable hash
// consistent with that equality when a client reserializes an exact Quantity.
fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            let number = number.as_f64().expect("JSON float");
            if number.fract() == 0.0 {
                if (-9223372036854775808.0..9223372036854775808.0).contains(&number) {
                    *value = json!(number as i64);
                } else if (0.0..18446744073709551616.0).contains(&number) {
                    *value = json!(number as u64);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_numbers(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                normalize_numbers(value);
            }
        }
        _ => {}
    }
}
fn code(coding: &Value, config: &DicomUpsConfig) -> Result<Code, String> {
    let scheme = config
        .coding_schemes
        .get(coding["system"].as_str().ok_or_else(invalid)?)
        .ok_or_else(invalid)?
        .clone();
    let value = coding["code"]
        .as_str()
        .filter(|v| {
            !v.trim().is_empty()
                && v.len() <= 1024
                && !v.chars().any(char::is_control)
                && !v.contains('\\')
        })
        .ok_or_else(invalid)?
        .to_owned();
    let meaning = coding["display"]
        .as_str()
        .filter(|v| {
            !v.trim().is_empty()
                && v.chars().count() <= 64
                && !v.chars().any(char::is_control)
                && !v.contains('\\')
        })
        .ok_or_else(invalid)?
        .to_owned();
    let version = coding
        .get("version")
        .map(|v| {
            v.as_str()
                .filter(|v| super::config::text(v, 16))
                .map(Into::into)
                .ok_or_else(invalid)
        })
        .transpose()?;
    Ok(Code {
        value,
        scheme,
        meaning,
        version,
    })
}
fn concept(value: &Value, config: &DicomUpsConfig) -> Result<Code, String> {
    let items = value["coding"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or_else(invalid)?;
    code(&items[0], config)
}
pub(super) fn request(
    task: &Value,
    patient: &Value,
    study: &Value,
    study_uid: &str,
    connection: &DicomConnection,
) -> Result<Request, String> {
    if task
        .get("doNotPerform")
        .is_some_and(|value| value.as_bool() != Some(false))
        || task.get("_doNotPerform").is_some()
        || has_modifiers(task)
    {
        return Err(
            "Prohibited/modified Tasks require explicit semantic support; no UPS is submitted"
                .into(),
        );
    }
    if has_modifiers(&task["restriction"])
        || task["restriction"]
            .get("repetitions")
            .is_some_and(|value| value.as_u64() != Some(1))
    {
        return Err("UPS Task repetition/modifier semantics are unsupported".into());
    }
    let config = connection.ups.as_ref().ok_or_else(invalid)?;
    let patient_id = patient["id"].as_str().ok_or_else(invalid)?;
    let study_id = study["id"].as_str().ok_or_else(invalid)?;
    if task["for"]["reference"] != format!("Patient/{patient_id}")
        || task["focus"]["reference"] != format!("ImagingStudy/{study_id}")
        || task["intent"] != "order"
    {
        return Err(invalid());
    }
    let mut identifiers = Vec::new();
    for (issuer, system) in &connection.patient_identifier_systems {
        for id in patient["identifier"].as_array().into_iter().flatten() {
            if id["system"] == *system
                && let Some(value) = id["value"].as_str().filter(|v| !v.trim().is_empty())
            {
                identifiers.push((issuer.clone(), value.to_owned()));
            }
        }
    }
    if identifiers.len() != 1 {
        return Err(
            "UPS Task requires exactly one configured assigning-authority identifier".into(),
        );
    }
    let (issuer, patient_identifier) = identifiers.pop().unwrap();
    if issuer.chars().count() > 64
        || patient_identifier.chars().count() > 64
        || [&issuer, &patient_identifier]
            .iter()
            .any(|v| v.chars().any(char::is_control) || v.contains('\\'))
    {
        return Err(invalid());
    }
    let work_code = concept(&task["code"], config)?;
    let mut parameters = Vec::new();
    if let Some(inputs) = task.get("input") {
        for input in inputs
            .as_array()
            .filter(|v| v.len() <= 100)
            .ok_or_else(invalid)?
        {
            if has_modifiers(input) {
                return Err(
                    "UPS Task.input modifier extensions require explicit semantic support".into(),
                );
            }
            let concept = concept(&input["type"], config)?;
            let values: Vec<_> = input
                .as_object()
                .ok_or_else(invalid)?
                .iter()
                .filter(|(k, _)| k.starts_with("value"))
                .collect();
            if values.len() != 1 {
                return Err(invalid());
            }
            let (key, value) = values[0];
            let value=match key.as_str(){
   "valueString"=>ParameterValue::Text(value.as_str().filter(|v|v.len()<=65536&&!v.contains('\0')).ok_or_else(invalid)?.into()),
   "valueDateTime"=>ParameterValue::DateTime(dt(value.as_str().ok_or_else(invalid)?)?),
   "valueCodeableConcept"=>ParameterValue::Code(concept_code(value,config)?),
   "valueQuantity"=>{
    if value.get("comparator").is_some(){return Err(invalid());}
    let number=value["value"].as_number().ok_or_else(invalid)?.to_string();if number.len()>16{return Err(invalid());}
    let units=code(&json!({"system":value["system"],"code":value["code"],"display":value["unit"].as_str().or_else(||value["code"].as_str()).ok_or_else(invalid)?}),config)?;
    ParameterValue::Numeric{number,units}
   },_=>return Err("Unsupported UPS Task.input value type; supported: String, DateTime, CodeableConcept and exact Quantity".into())
  };
            parameters.push(Parameter { concept, value });
        }
    }
    let mut instances = Vec::new();
    for series in study["series"].as_array().ok_or_else(invalid)? {
        for instance in series["instance"].as_array().ok_or_else(invalid)? {
            let class = mapping::sop_class(instance).map_err(|_| invalid())?;
            let item = Instance {
                study: study_uid.into(),
                series: series["uid"].as_str().ok_or_else(invalid)?.into(),
                uid: instance["uid"].as_str().ok_or_else(invalid)?.into(),
                class: class.into(),
            };
            validate_instance(&item)?;
            instances.push(item);
        }
    }
    if instances.is_empty() || instances.len() > config.max_references {
        return Err("UPS input instance count exceeds configured bounds".into());
    }
    instances.sort();
    let task_id = task["id"].as_str().ok_or_else(invalid)?.to_owned();
    let hash = task_hash(task);
    let priority = match task["priority"].as_str().unwrap_or("routine") {
        "routine" => "MEDIUM",
        "urgent" | "asap" | "stat" => "HIGH",
        _ => return Err(invalid()),
    }
    .into();
    if task.get("requestedPeriod").is_some() && task["restriction"].get("period").is_some() {
        return Err("Choose one UPS requested period".into());
    }
    let period = task
        .get("requestedPeriod")
        .unwrap_or(&task["restriction"]["period"]);
    let start = period["start"]
        .as_str()
        .map(dt)
        .transpose()?
        .unwrap_or_else(|| chrono::Utc::now().format("%Y%m%d%H%M%S%.6f%z").to_string());
    let deadline = period["end"].as_str().map(dt).transpose()?;
    if ["owner", "performerType", "requestedPerformer", "location"]
        .iter()
        .any(|field| task.get(*field).is_some())
        || task["restriction"].get("recipient").is_some()
    {
        return Err("UPS performer/location selection uses configured workitem codes; Task performer/location mappings are unsupported".into());
    }
    let request = Request {
        task_id,
        task_hash: hash.clone(),
        task_projection: task_projection(task),
        patient_id: patient_id.into(),
        study_id: study_id.into(),
        study_uid: study_uid.into(),
        patient_identifier,
        issuer,
        code: work_code,
        parameters,
        priority,
        label: format!("OctoFHIR {}", &hash[..48]),
        start,
        deadline,
        instances,
    };
    if serde_json::to_vec(&request).map_err(|_| invalid())?.len() > 2 * 1024 * 1024 {
        return Err("UPS command metadata exceeds 2 MiB".into());
    }
    Ok(request)
}
fn concept_code(value: &Value, config: &DicomUpsConfig) -> Result<Code, String> {
    concept(value, config)
}
fn attr(vr: &str, value: &str) -> Value {
    json!({"vr":vr,"Value":[value]})
}
fn sq(values: Vec<Value>) -> Value {
    json!({"vr":"SQ","Value":values})
}
fn code_dataset(code: &Code) -> Value {
    let mut result =
        json!({"00080102":attr("SH",&code.scheme),"00080104":attr("LO",&code.meaning)});
    if code.value.starts_with("urn:") || code.value.contains("://") {
        result["00080120"] = attr("UR", &code.value);
    } else if code.value.chars().count() <= 16 {
        result["00080100"] = attr("SH", &code.value);
    } else {
        result["00080119"] = attr("UC", &code.value);
    }
    if let Some(version) = &code.version {
        result["00080103"] = attr("SH", version);
    }
    result
}
impl Request {
    pub(super) fn dataset(&self, connection: &DicomConnection) -> Result<Value, String> {
        let mut dataset = json!({"00080005":attr("CS","ISO_IR 192"),"00741200":attr("CS",&self.priority),"00741204":attr("LO",&self.label),"00404005":attr("DT",&self.start),"00404018":sq(vec![code_dataset(&self.code)]),"00404041":attr("CS","READY"),"00100020":attr("LO",&self.patient_identifier),"00100021":attr("LO",&self.issuer),"00741000":attr("CS","SCHEDULED")});
        if let Some(deadline) = &self.deadline {
            dataset["00404011"] = attr("DT", deadline);
        }
        for (tag, vr) in [
            ("00081195", "UI"),
            ("00741202", "LO"),
            ("00400400", "LT"),
            ("00100010", "PN"),
            ("00100030", "DA"),
            ("00100040", "CS"),
            ("00380010", "LO"),
            ("00081080", "LO"),
        ] {
            dataset[tag] = json!({"vr":vr});
        }
        for tag in [
            "00404025", "00404026", "00404027", "00100024", "00101002", "00380014", "00081084",
            "0040A370", "00741002", "00741216",
        ] {
            dataset[tag] = sq(vec![]);
        }
        dataset["00741210"] = self.processing_parameters();
        dataset["00404021"]=sq(self.instances.iter().map(|instance|{
   let mut url=connection.base_url().map_err(|_|invalid())?;url.path_segments_mut().map_err(|_|invalid())?.pop_if_empty().extend(["studies",&instance.study,"series",&instance.series,"instances",&instance.uid]);
   Ok(json!({"0040E020":attr("CS","DICOM"),"0020000D":attr("UI",&instance.study),"0020000E":attr("UI",&instance.series),"00081199":sq(vec![json!({"00081150":attr("UI",&instance.class),"00081155":attr("UI",&instance.uid)})]),"0040E025":sq(vec![json!({"00081190":attr("UR",url.as_str())})])}))
  }).collect::<Result<Vec<_>,String>>()?);
        if let Some(url) = connection
            .ups
            .as_ref()
            .and_then(|u| u.output_stow_url.as_ref())
        {
            dataset["00404070"] = sq(vec![
                json!({"00404072":sq(vec![json!({"00404073":attr("UR",url)})])}),
            ]);
        }
        Ok(dataset)
    }
    fn processing_parameters(&self) -> Value {
        sq(self
            .parameters
            .iter()
            .map(|p| {
                let mut item = json!({"0040A043":sq(vec![code_dataset(&p.concept)])});
                match &p.value {
                    ParameterValue::Text(value) => {
                        item["0040A040"] = attr("CS", "TEXT");
                        item["0040A160"] = attr("UT", value);
                    }
                    ParameterValue::Code(value) => {
                        item["0040A040"] = attr("CS", "CODE");
                        item["0040A168"] = sq(vec![code_dataset(value)]);
                    }
                    ParameterValue::DateTime(value) => {
                        item["0040A040"] = attr("CS", "DATETIME");
                        item["0040A120"] = attr("DT", value);
                    }
                    ParameterValue::Numeric { number, units } => {
                        item["0040A040"] = attr("CS", "NUMERIC");
                        item["0040A30A"] = attr("DS", number);
                        item["004008EA"] = sq(vec![code_dataset(units)]);
                    }
                }
                item
            })
            .collect())
    }
    pub(super) fn verify(&self, dataset: &Value, uid: &str) -> Result<(), String> {
        for (tag, vr, value) in [
            ("00741204", "LO", self.label.as_str()),
            ("00100020", "LO", self.patient_identifier.as_str()),
            ("00100021", "LO", self.issuer.as_str()),
        ] {
            if mapping::required(dataset, tag, vr).map_err(|_| invalid())? != value {
                return Err(
                    "Existing UPS workitem does not match its immutable request identity".into(),
                );
            }
        }
        if dataset.get("00080018").is_some()
            && mapping::required(dataset, "00080018", "UI").map_err(|_| invalid())? != uid
        {
            return Err(invalid());
        }
        let codes = dataset["00404018"]["Value"]
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or_else(invalid)?;
        let expected = code_dataset(&self.code);
        for tag in ["00080100", "00080119", "00080120", "00080102", "00080103"] {
            if codes[0].get(tag) != expected.get(tag) {
                return Err("Existing UPS scheduled code differs from its request".into());
            }
        }
        if dataset.get("00080016").is_some()
            && mapping::required(dataset, "00080016", "UI").map_err(|_| invalid())?
                != "1.2.840.10008.5.1.4.34.6.1"
        {
            return Err("UPS workitem has an unexpected SOP Class".into());
        }
        let mut expected_parameters = self.processing_parameters();
        let mut actual_parameters = dataset
            .get("00741210")
            .cloned()
            .unwrap_or_else(|| sq(vec![]));
        canonical_parameters(&mut expected_parameters)?;
        canonical_parameters(&mut actual_parameters)?;
        if actual_parameters != expected_parameters {
            return Err("UPS processing parameters differ from the immutable Task request".into());
        }
        let actual = references(dataset, "00404021", self.instances.len())?;
        if actual != self.instances {
            return Err("Existing UPS inputs differ from the authorized manifest".into());
        }
        Ok(())
    }
}
// DICOM JSON permits DS as a number or a string and omits Value for empty SQ.
// Compare the complete bounded content tree, without accepting extra parameters.
fn canonical_parameters(value: &mut Value) -> Result<(), String> {
    fn walk(value: &mut Value, depth: usize) -> Result<(), String> {
        if depth > 16 {
            return Err(invalid());
        }
        match value {
            Value::Object(object) => {
                if object.get("vr").and_then(Value::as_str) == Some("SQ")
                    && !object.contains_key("Value")
                {
                    object.insert("Value".into(), json!([]));
                }
                if object.get("vr").and_then(Value::as_str) == Some("DS")
                    && let Some(values) = object.get_mut("Value").and_then(Value::as_array_mut)
                {
                    for number in values {
                        let text = match &*number {
                            Value::String(text) => text.clone(),
                            Value::Number(number) => number.to_string(),
                            _ => return Err(invalid()),
                        };
                        *number = Value::String(canonical_decimal(&text)?);
                    }
                }
                for child in object.values_mut() {
                    walk(child, depth + 1)?;
                }
            }
            Value::Array(values) => {
                for child in values {
                    walk(child, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(value, 0)?;
    normalize_numbers(value);
    value.sort_all_objects();
    Ok(())
}
// Normalize DS exactly: a binary float would alias adjacent 16-digit integers.
fn canonical_decimal(text: &str) -> Result<String, String> {
    if text.is_empty() || text.len() > 64 {
        return Err(invalid());
    }
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().map_err(|_| invalid())?),
        None => (text, 0),
    };
    if !(-1000..=1000).contains(&exponent) {
        return Err(invalid());
    }
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.strip_prefix(['-', '+']).unwrap_or(mantissa);
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut digits = format!("{integer}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|v| v.is_ascii_digit()) {
        return Err(invalid());
    }
    digits = digits.trim_start_matches('0').to_owned();
    if digits.is_empty() {
        return Ok("0".into());
    }
    let trailing = digits.len() - digits.trim_end_matches('0').len();
    digits.truncate(digits.len() - trailing);
    let exponent = exponent - fraction.len() as i32 + trailing as i32;
    Ok(format!(
        "{}{digits}e{exponent}",
        if negative { "-" } else { "" }
    ))
}
fn validate_instance(instance: &Instance) -> Result<(), String> {
    for uid in [
        &instance.study,
        &instance.series,
        &instance.uid,
        &instance.class,
    ] {
        super::super::validate_uid(uid).map_err(|_| invalid())?;
    }
    Ok(())
}
pub(super) fn references(dataset: &Value, tag: &str, max: usize) -> Result<Vec<Instance>, String> {
    let mut result = BTreeSet::new();
    let mut seen_uids = BTreeSet::new();
    let Some(attribute) = dataset.get(tag) else {
        return Ok(vec![]);
    };
    if attribute["vr"] != "SQ" {
        return Err(invalid());
    }
    let Some(items) = attribute.get("Value") else {
        return Ok(vec![]);
    };
    for item in items.as_array().ok_or_else(invalid)? {
        if mapping::required(item, "0040E020", "CS").map_err(|_| invalid())? != "DICOM" {
            return Err(
                "UPS references non-DICOM output; explicit mapping support is required".into(),
            );
        }
        let study = mapping::required(item, "0020000D", "UI").map_err(|_| invalid())?;
        let series = mapping::required(item, "0020000E", "UI").map_err(|_| invalid())?;
        if item["00081199"]["vr"] != "SQ" {
            return Err(invalid());
        }
        for sop in item["00081199"]["Value"].as_array().ok_or_else(invalid)? {
            if sop.get("00081160").is_some() || sop.get("0062000B").is_some() {
                return Err(
                    "UPS frame/segment subsets require an explicit result mapping implementation"
                        .into(),
                );
            }
            let instance = Instance {
                study: study.into(),
                series: series.into(),
                uid: mapping::required(sop, "00081155", "UI")
                    .map_err(|_| invalid())?
                    .into(),
                class: mapping::required(sop, "00081150", "UI")
                    .map_err(|_| invalid())?
                    .into(),
            };
            validate_instance(&instance)?;
            if !seen_uids.insert(instance.uid.clone())
                || !result.insert(instance)
                || result.len() > max
            {
                return Err("Duplicate or excessive UPS references".into());
            }
        }
    }
    Ok(result.into_iter().collect())
}
pub(super) fn state(dataset: &Value) -> Result<&str, String> {
    let state = mapping::required(dataset, "00741000", "CS").map_err(|_| invalid())?;
    match state {
        // Normalize the PACS spelling at ingress so transitions, persisted state,
        // publication and subscription cleanup all use the standard spelling.
        "CANCELLED" => Ok("CANCELED"),
        "SCHEDULED" | "IN PROGRESS" | "COMPLETED" | "CANCELED" => Ok(state),
        _ => Err("Unsupported UPS procedure state".into()),
    }
}

/// PS3.4 CC.2.5 places Output Information inside each Performed Procedure item,
/// not at the workitem root. A COMPLETED state without that information is not a
/// complete snapshot suitable for clinical publication.
pub(super) fn output_references(dataset: &Value, max: usize) -> Result<Vec<Instance>, String> {
    let sequence = &dataset["00741216"];
    if sequence["vr"] != "SQ" {
        return Err("Completed UPS workitem lacks performed procedure information".into());
    }
    let procedures = sequence["Value"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or("Completed UPS workitem lacks performed procedure items")?;
    let mut result = BTreeSet::new();
    let mut uids = std::collections::BTreeMap::new();
    for procedure in procedures {
        if procedure.get("00404033").is_none() {
            return Err("Completed UPS procedure lacks its output information sequence".into());
        }
        for instance in references(procedure, "00404033", max)? {
            if let Some(previous) = uids.insert(instance.uid.clone(), instance.clone())
                && previous != instance
            {
                return Err("UPS output UID has inconsistent study/series/class references".into());
            }
            result.insert(instance);
            if result.len() > max {
                return Err("UPS output count exceeds configured limits".into());
            }
        }
    }
    Ok(result.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Value, Value, DicomConnection) {
        let task = json!({"resourceType":"Task","id":"t","status":"draft","intent":"order","for":{"reference":"Patient/p"},"focus":{"reference":"ImagingStudy/s"},"code":{"coding":[{"system":"urn:test:procedure","code":"TEST","display":"Synthetic processing"}]}});
        let patient = json!({"id":"p","identifier":[{"system":"urn:test:mrn","value":"MRN-1"}]});
        let study = json!({"id":"s","series":[{"uid":"1.2.3.1","instance":[{"uid":"1.2.3.1.1","sopClass":{"system":"urn:ietf:rfc:3986","code":"urn:oid:1.2.840.10008.5.1.4.1.1.2"}}]}]});
        let config=serde_json::from_value(json!({"wado_url":"https://pacs.example/dicom","patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"},"ups":{"url":"https://pacs.example/ups","client_id":"service","scopes":["system/*.cruds"],"ae_title":"OCTOFHIR","coding_schemes":{"urn:test:procedure":"99TEST","http://unitsofmeasure.org":"UCUM"}}})).unwrap();
        (task, patient, study, config)
    }
    #[test]
    fn decimal_comparison_is_exact_for_large_values_and_exponents() {
        assert_eq!(
            canonical_decimal("0.70").unwrap(),
            canonical_decimal("7e-1").unwrap()
        );
        assert_eq!(
            canonical_decimal("-0").unwrap(),
            canonical_decimal("+0.00").unwrap()
        );
        assert_ne!(
            canonical_decimal("9007199254740992").unwrap(),
            canonical_decimal("9007199254740993").unwrap()
        );
        for bad in ["NaN", "1e99999", "1.2.3", "1e1e2", "--1"] {
            assert!(canonical_decimal(bad).is_err());
        }
    }
    #[test]
    fn processing_parameters_compare_dicom_json_numbers_and_reject_changes() {
        let mut text = json!({"vr":"SQ","Value":[{"0040A30A":{"vr":"DS","Value":["1.0"]}}]});
        let mut numeric = json!({"vr":"SQ","Value":[{"0040A30A":{"vr":"DS","Value":[1]}}]});
        canonical_parameters(&mut text).unwrap();
        canonical_parameters(&mut numeric).unwrap();
        assert_eq!(text, numeric);
        numeric["Value"][0]["0040A30A"]["Value"][0] = json!(2);
        assert_ne!(text, numeric);
    }
    #[test]
    fn creates_standard_metadata_without_sop_instance_or_performer_transaction() {
        let (task, patient, study, config) = fixture();
        let request = request(&task, &patient, &study, "1.2.3", &config).unwrap();
        let dataset = request.dataset(&config).unwrap();
        assert!(dataset.get("00080018").is_none());
        assert!(dataset.get("0020000D").is_none());
        assert!(dataset["00081195"].get("Value").is_none());
        assert_eq!(dataset["00741000"]["Value"][0], "SCHEDULED");
        assert!(dataset["00741216"]["Value"].as_array().unwrap().is_empty());
        assert_eq!(
            references(&dataset, "00404021", 10).unwrap(),
            request.instances
        );
        assert!(
            dataset["00404021"]["Value"][0]["0040E025"]["Value"][0]["00081190"]["Value"][0]
                .as_str()
                .unwrap()
                .ends_with("/studies/1.2.3/series/1.2.3.1/instances/1.2.3.1.1")
        );
        assert!(!serde_json::to_string(&request).unwrap().contains("\"vr\""));
    }
    #[test]
    fn normalizes_remote_cancelled_state() {
        for spelling in ["CANCELED", "CANCELLED"] {
            let dataset = json!({"00741000":attr("CS", spelling)});
            assert_eq!(state(&dataset).unwrap(), "CANCELED");
        }
        assert!(state(&json!({"00741000":attr("CS", "UNKNOWN")})).is_err());
    }
    #[test]
    fn rejects_negated_or_unknown_modifier_semantics_and_unmapped_performers() {
        let (task, patient, study, config) = fixture();
        for change in [
            json!({"doNotPerform":true}),
            json!({"doNotPerform":"true"}),
            json!({"modifierExtension":[{"url":"http://hl7.org/fhir/StructureDefinition/request-doNotPerform","valueBoolean":true}]}),
            json!({"requestedPerformer":[{"reference":{"reference":"Device/x"}}]}),
            json!({"restriction":{"repetitions":2}}),
        ] {
            let mut modified = task.clone();
            modified
                .as_object_mut()
                .unwrap()
                .extend(change.as_object().unwrap().clone());
            assert!(
                request(&modified, &patient, &study, "1.2.3", &config).is_err(),
                "{change}"
            );
        }
    }
    #[test]
    fn postgres_equivalent_numbers_and_annotations_keep_the_same_task_binding() {
        let (mut task, patient, study, config) = fixture();
        task["input"] = json!([{"type":{"coding":[{"system":"urn:test:procedure","code":"SIZE","display":"Threshold"}]},"valueQuantity":{"value":1.0,"system":"http://unitsofmeasure.org","code":"mm","unit":"millimetre"}}]);
        let before = task_hash(&task);
        let request = request(&task, &patient, &study, "1.2.3", &config).unwrap();
        assert_eq!(
            request.dataset(&config).unwrap()["00741210"]["Value"][0]["0040A040"]["Value"][0],
            "NUMERIC"
        );
        task["input"][0]["valueQuantity"]["value"] = json!(1);
        task["note"] = json!([{"text":"Clinical annotation"}]);
        task["status"] = json!("in-progress");
        assert_eq!(task_hash(&task), before);
        task["for"]["reference"] = json!("Patient/other");
        assert_ne!(task_hash(&task), before);
    }
    #[test]
    fn reads_nested_performed_outputs_and_refuses_missing_or_subset_information() {
        let reference = json!({"0040E020":{"vr":"CS","Value":["DICOM"]},"0020000D":{"vr":"UI","Value":["1.2.3"]},"0020000E":{"vr":"UI","Value":["1.2.3.2"]},"00081199":{"vr":"SQ","Value":[{"00081150":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.88.22"]},"00081155":{"vr":"UI","Value":["1.2.3.2.1"]}}]}});
        let mut workitem =
            json!({"00741216":{"vr":"SQ","Value":[{"00404033":{"vr":"SQ","Value":[reference]}}]}});
        assert_eq!(
            output_references(&workitem, 10).unwrap()[0].uid,
            "1.2.3.2.1"
        );
        workitem["00741216"]["Value"][0]["00404033"]["Value"][0]["00081199"]["Value"][0]["0062000B"] =
            json!({"vr":"US","Value":[1]});
        assert!(output_references(&workitem, 10).is_err());
        assert!(output_references(&json!({"00404033":{"vr":"SQ"}}), 10).is_err());
        assert!(
            output_references(
                &json!({"00741216":{"vr":"SQ","Value":[{"00404033":{"vr":"SQ"}}]}}),
                10
            )
            .unwrap()
            .is_empty()
        );
    }
}
