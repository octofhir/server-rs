//! `Measure/$evaluate-measure`: population evaluation and scoring on CQL.
//!
//! The Measure's library is compiled once and evaluated for each subject. A population
//! criterion names a CQL define: `true`, or a non-empty list, puts the subject in it.
//! Measures are read as patient-based. Proportion, ratio and cohort scoring are
//! implemented; continuous variable needs observation functions and is refused.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{StreamExt, TryStreamExt, stream};
use indexmap::IndexMap;
use octofhir_core::fhir_reference::parse_reference;
use octofhir_storage::{SearchConstraint, SearchParams, StoredResource};
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::cql::{map_cql_error, scoped_service};
use super::handler::OperationLevel;
use super::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// Subjects evaluated by one compiled library on one blocking thread.
const SUBJECT_CHUNK: usize = 32;

/// Chunks evaluated at once.
const CHUNK_CONCURRENCY: usize = 4;

const MEASURE_POPULATION: &str = "http://terminology.hl7.org/CodeSystem/measure-population";

/// The population codes this evaluator understands, in report order.
const POPULATIONS: [&str; 6] = [
    "initial-population",
    "denominator",
    "denominator-exclusion",
    "denominator-exception",
    "numerator",
    "numerator-exclusion",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scoring {
    Proportion,
    Ratio,
    Cohort,
}

impl Scoring {
    fn of(measure: &Value) -> Result<Self, OperationError> {
        let code = measure["scoring"]["coding"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|coding| coding["code"].as_str());
        match code {
            Some("proportion") => Ok(Self::Proportion),
            Some("ratio") => Ok(Self::Ratio),
            Some("cohort") => Ok(Self::Cohort),
            Some(other) => Err(OperationError::NotSupported(format!(
                "Measure scoring {other} is not supported"
            ))),
            None => Err(OperationError::InvalidParameters(
                "The Measure has no scoring".into(),
            )),
        }
    }
}

/// `true`, or a list with something in it.
fn in_population(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Array(items)) => !items.is_empty(),
        _ => false,
    }
}

/// Where one subject falls in a group, per the population rules of the scoring.
fn classify(scoring: Scoring, criteria: impl Fn(&str) -> bool) -> [bool; 6] {
    let initial = criteria("initial-population");
    if scoring == Scoring::Cohort {
        return [initial, false, false, false, false, false];
    }
    let denominator = initial && criteria("denominator");
    let exclusion = denominator && criteria("denominator-exclusion");
    // A ratio's numerator does not depend on its denominator.
    let numerator_base = match scoring {
        Scoring::Ratio => initial,
        _ => denominator && !exclusion,
    };
    let numerator = numerator_base && criteria("numerator");
    let numerator_exclusion = numerator && criteria("numerator-exclusion");
    let exception = scoring == Scoring::Proportion
        && denominator
        && !exclusion
        && !numerator
        && criteria("denominator-exception");
    [
        initial,
        denominator,
        exclusion,
        exception,
        numerator,
        numerator_exclusion,
    ]
}

/// Population counts of a group or stratum.
#[derive(Debug, Clone, Default)]
struct Tally {
    counts: [u64; 6],
    subjects: [Vec<String>; 6],
}

impl Tally {
    fn add(&mut self, memberships: [bool; 6], subject: &str) {
        for (index, member) in memberships.into_iter().enumerate() {
            if member {
                self.counts[index] += 1;
                self.subjects[index].push(subject.to_owned());
            }
        }
    }

    fn score(&self, scoring: Scoring) -> Option<f64> {
        let [
            _,
            denominator,
            exclusion,
            exception,
            numerator,
            numerator_exclusion,
        ] = self.counts;
        let top = numerator.checked_sub(numerator_exclusion)?;
        let bottom = match scoring {
            Scoring::Cohort => return None,
            Scoring::Proportion => denominator.checked_sub(exclusion + exception)?,
            Scoring::Ratio => denominator.checked_sub(exclusion)?,
        };
        (bottom > 0).then(|| top as f64 / bottom as f64)
    }

    fn populations(&self, used: &[&str], list_id: Option<&str>) -> Vec<Value> {
        POPULATIONS
            .iter()
            .enumerate()
            .filter(|(_, code)| used.contains(code))
            .map(|(index, code)| {
                let mut population = json!({
                    "code": {"coding": [{"system": MEASURE_POPULATION, "code": code}]},
                    "count": self.counts[index],
                });
                if let Some(prefix) = list_id {
                    population["subjectResults"] =
                        json!({"reference": format!("#{prefix}-{code}")});
                }
                population
            })
            .collect()
    }
}

/// One `Measure.group` as the evaluator uses it.
struct Group<'m> {
    source: &'m Value,
    /// Population code to the name of its define.
    criteria: BTreeMap<&'m str, &'m str>,
    stratifiers: Vec<&'m str>,
}

