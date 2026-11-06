//! `Measure/$evaluate-measure`: population evaluation and scoring on CQL.
//!
//! The Measure's library is compiled once and evaluated for each subject. A population
//! criterion names a CQL define. In a subject-based measure `true`, or a non-empty list,
//! puts the subject in the population. With a `populationBasis` extension naming a resource
//! type the members are the resources the defines return, such as Encounters, and each is
//! counted. Proportion, ratio, cohort and continuous variable scoring are implemented. A
//! `measure-observation` names a CQL function: each member of the measure population goes
//! through it and the group's `aggregateMethod` turns the values into the score.

use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{StreamExt, TryStreamExt, stream};
use indexmap::IndexMap;
use octofhir_core::fhir_reference::parse_reference;
use octofhir_cql_service::CqlService;
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
const POPULATIONS: [&str; 9] = [
    "initial-population",
    "denominator",
    "denominator-exclusion",
    "denominator-exception",
    "numerator",
    "numerator-exclusion",
    "measure-population",
    "measure-population-exclusion",
    "measure-observation",
];

const OBSERVATION: &str = "measure-observation";

/// Whether a member is in each of [`POPULATIONS`].
type Memberships = [bool; POPULATIONS.len()];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scoring {
    Proportion,
    Ratio,
    Cohort,
    ContinuousVariable,
}

impl Scoring {
    /// From the group (R5 and later) or the Measure (R4, R5).
    fn of(measure: &Value, group: &Value) -> Result<Self, OperationError> {
        let code = [group, measure].into_iter().find_map(|source| {
            source["scoring"]["coding"]
                .as_array()?
                .iter()
                .find_map(|coding| coding["code"].as_str())
        });
        match code {
            Some("proportion") => Ok(Self::Proportion),
            Some("ratio") => Ok(Self::Ratio),
            Some("cohort") => Ok(Self::Cohort),
            Some("continuous-variable") => Ok(Self::ContinuousVariable),
            Some(other) => Err(OperationError::NotSupported(format!(
                "Measure scoring {other} is not supported"
            ))),
            None => Err(OperationError::InvalidParameters(
                "The Measure has no scoring".into(),
            )),
        }
    }
}

/// How observation values become the score, from `aggregateMethod` (R5 and later) or the
/// extension of that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Aggregate {
    Sum,
    Average,
    Median,
    Minimum,
    Maximum,
    Count,
}

impl Aggregate {
    fn of(population: &Value) -> Result<Self, OperationError> {
        let code = population["aggregateMethod"]["coding"][0]["code"]
            .as_str()
            .or_else(|| {
                population["extension"]
                    .as_array()?
                    .iter()
                    .find(|extension| {
                        extension["url"]
                            .as_str()
                            .is_some_and(|url| url.ends_with("aggregateMethod"))
                    })?["valueCode"]
                    .as_str()
            });
        match code {
            Some("sum") => Ok(Self::Sum),
            Some("average") => Ok(Self::Average),
            Some("median") => Ok(Self::Median),
            Some("minimum") => Ok(Self::Minimum),
            Some("maximum") => Ok(Self::Maximum),
            Some("count") => Ok(Self::Count),
            Some(other) => Err(OperationError::NotSupported(format!(
                "Aggregate method {other} is not supported"
            ))),
            None => Err(OperationError::InvalidParameters(
                "The measure-observation has no aggregateMethod".into(),
            )),
        }
    }

    /// The score of `values`; nothing to aggregate gives no score, except for a count.
    fn apply(self, values: &[f64]) -> Option<f64> {
        if self == Self::Count {
            return Some(values.len() as f64);
        }
        let first = *values.first()?;
        let sum = || values.iter().sum::<f64>();
        Some(match self {
            Self::Sum => sum(),
            Self::Average => sum() / values.len() as f64,
            Self::Minimum => values.iter().copied().fold(first, f64::min),
            Self::Maximum => values.iter().copied().fold(first, f64::max),
            Self::Median => {
                let mut sorted = values.to_vec();
                sorted.sort_unstable_by(f64::total_cmp);
                let middle = sorted.len() / 2;
                if sorted.len() % 2 == 1 {
                    sorted[middle]
                } else {
                    (sorted[middle - 1] + sorted[middle]) / 2.0
                }
            }
            Self::Count => unreachable!("counted above"),
        })
    }
}

/// A number, or the value of a Quantity.
fn observed_number(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value["value"].as_f64())
}