impl<'m> Group<'m> {
    fn parse(source: &'m Value) -> Result<Self, OperationError> {
        let mut criteria = BTreeMap::new();
        for population in source["population"].as_array().into_iter().flatten() {
            let code = population["code"]["coding"][0]["code"]
                .as_str()
                .ok_or_else(|| {
                    OperationError::InvalidParameters("A population has no code".into())
                })?;
            if !POPULATIONS.contains(&code) {
                return Err(OperationError::NotSupported(format!(
                    "Population {code} is not supported"
                )));
            }
            let expression = population["criteria"]["expression"]
                .as_str()
                .ok_or_else(|| {
                    OperationError::InvalidParameters(format!(
                        "Population {code} has no expression"
                    ))
                })?;
            criteria.insert(code, expression);
        }
        let stratifiers = source["stratifier"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|stratifier| stratifier["criteria"]["expression"].as_str())
            .collect();
        Ok(Self {
            source,
            criteria,
            stratifiers,
        })
    }

    fn memberships(&self, scoring: Scoring, defines: &IndexMap<String, Value>) -> [bool; 6] {
        classify(scoring, |code| {
            self.criteria
                .get(code)
                .is_some_and(|name| in_population(defines.get(*name)))
        })
    }
}

/// A stratum's key: the define's value as text.
fn stratum_key(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => "null".to_owned(),
        Some(other) => other.to_string(),
    }
}

fn contained_lists(prefix: &str, used: &[&str], tally: &Tally, now: &str) -> Vec<Value> {
    POPULATIONS
        .iter()
        .enumerate()
        .filter(|(_, code)| used.contains(code))
        .map(|(index, code)| {
            let entries: Vec<Value> = tally.subjects[index]
                .iter()
                .map(|subject| json!({"item": {"reference": subject}}))
                .collect();
            json!({"resourceType": "List", "id": format!("{prefix}-{code}"),
                "status": "current", "mode": "working", "date": now, "entry": entries})
        })
        .collect()
}

/// What a request asks for.
struct Request {
    period: (String, String),
    report_type: Option<String>,
    subject: Option<String>,
}

fn text_parameter<'a>(params: &'a Value, name: &str) -> Option<&'a str> {
    params["parameter"]
        .as_array()?
        .iter()
        .find(|parameter| parameter["name"] == name)?
        .as_object()?
        .iter()
        .find(|(key, _)| key.starts_with("value"))?
        .1
        .as_str()
}

/// A date widened to the instant at the start or end of that day.
fn instant(value: &str, end: bool) -> String {
    if value.contains('T') {
        return value.to_owned();
    }
    let date = match value.len() {
        4 if end => format!("{value}-12-31"),
        4 => format!("{value}-01-01"),
        _ => value.to_owned(),
    };
    let time = if end { "23:59:59.999" } else { "00:00:00.000" };
    format!("{date}T{time}Z")
}

impl Request {
    fn parse(params: &Value) -> Result<Self, OperationError> {
        let required = |name: &str| {
            text_parameter(params, name)
                .ok_or_else(|| OperationError::InvalidParameters(format!("{name} is required")))
        };
        Ok(Self {
            period: (
                instant(required("periodStart")?, false),
                instant(required("periodEnd")?, true),
            ),
            report_type: text_parameter(params, "reportType").map(str::to_owned),
            subject: text_parameter(params, "subject").map(str::to_owned),
        })
    }
}

async fn read_patients(
    state: &AppState,
    context: &OperationContext,
    ids: &[String],
) -> Result<Vec<StoredResource>, OperationError> {
    let mut patients = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        patients.extend(context.read_many(state, "Patient", chunk).await?);
    }
    Ok(patients)
}

/// Every Patient the caller may read, in id order.
async fn all_patients(
    state: &AppState,
    context: &OperationContext,
) -> Result<Vec<StoredResource>, OperationError> {
    let mut patients = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut search = SearchParams::new().with_count(500).with_sort("_id", false);
        search.constraint = after.clone().map(SearchConstraint::IdAfter);
        let result = context.search(state, "Patient", &search).await?;
        after = result.entries.last().map(|patient| patient.id.clone());
        let done = !result.has_more || result.entries.is_empty();
        patients.extend(result.entries);
        if done {
            return Ok(patients);
        }
    }
}

pub struct EvaluateMeasureOperation;

impl EvaluateMeasureOperation {
    pub fn new() -> Self {
        Self
    }

    /// The subjects and whether this is a report about one person.
    async fn subjects(
        &self,
        state: &AppState,
        context: &OperationContext,
        subject: Option<&str>,
    ) -> Result<(Vec<StoredResource>, bool), OperationError> {
        let Some(subject) = subject else {
            return Ok((all_patients(state, context).await?, false));
        };
        let reference = parse_reference(subject, Some(&state.fhir_base_url()))
            .map_err(|_| OperationError::InvalidParameters(format!("Invalid subject {subject}")))?;
        match reference.resource_type.as_str() {
            "Patient" => {
                let patient = context
                    .read(state, "Patient", &reference.id)
                    .await?
                    .ok_or_else(|| OperationError::NotFound(format!("{subject} not found")))?;
                Ok((vec![patient], true))
            }
            "Group" => {
                let members = super::group::resolve_membership(
                    state,
                    context,
                    &reference.id,
                    OffsetDateTime::now_utc(),
                )
                .await?
                .patient_ids;
                Ok((read_patients(state, context, &members).await?, false))
            }
            other => Err(OperationError::InvalidParameters(format!(
                "A subject is a Patient or a Group, not {other}"
            ))),
        }
    }

    async fn evaluate(
        &self,
        state: &AppState,
        context: &OperationContext,
        measure: &Value,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let service = state
            .cql_service
            .as_ref()
            .ok_or_else(|| OperationError::NotSupported("CQL service not enabled".into()))?;
        let service = scoped_service(state, context, service);
        let request = Request::parse(params)?;
        let scoring = Scoring::of(measure)?;
        let groups = measure["group"]
            .as_array()
            .filter(|groups| !groups.is_empty())
            .ok_or_else(|| OperationError::InvalidParameters("The Measure has no group".into()))?
            .iter()
            .map(Group::parse)
            .collect::<Result<Vec<_>, _>>()?;
        let library = measure["library"][0].as_str().ok_or_else(|| {
            OperationError::InvalidParameters("The Measure has no library".into())
        })?;
        let (url, version) = library
            .split_once('|')
            .map_or((library, None), |(url, version)| (url, Some(version)));
        let source = service
            .library_source(url, version)
            .await
            .map_err(map_cql_error)?;

        let (subjects, individual) = self
            .subjects(state, context, request.subject.as_deref())
            .await?;
        let references: Vec<String> = subjects
            .iter()
            .map(|patient| format!("Patient/{}", patient.id))
            .collect();
        let chunks: Vec<Vec<Value>> = subjects
            .chunks(SUBJECT_CHUNK)
            .map(|chunk| chunk.iter().map(|p| p.resource.clone()).collect())
            .collect();
        drop(subjects);
        let period = (request.period.0.as_str(), request.period.1.as_str());
        let evaluated: Vec<Vec<IndexMap<String, Value>>> = stream::iter(chunks)
            .map(|chunk| service.evaluate_subjects(&source, "Patient", chunk, period))
            .buffered(CHUNK_CONCURRENCY)
            .try_collect()
            .await
            .map_err(map_cql_error)?;

        let subject_list = request.report_type.as_deref() == Some("subject-list");
        let now = Utc::now().to_rfc3339();
        let mut contained = Vec::new();
        let mut report_groups = Vec::with_capacity(groups.len());
        for (index, group) in groups.iter().enumerate() {
            let mut tally = Tally::default();
            let mut strata: Vec<BTreeMap<String, Tally>> =
                vec![BTreeMap::new(); group.stratifiers.len()];
            for (defines, subject) in evaluated.iter().flatten().zip(&references) {
                let memberships = group.memberships(scoring, defines);
                tally.add(memberships, subject);
                for (stratifier, by_value) in group.stratifiers.iter().zip(&mut strata) {
                    by_value
                        .entry(stratum_key(defines.get(*stratifier)))
                        .or_default()
                        .add(memberships, subject);
                }
            }
            let used: Vec<&str> = group.criteria.keys().copied().collect();
            let prefix = format!("group-{index}");
            let list = subject_list.then_some(prefix.as_str());
            if subject_list {
                contained.extend(contained_lists(&prefix, &used, &tally, &now));
            }
            let mut result = json!({"population": tally.populations(&used, list)});
            if let Some(code) = group.source.get("code") {
                result["code"] = code.clone();
            }
            if let Some(score) = tally.score(scoring) {
                result["measureScore"] = json!({"value": score});
            }
            if !strata.is_empty() {
                let stratifiers: Vec<Value> = strata
                    .iter()
                    .map(|by_value| {
                        let stratum: Vec<Value> = by_value
                            .iter()
                            .map(|(value, tally)| {
                                let mut item = json!({
                                    "value": {"text": value},
                                    "population": tally.populations(&used, None),
                                });
                                if let Some(score) = tally.score(scoring) {
                                    item["measureScore"] = json!({"value": score});
                                }
                                item
                            })
                            .collect();
                        json!({"stratum": stratum})
                    })
                    .collect();
                result["stratifier"] = json!(stratifiers);
            }
            report_groups.push(result);
        }

        let report_type = match (individual, subject_list) {
            (true, _) => "individual",
            (false, true) => "subject-list",
            (false, false) => "summary",
        };
        let mut report = json!({
            "resourceType": "MeasureReport",
            "status": "complete",
            "type": report_type,
            "measure": measure["url"].as_str().unwrap_or_default(),
            "date": now,
            "period": {"start": request.period.0, "end": request.period.1},
            "group": report_groups,
        });
        if individual {
            report["subject"] = json!({"reference": references.first()});
        } else if let Some(subject) = &request.subject {
            report["subject"] = json!({"reference": subject});
        }
        if !contained.is_empty() {
            report["contained"] = json!(contained);
        }
        Ok(report)
    }
}