/// `true`, or a list with something in it.
fn in_population(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Array(items)) => !items.is_empty(),
        _ => false,
    }
}

/// Whether the populations hold resources rather than subjects: `basis` (R5 and later) or
/// the `populationBasis` extension names a type other than `boolean`.
fn resource_based(measure: &Value, group: &Value) -> bool {
    let declared = |source: &'_ Value| -> Option<String> {
        let extension = || {
            source["extension"].as_array()?.iter().find(|extension| {
                extension["url"]
                    .as_str()
                    .is_some_and(|url| url.ends_with("populationBasis"))
            })
        };
        source["basis"]
            .as_str()
            .or_else(|| extension()?["valueCode"].as_str())
            .map(str::to_owned)
    };
    declared(group)
        .or_else(|| declared(measure))
        .is_some_and(|basis| basis != "boolean")
}

/// The ids of the resources a define returned; a population holds one resource type.
fn members(value: Option<&Value>) -> HashSet<&str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str())
        .collect()
}

/// Where one subject falls in a group, per the population rules of the scoring.
fn classify(scoring: Scoring, criteria: impl Fn(&str) -> bool) -> Memberships {
    let initial = criteria("initial-population");
    let mut memberships = [false; POPULATIONS.len()];
    memberships[0] = initial;
    if scoring == Scoring::Cohort {
        return memberships;
    }
    if scoring == Scoring::ContinuousVariable {
        let population = initial && criteria("measure-population");
        let exclusion = population && criteria("measure-population-exclusion");
        memberships[6] = population;
        memberships[7] = exclusion;
        memberships[8] = population && !exclusion && criteria(OBSERVATION);
        return memberships;
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
    memberships[1] = denominator;
    memberships[2] = exclusion;
    memberships[3] = exception;
    memberships[4] = numerator;
    memberships[5] = numerator_exclusion;
    memberships
}

/// Where a member falls, and the value observed for it.
type Classified = (Memberships, Option<f64>);

/// Population counts of a group or stratum.
#[derive(Debug, Clone, Default)]
struct Tally {
    counts: [u64; POPULATIONS.len()],
    subjects: [Vec<String>; POPULATIONS.len()],
    observations: Vec<f64>,
}

impl Tally {
    fn add(&mut self, (memberships, observation): Classified, subject: &str) {
        for (index, member) in memberships.into_iter().enumerate() {
            if member {
                self.counts[index] += 1;
                self.subjects[index].push(subject.to_owned());
            }
        }
        if memberships[8] {
            self.observations.extend(observation);
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
            ..,
        ] = self.counts;
        let top = numerator.checked_sub(numerator_exclusion)?;
        let bottom = match scoring {
            Scoring::Cohort | Scoring::ContinuousVariable => return None,
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
    /// How the observations of a continuous variable group are scored.
    aggregate: Option<Aggregate>,
    /// The define added to the library to run the observation function.
    observation: Option<String>,
}

impl<'m> Group<'m> {
    fn parse(index: usize, source: &'m Value) -> Result<Self, OperationError> {
        let mut criteria = BTreeMap::new();
        let mut aggregate = None;
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
            if code == OBSERVATION {
                aggregate = Some(Aggregate::of(population)?);
            }
        }
        let stratifiers = source["stratifier"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|stratifier| stratifier["criteria"]["expression"].as_str())
            .collect();
        let observation = aggregate.map(|_| format!("OctoFHIR Observation {index}"));
        Ok(Self {
            source,
            criteria,
            stratifiers,
            aggregate,
            observation,
        })
    }

    fn observed<'d>(&self, defines: &'d IndexMap<String, Value>) -> Option<&'d Value> {
        defines.get(self.observation.as_deref()?)
    }

    /// The CQL that runs the observation function: over each member of the measure
    /// population, or once for a subject-based measure.
    fn observation_define(&self, by_resource: bool) -> Result<Option<String>, OperationError> {
        let Some(name) = &self.observation else {
            return Ok(None);
        };
        let function = self.criteria[OBSERVATION];
        let population = self.criteria.get("measure-population").ok_or_else(|| {
            OperationError::InvalidParameters(
                "A measure-observation needs a measure-population".into(),
            )
        })?;
        if function.contains('"') || population.contains('"') {
            return Err(OperationError::InvalidParameters(
                "A population names a define or function, not an expression".into(),
            ));
        }
        Ok(Some(if by_resource {
            format!(
                "define \"{name}\": \"{population}\" M return Tuple {{ id: M.id, value: \"{function}\"(M) }}\n"
            )
        } else {
            format!("define \"{name}\": \"{function}\"()\n")
        }))
    }

    fn in_define(&self, code: &str, defines: &IndexMap<String, Value>) -> bool {
        self.criteria
            .get(code)
            .is_some_and(|name| in_population(defines.get(*name)))
    }

    /// The subject as the one member of its populations.
    fn subject_member(&self, scoring: Scoring, defines: &IndexMap<String, Value>) -> Classified {
        let observation = self.observed(defines).and_then(observed_number);
        let memberships = classify(scoring, |code| match code {
            OBSERVATION => observation.is_some(),
            _ => self.in_define(code, defines),
        });
        (memberships, observation)
    }

    /// One classification per resource found in any population of a subject.
    fn resource_members(
        &self,
        scoring: Scoring,
        defines: &IndexMap<String, Value>,
    ) -> Vec<Classified> {
        let populations: BTreeMap<&str, HashSet<&str>> = self
            .criteria
            .iter()
            .filter(|(code, _)| **code != OBSERVATION)
            .map(|(code, name)| (*code, members(defines.get(*name))))
            .collect();
        let observed: HashMap<&str, f64> = self
            .observed(defines)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| Some((item["id"].as_str()?, observed_number(&item["value"])?)))
            .collect();
        let all: HashSet<&str> = populations.values().flatten().copied().collect();
        all.into_iter()
            .map(|resource| {
                let observation = observed.get(resource).copied();
                let memberships = classify(scoring, |code| match code {
                    OBSERVATION => observation.is_some(),
                    _ => populations
                        .get(code)
                        .is_some_and(|population| population.contains(resource)),
                });
                (memberships, observation)
            })
            .collect()
    }

    fn score(&self, tally: &Tally, scoring: Scoring) -> Option<f64> {
        match self.aggregate {
            Some(aggregate) if scoring == Scoring::ContinuousVariable => {
                aggregate.apply(&tally.observations)
            }
            _ => tally.score(scoring),
        }
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

fn parameter<'a>(params: &'a Value, name: &str) -> Option<&'a Value> {
    params["parameter"]
        .as_array()?
        .iter()
        .find(|parameter| parameter["name"] == name)
}