impl Default for EvaluateMeasureOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for EvaluateMeasureOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        matches!(level, OperationLevel::Type | OperationLevel::Instance)
            && resource_type == Some("Measure")
    }

    fn code(&self) -> &str {
        "evaluate-measure"
    }

    /// `Measure/$evaluate-measure?measure=`: the Measure by canonical URL.
    async fn handle_type(
        &self,
        state: &AppState,
        context: &OperationContext,
        _resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let url = text_parameter(params, "measure").ok_or_else(|| {
            OperationError::InvalidParameters("measure is required at this level".into())
        })?;
        let search = SearchParams::new().with_count(1).with_param("url", url);
        let measure = context
            .search(state, "Measure", &search)
            .await?
            .entries
            .into_iter()
            .next()
            .ok_or_else(|| OperationError::NotFound(format!("Measure {url} not found")))?;
        self.evaluate(state, context, &measure.resource, params)
            .await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        _resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let measure = context
            .read(state, "Measure", id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("Measure/{id} not found")))?;
        self.evaluate(state, context, &measure.resource, params)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(scoring: Scoring, subjects: &[&[&str]]) -> Tally {
        let mut tally = Tally::default();
        for (index, memberships) in subjects.iter().enumerate() {
            let classified = classify(scoring, |code| memberships.contains(&code));
            tally.add(classified, &format!("Patient/{index}"));
        }
        tally
    }

    #[test]
    fn proportion_follows_the_population_rules() {
        let result = tally(
            Scoring::Proportion,
            &[
                &["initial-population", "denominator", "numerator"],
                &["initial-population", "denominator"],
                // An exclusion leaves the denominator even when the numerator holds.
                &[
                    "initial-population",
                    "denominator",
                    "denominator-exclusion",
                    "numerator",
                ],
                // An exception only counts for those outside the numerator.
                &["initial-population", "denominator", "denominator-exception"],
                &[
                    "initial-population",
                    "denominator",
                    "denominator-exception",
                    "numerator",
                ],
                // Nothing counts outside the initial population.
                &["denominator", "numerator"],
            ],
        );
        assert_eq!(result.counts, [5, 5, 1, 1, 2, 0]);
        assert_eq!(result.score(Scoring::Proportion), Some(2.0 / 3.0));
    }

    #[test]
    fn a_ratio_numerator_stands_apart_from_its_denominator() {
        let result = tally(
            Scoring::Ratio,
            &[
                &["initial-population", "numerator"],
                &["initial-population", "denominator", "numerator"],
                &["initial-population", "denominator"],
            ],
        );
        assert_eq!(result.score(Scoring::Ratio), Some(1.0));
    }

    #[test]
    fn a_cohort_has_no_score_and_an_empty_denominator_no_value() {
        let cohort = tally(Scoring::Cohort, &[&["initial-population"]]);
        assert_eq!(cohort.counts[0], 1);
        assert_eq!(cohort.score(Scoring::Cohort), None);
        assert_eq!(Tally::default().score(Scoring::Proportion), None);
    }

    #[test]
    fn membership_comes_from_booleans_and_lists() {
        assert!(in_population(Some(&json!(true))));
        assert!(in_population(Some(&json!([{"id": 1}]))));
        assert!(!in_population(Some(&json!(false))));
        assert!(!in_population(Some(&json!([]))));
        assert!(!in_population(Some(&Value::Null)));
        assert!(!in_population(None));
    }

    #[test]
    fn dates_widen_to_the_whole_day() {
        assert_eq!(instant("2024-01-01", false), "2024-01-01T00:00:00.000Z");
        assert_eq!(instant("2024-12-31", true), "2024-12-31T23:59:59.999Z");
        assert_eq!(instant("2024", true), "2024-12-31T23:59:59.999Z");
        assert_eq!(
            instant("2024-05-05T10:00:00Z", true),
            "2024-05-05T10:00:00Z"
        );
    }

    #[test]
    fn continuous_variable_is_refused() {
        let measure = json!({"scoring":{"coding":[{"code":"continuous-variable"}]}});
        assert!(matches!(
            Scoring::of(&measure),
            Err(OperationError::NotSupported(_))
        ));
    }
}