pub(super) fn text_parameter<'a>(params: &'a Value, name: &str) -> Option<&'a str> {
    parameter(params, name)?
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

/// The canonical of a Measure's library and its CQL text.
pub(super) async fn measure_library<'m>(
    service: &CqlService,
    measure: &'m Value,
) -> Result<(&'m str, String), OperationError> {
    let library = measure["library"][0]
        .as_str()
        .or_else(|| measure["group"][0]["library"][0].as_str())
        .ok_or_else(|| OperationError::InvalidParameters("The Measure has no library".into()))?;
    let (url, version) = library
        .split_once('|')
        .map_or((library, None), |(url, version)| (url, Some(version)));
    let source = service
        .library_source(url, version)
        .await
        .map_err(map_cql_error)?;
    Ok((library, source))
}

async fn measure_by_url(
    state: &AppState,
    context: &OperationContext,
    url: &str,
) -> Result<Value, OperationError> {
    let search = SearchParams::new().with_count(1).with_param("url", url);
    context
        .search(state, "Measure", &search)
        .await?
        .entries
        .into_iter()
        .next()
        .map(|measure| measure.resource)
        .ok_or_else(|| OperationError::NotFound(format!("Measure {url} not found")))
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
        request: &Request,
    ) -> Result<Value, OperationError> {
        let service = state
            .cql_service
            .as_ref()
            .ok_or_else(|| OperationError::NotSupported("CQL service not enabled".into()))?;
        let service = scoped_service(state, context, service);
        let groups = measure["group"]
            .as_array()
            .filter(|groups| !groups.is_empty())
            .ok_or_else(|| OperationError::InvalidParameters("The Measure has no group".into()))?
            .iter()
            .enumerate()
            .map(|(index, group)| {
                Ok((
                    Group::parse(index, group)?,
                    Scoring::of(measure, group)?,
                    resource_based(measure, group),
                ))
            })
            .collect::<Result<Vec<_>, OperationError>>()?;
        let (_, mut source) = measure_library(&service, measure).await?;
        for (group, _, by_resource) in &groups {
            if let Some(define) = group.observation_define(*by_resource)? {
                source.push('\n');
                source.push_str(&define);
            }
        }

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
        // R5 turned these two into choice elements.
        let (score_element, value_element) = match state.fhir_version.as_str() {
            "R4" | "R4B" => ("measureScore", "value"),
            _ => ("measureScoreQuantity", "valueCodeableConcept"),
        };
        for (index, (group, scoring, by_resource)) in groups.iter().enumerate() {
            let (scoring, by_resource) = (*scoring, *by_resource);
            let mut tally = Tally::default();
            let mut strata: Vec<BTreeMap<String, Tally>> =
                vec![BTreeMap::new(); group.stratifiers.len()];
            for (defines, subject) in evaluated.iter().flatten().zip(&references) {
                let classified = if by_resource {
                    group.resource_members(scoring, defines)
                } else {
                    vec![group.subject_member(scoring, defines)]
                };
                for member in classified {
                    tally.add(member, subject);
                    for (stratifier, by_value) in group.stratifiers.iter().zip(&mut strata) {
                        by_value
                            .entry(stratum_key(defines.get(*stratifier)))
                            .or_default()
                            .add(member, subject);
                    }
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
            if let Some(score) = group.score(&tally, scoring) {
                result[score_element] = json!({"value": score});
            }
            if !strata.is_empty() {
                let stratifiers: Vec<Value> = strata
                    .iter()
                    .map(|by_value| {
                        let stratum: Vec<Value> = by_value
                            .iter()
                            .map(|(value, tally)| {
                                let mut item = json!({
                                    value_element: {"text": value},
                                    "population": tally.populations(&used, None),
                                });
                                if let Some(score) = group.score(tally, scoring) {
                                    item[score_element] = json!({"value": score});
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
        let measure = measure_by_url(state, context, url).await?;
        self.evaluate(state, context, &measure, &Request::parse(params)?)
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
        self.evaluate(state, context, &measure.resource, &Request::parse(params)?)
            .await
    }
}

/// `Measure/$evaluate` (R6): any number of measures, the reports as Bundles organized by
/// subject in the `return` parameters.
pub struct EvaluateOperation;

impl EvaluateOperation {
    /// The report type asked for, or the default for this subject. `subject` and
    /// `population` are the older names the definition keeps for compatibility.
    fn report_type<'p>(params: &'p Value, group: bool) -> Result<&'p str, OperationError> {
        let subject = text_parameter(params, "subject");
        match text_parameter(params, "reportType") {
            Some("individual" | "subject") => Ok("individual"),
            Some("summary" | "population") => Ok("summary"),
            Some("subject-list") => Ok("subject-list"),
            Some(other) => Err(OperationError::InvalidParameters(format!(
                "Unknown reportType {other}"
            ))),
            None if subject.is_some() && !group => Ok("individual"),
            None => Ok("summary"),
        }
    }

    async fn bundle(
        state: &AppState,
        context: &OperationContext,
        measures: &[Value],
        request: &Request,
    ) -> Result<Value, OperationError> {
        let mut entry = Vec::with_capacity(measures.len());
        for measure in measures {
            let report = EvaluateMeasureOperation
                .evaluate(state, context, measure, request)
                .await?;
            entry.push(json!({"resource": report}));
        }
        Ok(json!({"name": "return", "resource": {
            "resourceType": "Bundle",
            "type": "collection",
            "entry": entry,
        }}))
    }
}

#[async_trait]
impl OperationHandler for EvaluateOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        level == OperationLevel::Type && resource_type == Some("Measure")
    }

    fn code(&self) -> &str {
        "evaluate"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        context: &OperationContext,
        _resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        for name in ["subjectGroup", "reporterResource", "manifest"] {
            if parameter(params, name).is_some() {
                return Err(OperationError::NotSupported(format!(
                    "The {name} parameter is not supported"
                )));
            }
        }
        let urls: Vec<&str> = params["parameter"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|parameter| parameter["name"] == "measureUrl")
            .filter_map(|parameter| {
                parameter["valueCanonical"]
                    .as_str()
                    .or_else(|| parameter["valueString"].as_str())
            })
            .collect();
        if urls.is_empty() {
            return Err(OperationError::InvalidParameters(
                "measureUrl is required".into(),
            ));
        }
        let mut measures = Vec::with_capacity(urls.len());
        for url in urls {
            measures.push(measure_by_url(state, context, url).await?);
        }

        let mut request = Request::parse(params)?;
        let group = request
            .subject
            .as_deref()
            .is_some_and(|subject| subject.contains("Group/"));
        let report_type = Self::report_type(params, group)?;
        request.report_type = Some(report_type.to_owned());

        let mut bundles = Vec::new();
        if report_type == "individual" {
            let (subjects, _) = EvaluateMeasureOperation
                .subjects(state, context, request.subject.as_deref())
                .await?;
            for subject in subjects {
                request.subject = Some(format!("Patient/{}", subject.id));
                bundles.push(Self::bundle(state, context, &measures, &request).await?);
            }
        } else {
            bundles.push(Self::bundle(state, context, &measures, &request).await?);
        }
        Ok(json!({"resourceType": "Parameters", "parameter": bundles}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(scoring: Scoring, subjects: &[&[&str]]) -> Tally {
        let mut tally = Tally::default();
        for (index, memberships) in subjects.iter().enumerate() {
            let classified = classify(scoring, |code| memberships.contains(&code));
            tally.add((classified, None), &format!("Patient/{index}"));
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
        assert_eq!(result.counts[..6], [5, 5, 1, 1, 2, 0]);
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
    fn the_group_scoring_comes_before_the_measure_one() {
        let scoring = |code: &str| json!({"scoring": {"coding": [{"code": code}]}});
        assert_eq!(
            Scoring::of(&scoring("cohort"), &scoring("ratio")).unwrap(),
            Scoring::Ratio
        );
        assert_eq!(
            Scoring::of(&scoring("cohort"), &json!({})).unwrap(),
            Scoring::Cohort
        );
    }

    #[test]
    fn the_aggregate_method_is_an_element_from_r5() {
        let population = json!({"aggregateMethod": {"coding": [{"code": "median"}]}});
        assert_eq!(Aggregate::of(&population).unwrap(), Aggregate::Median);
    }

    #[test]
    fn the_report_type_defaults_by_subject() {
        let params = |parameters: Value| json!({"parameter": parameters});
        let of = |parameters: Value, group| {
            EvaluateOperation::report_type(&params(parameters), group)
                .unwrap()
                .to_owned()
        };
        assert_eq!(of(json!([]), false), "summary");
        assert_eq!(
            of(
                json!([{"name": "subject", "valueString": "Patient/1"}]),
                false
            ),
            "individual"
        );
        assert_eq!(
            of(json!([{"name": "subject", "valueString": "Group/1"}]), true),
            "summary"
        );
        assert_eq!(
            of(
                json!([{"name": "reportType", "valueCode": "population"}]),
                false
            ),
            "summary"
        );
        assert!(
            EvaluateOperation::report_type(
                &params(json!([{"name": "reportType", "valueCode": "x"}])),
                false
            )
            .is_err()
        );
    }

    #[test]
    fn resource_based_populations_count_each_resource() {
        let measure = json!({"extension":[{
            "url":"http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-populationBasis",
            "valueCode":"Encounter"}]});
        assert!(resource_based(&measure, &json!({})));
        assert!(resource_based(&json!({}), &json!({"basis": "Encounter"})));
        assert!(!resource_based(&measure, &json!({"basis": "boolean"})));
        let boolean = json!({"extension":[{"url":"x/populationBasis","valueCode":"boolean"}]});
        assert!(!resource_based(&boolean, &json!({})));
        assert!(!resource_based(&json!({}), &json!({})));

        let source = json!({"population":[
            {"code":{"coding":[{"code":"initial-population"}]},"criteria":{"expression":"IP"}},
            {"code":{"coding":[{"code":"denominator"}]},"criteria":{"expression":"Den"}},
            {"code":{"coding":[{"code":"numerator"}]},"criteria":{"expression":"Num"}}]});
        let group = Group::parse(0, &source).unwrap();
        let encounter = |id: &str| json!({"resourceType":"Encounter","id":id});
        let defines: IndexMap<String, Value> = IndexMap::from([
            (
                "IP".to_owned(),
                json!([encounter("a"), encounter("b"), encounter("c")]),
            ),
            ("Den".to_owned(), json!([encounter("a"), encounter("b")])),
            ("Num".to_owned(), json!([encounter("a"), encounter("z")])),
        ]);
        let mut tally = Tally::default();
        for member in group.resource_members(Scoring::Proportion, &defines) {
            tally.add(member, "Patient/1");
        }
        assert_eq!(tally.counts[..6], [3, 2, 0, 0, 1, 0]);
        assert_eq!(tally.score(Scoring::Proportion), Some(0.5));
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

    fn continuous_group() -> Value {
        json!({"population":[
            {"code":{"coding":[{"code":"initial-population"}]},"criteria":{"expression":"IP"}},
            {"code":{"coding":[{"code":"measure-population"}]},"criteria":{"expression":"MP"}},
            {"code":{"coding":[{"code":"measure-population-exclusion"}]},
                "criteria":{"expression":"MPE"}},
            {"extension":[{
                "url":"http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-aggregateMethod",
                "valueCode":"median"}],
                "code":{"coding":[{"code":"measure-observation"}]},
                "criteria":{"expression":"Stay"}}]})
    }

    #[test]
    fn continuous_variable_observes_the_measure_population_without_its_exclusions() {
        let source = continuous_group();
        let group = Group::parse(0, &source).unwrap();
        assert_eq!(group.aggregate, Some(Aggregate::Median));
        assert_eq!(
            group.observation_define(true).unwrap().unwrap(),
            "define \"OctoFHIR Observation 0\": \"MP\" M return \
Tuple { id: M.id, value: \"Stay\"(M) }\n"
        );
        assert_eq!(
            group.observation_define(false).unwrap().unwrap(),
            "define \"OctoFHIR Observation 0\": \"Stay\"()\n"
        );

        let resource = |id: &str| json!({"resourceType":"Encounter","id":id});
        let observed = |id: &str, value: Value| json!({"id":id,"value":value});
        let defines: IndexMap<String, Value> = IndexMap::from([
            (
                "IP".to_owned(),
                json!([resource("a"), resource("b"), resource("c"), resource("d")]),
            ),
            (
                "MP".to_owned(),
                json!([resource("a"), resource("b"), resource("c")]),
            ),
            ("MPE".to_owned(), json!([resource("c")])),
            (
                "OctoFHIR Observation 0".to_owned(),
                json!([
                    observed("a", json!(10)),
                    observed("b", json!({"value":20.0,"unit":"min"})),
                    observed("c", json!(1000))
                ]),
            ),
        ]);
        let mut tally = Tally::default();
        for member in group.resource_members(Scoring::ContinuousVariable, &defines) {
            tally.add(member, "Patient/1");
        }
        assert_eq!(tally.counts, [4, 0, 0, 0, 0, 0, 3, 1, 2]);
        assert_eq!(group.score(&tally, Scoring::ContinuousVariable), Some(15.0));
    }

    #[test]
    fn a_subject_based_observation_is_one_value_per_subject() {
        let source = continuous_group();
        let group = Group::parse(0, &source).unwrap();
        let defines: IndexMap<String, Value> = IndexMap::from([
            ("IP".to_owned(), json!(true)),
            ("MP".to_owned(), json!(true)),
            ("MPE".to_owned(), json!(false)),
            ("OctoFHIR Observation 0".to_owned(), json!(7)),
        ]);
        let (memberships, observation) =
            group.subject_member(Scoring::ContinuousVariable, &defines);
        assert_eq!(observation, Some(7.0));
        assert!(memberships[8]);
    }

    #[test]
    fn aggregates_follow_their_definitions() {
        let values = [4.0, 1.0, 3.0, 2.0];
        assert_eq!(Aggregate::Sum.apply(&values), Some(10.0));
        assert_eq!(Aggregate::Average.apply(&values), Some(2.5));
        assert_eq!(Aggregate::Median.apply(&values), Some(2.5));
        assert_eq!(Aggregate::Median.apply(&values[..3]), Some(3.0));
        assert_eq!(Aggregate::Minimum.apply(&values), Some(1.0));
        assert_eq!(Aggregate::Maximum.apply(&values), Some(4.0));
        assert_eq!(Aggregate::Count.apply(&values), Some(4.0));
        assert_eq!(Aggregate::Sum.apply(&[]), None);
        assert_eq!(Aggregate::Count.apply(&[]), Some(0.0));
    }

    #[test]
    fn an_observation_needs_a_method_and_a_plain_name() {
        let method_missing = json!({"population":[
            {"code":{"coding":[{"code":"measure-observation"}]},"criteria":{"expression":"F"}}]});
        assert!(matches!(
            Group::parse(0, &method_missing),
            Err(OperationError::InvalidParameters(_))
        ));
        let mut source = continuous_group();
        source["population"][3]["criteria"]["expression"] = json!("Stay\" x");
        let group = Group::parse(0, &source).unwrap();
        assert!(group.observation_define(true).is_err());
    }
}
