//! `Measure/$evaluate-measure`: population evaluation and scoring on CQL.
//!
//! The Measure's library is compiled once and evaluated for each subject. A population
//! criterion names a CQL define. In a subject-based measure `true`, or a non-empty list,
//! puts the subject in the population. With a `populationBasis` extension naming a resource
//! type the members are the resources the defines return, such as Encounters, and each is
//! counted. Proportion, ratio, cohort and continuous variable scoring are implemented. A
//! `measure-observation` names a CQL function: each member of the population it refers to
//! goes through it and the group's `aggregateMethod` turns the values into the score. A ratio
//! can observe its numerator and denominator, and then divides the two aggregates.
//! [`composite`] combines other measures and [`supplemental`] reports `supplementalData`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{StreamExt, TryStreamExt, stream};
use indexmap::IndexMap;
use octofhir_core::fhir_reference::parse_reference;
use octofhir_cql_service::CqlService;
use octofhir_storage::{SearchConstraint, SearchParams, StoredResource};
use serde_json::{Value, json};
use time::OffsetDateTime;

mod composite;
mod supplemental;

use self::supplemental::Supplemental;
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

/// The population an observation is taken over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationTarget {
    MeasurePopulation,
    Numerator,
    Denominator,
}

impl ObservationTarget {
    fn population(self) -> &'static str {
        match self {
            Self::MeasurePopulation => "measure-population",
            Self::Numerator => "numerator",
            Self::Denominator => "denominator",
        }
    }

    /// Whether a member counts here: exclusions leave before anything is observed.
    fn holds(self, memberships: &Memberships) -> bool {
        match self {
            Self::MeasurePopulation => memberships[8],
            Self::Numerator => memberships[4] && !memberships[5],
            Self::Denominator => memberships[1] && !memberships[2],
        }
    }
}

/// A group has one observation, or a ratio's two.
const MAX_OBSERVATIONS: usize = 2;

/// Where a member falls, and the values observed for it, one per observation of the group.
type Classified = (Memberships, [Option<f64>; MAX_OBSERVATIONS]);

/// Population counts of a group or stratum.
#[derive(Debug, Clone, Default)]
struct Tally {
    counts: [u64; POPULATIONS.len()],
    subjects: [Vec<String>; POPULATIONS.len()],
    observations: [Vec<f64>; MAX_OBSERVATIONS],
}

impl Tally {
    fn count(&mut self, memberships: Memberships, subject: &str) {
        for (index, member) in memberships.into_iter().enumerate() {
            if member {
                self.counts[index] += 1;
                self.subjects[index].push(subject.to_owned());
            }
        }
    }

    fn add(&mut self, (memberships, observed): Classified, subject: &str, group: &Group<'_>) {
        self.count(memberships, subject);
        for (slot, observation) in group.observations.iter().enumerate() {
            if observation.target.holds(&memberships) {
                self.observations[slot].extend(observed[slot]);
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

    /// The populations of a report; each observation of a group is a population of its own.
    fn populations(&self, used: &[&str], list_id: Option<&str>, observations: usize) -> Vec<Value> {
        let row = |code: &str, count: u64| {
            let mut population = json!({
                "code": {"coding": [{"system": MEASURE_POPULATION, "code": code}]},
                "count": count,
            });
            if let Some(prefix) = list_id {
                population["subjectResults"] = json!({"reference": format!("#{prefix}-{code}")});
            }
            population
        };
        let mut rows = Vec::with_capacity(used.len() + 1);
        for (index, code) in POPULATIONS.iter().enumerate() {
            if !used.contains(code) {
                continue;
            }
            if *code == OBSERVATION {
                rows.extend(
                    self.observations[..observations]
                        .iter()
                        .map(|values| row(code, values.len() as u64)),
                );
            } else {
                rows.push(row(code, self.counts[index]));
            }
        }
        rows
    }
}

/// One `Measure.group` as the evaluator uses it.
struct Group<'m> {
    source: &'m Value,
    /// Population code to the name of its define.
    criteria: BTreeMap<&'m str, &'m str>,
    stratifiers: Vec<&'m str>,
    observations: Vec<Observation<'m>>,
}

/// A `measure-observation` population.
#[derive(Debug, Clone)]
struct Observation<'m> {
    target: ObservationTarget,
    function: &'m str,
    aggregate: Aggregate,
    /// The define added to the library to run the function.
    define: String,
}

impl<'m> Observation<'m> {
    fn parse(
        group: usize,
        population: &'m Value,
        function: &'m str,
        populations: &'m [Value],
    ) -> Result<Self, OperationError> {
        let target = match criteria_reference(population) {
            None => ObservationTarget::MeasurePopulation,
            Some(reference) => {
                let code = populations
                    .iter()
                    .find(|other| other["id"] == reference || other["linkId"] == reference)
                    .and_then(|other| other["code"]["coding"][0]["code"].as_str());
                match code {
                    Some("measure-population") => ObservationTarget::MeasurePopulation,
                    Some("numerator") => ObservationTarget::Numerator,
                    Some("denominator") => ObservationTarget::Denominator,
                    _ => {
                        return Err(OperationError::InvalidParameters(format!(
                            "A measure-observation refers to {reference}, which is no \
                             measure population, numerator or denominator of its group"
                        )));
                    }
                }
            }
        };
        let define = match target {
            ObservationTarget::MeasurePopulation => format!("OctoFHIR Observation {group}"),
            ObservationTarget::Numerator => format!("OctoFHIR Observation {group} Numerator"),
            ObservationTarget::Denominator => format!("OctoFHIR Observation {group} Denominator"),
        };
        Ok(Self {
            target,
            function,
            aggregate: Aggregate::of(population)?,
            define,
        })
    }
}

/// The population an observation is over: `inputPopulationId` (R5 and later) or the
/// `criteriaReference` extension.
fn criteria_reference(population: &Value) -> Option<&str> {
    population["inputPopulationId"].as_str().or_else(|| {
        population["extension"]
            .as_array()?
            .iter()
            .find(|extension| {
                extension["url"]
                    .as_str()
                    .is_some_and(|url| url.ends_with("criteriaReference"))
            })?["valueString"]
            .as_str()
    })
}

impl<'m> Group<'m> {
    fn parse(index: usize, source: &'m Value) -> Result<Self, OperationError> {
        let mut criteria = BTreeMap::new();
        let mut observations = Vec::new();
        let populations = source["population"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        for population in populations {
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
                observations.push(Observation::parse(
                    index,
                    population,
                    expression,
                    populations,
                )?);
            }
        }
        let stratifiers = source["stratifier"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|stratifier| stratifier["criteria"]["expression"].as_str())
            .collect();
        if observations.len() > MAX_OBSERVATIONS {
            return Err(OperationError::NotSupported(
                "A group has one measure-observation, or two on a ratio".into(),
            ));
        }
        Ok(Self {
            source,
            criteria,
            stratifiers,
            observations,
        })
    }

    /// A ratio observes its numerator and denominator both, or neither.
    fn check_observations(&self, scoring: Scoring) -> Result<(), OperationError> {
        let observes = |target| self.observations.iter().any(|o| o.target == target);
        let ratio_pair =
            observes(ObservationTarget::Numerator) && observes(ObservationTarget::Denominator);
        if scoring == Scoring::Ratio && !self.observations.is_empty() && !ratio_pair {
            return Err(OperationError::InvalidParameters(
                "A ratio observes its numerator and its denominator".into(),
            ));
        }
        Ok(())
    }

    /// The CQL that runs each observation function: over each member of its population, or
    /// once for a subject-based measure.
    fn observation_defines(&self, by_resource: bool) -> Result<String, OperationError> {
        let mut source = String::new();
        for observation in &self.observations {
            let population = observation.target.population();
            let population = self.criteria.get(population).ok_or_else(|| {
                OperationError::InvalidParameters(format!(
                    "A measure-observation needs a {population}"
                ))
            })?;
            let (name, function) = (&observation.define, observation.function);
            if function.contains('"') || population.contains('"') {
                return Err(OperationError::InvalidParameters(
                    "A population names a define or function, not an expression".into(),
                ));
            }
            source.push_str(&if by_resource {
                format!(
                    "define \"{name}\": \"{population}\" M return Tuple {{ id: M.id, value: \"{function}\"(M) }}\n"
                )
            } else {
                format!("define \"{name}\": \"{function}\"()\n")
            });
        }
        Ok(source)
    }

    fn in_define(&self, code: &str, defines: &IndexMap<String, Value>) -> bool {
        self.criteria
            .get(code)
            .is_some_and(|name| in_population(defines.get(*name)))
    }

    /// The subject as the one member of its populations.
    fn subject_member(&self, scoring: Scoring, defines: &IndexMap<String, Value>) -> Classified {
        let mut observed = [None; MAX_OBSERVATIONS];
        for (slot, observation) in self.observations.iter().enumerate() {
            observed[slot] = defines
                .get(observation.define.as_str())
                .and_then(observed_number);
        }
        let memberships = classify(scoring, |code| match code {
            OBSERVATION => observed[0].is_some(),
            _ => self.in_define(code, defines),
        });
        (memberships, observed)
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
        let observed: Vec<HashMap<&str, f64>> = self
            .observations
            .iter()
            .map(|observation| {
                defines
                    .get(observation.define.as_str())
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| {
                        Some((item["id"].as_str()?, observed_number(&item["value"])?))
                    })
                    .collect()
            })
            .collect();
        let all: HashSet<&str> = populations.values().flatten().copied().collect();
        all.into_iter()
            .map(|resource| {
                let mut values = [None; MAX_OBSERVATIONS];
                for (slot, by_resource) in observed.iter().enumerate() {
                    values[slot] = by_resource.get(resource).copied();
                }
                let memberships = classify(scoring, |code| match code {
                    OBSERVATION => values[0].is_some(),
                    _ => populations
                        .get(code)
                        .is_some_and(|population| population.contains(resource)),
                });
                (memberships, values)
            })
            .collect()
    }

    fn score(&self, tally: &Tally, scoring: Scoring) -> Option<f64> {
        let aggregate = |target| {
            let slot = self.observations.iter().position(|o| o.target == target)?;
            self.observations[slot]
                .aggregate
                .apply(&tally.observations[slot])
        };
        match scoring {
            Scoring::ContinuousVariable => aggregate(ObservationTarget::MeasurePopulation),
            Scoring::Ratio if !self.observations.is_empty() => {
                let numerator = aggregate(ObservationTarget::Numerator)?;
                let denominator = aggregate(ObservationTarget::Denominator)?;
                (denominator != 0.0).then(|| numerator / denominator)
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

/// References to what the populations of a subject hold, for `evaluatedResource`.
fn evaluated_resources(
    groups: &[(Group<'_>, Scoring, bool)],
    defines: Option<&IndexMap<String, Value>>,
) -> Vec<Value> {
    let Some(defines) = defines else {
        return Vec::new();
    };
    let found: BTreeSet<String> = groups
        .iter()
        .flat_map(|(group, ..)| {
            group
                .criteria
                .iter()
                .filter(|(code, _)| **code != OBSERVATION)
        })
        .filter_map(|(_, name)| defines.get(*name)?.as_array())
        .flatten()
        .filter_map(|item| {
            Some(format!(
                "{}/{}",
                item["resourceType"].as_str()?,
                item["id"].as_str()?
            ))
        })
        .collect();
    found
        .into_iter()
        .map(|reference| json!({"reference": reference}))
        .collect()
}

/// What a request asks for.
struct Request {
    period: (String, String),
    report_type: Option<String>,
    subject: Option<String>,
    /// The patients of a `subjectGroup`, in place of `subject`.
    members: Option<Vec<String>>,
    reporter: Option<String>,
    location: Option<String>,
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
            members: None,
            reporter: text_parameter(params, "reporter").map(str::to_owned),
            location: text_parameter(params, "location").map(str::to_owned),
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
    let (canonical, version) = url
        .split_once('|')
        .map_or((url, None), |(canonical, version)| {
            (canonical, Some(version))
        });
    let mut search = SearchParams::new()
        .with_count(1)
        .with_param("url", canonical);
    if let Some(version) = version {
        search = search.with_param("version", version);
    }
    context
        .search(state, "Measure", &search)
        .await?
        .entries
        .into_iter()
        .next()
        .map(|measure| measure.resource)
        .ok_or_else(|| OperationError::NotFound(format!("Measure {url} not found")))
}

/// Who a request evaluates.
struct Population {
    references: Vec<String>,
    /// The Patient resources, a chunk for each compiled library.
    chunks: Vec<Vec<Value>>,
    /// A report about one person.
    individual: bool,
}

/// The defines of each subject, in chunks.
async fn evaluate_chunks(
    service: &CqlService,
    source: &str,
    chunks: Vec<Vec<Value>>,
    request: &Request,
) -> Result<Vec<Vec<IndexMap<String, Value>>>, OperationError> {
    let period = (request.period.0.as_str(), request.period.1.as_str());
    stream::iter(chunks)
        .map(|chunk| service.evaluate_subjects(source, "Patient", chunk, period))
        .buffered(CHUNK_CONCURRENCY)
        .try_collect()
        .await
        .map_err(map_cql_error)
}

/// R5 turned the score into a choice element, and added `location` and `supplementalData`.
fn score_element(state: &AppState) -> &'static str {
    match state.fhir_version.as_str() {
        "R4" | "R4B" => "measureScore",
        _ => "measureScoreQuantity",
    }
}

/// The report around its groups.
fn report(
    state: &AppState,
    measure: &Value,
    request: &Request,
    population: &Population,
    groups: Vec<Value>,
    now: &str,
) -> Value {
    let report_type = match (
        population.individual,
        request.report_type.as_deref() == Some("subject-list"),
    ) {
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
        "group": groups,
    });
    if let Some(reporter) = &request.reporter {
        report["reporter"] = json!({"reference": reporter});
    }
    // MeasureReport has no location before R5.
    if let (Some(location), "measureScoreQuantity") = (&request.location, score_element(state)) {
        report["location"] = json!({"reference": location});
    }
    if population.individual {
        report["subject"] = json!({"reference": population.references.first()});
    } else if let Some(subject) = &request.subject {
        report["subject"] = json!({"reference": subject});
    }
    report
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
        if let Some(method) = composite::Method::of(measure)? {
            return self
                .evaluate_composite(state, context, &service, measure, method, request)
                .await;
        }
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
        for (group, scoring, by_resource) in &groups {
            group.check_observations(*scoring)?;
            let defines = group.observation_defines(*by_resource)?;
            if !defines.is_empty() {
                source.push('\n');
                source.push_str(&defines);
            }
        }

        let supplements = Supplemental::parse(measure)?;
        let mut population = self.population(state, context, request).await?;
        let chunks = std::mem::take(&mut population.chunks);
        let evaluated = evaluate_chunks(&service, &source, chunks, request).await?;
        let references = &population.references;
        let individual = population.individual;

        let subject_list = request.report_type.as_deref() == Some("subject-list");
        let now = Utc::now().to_rfc3339();
        let mut contained = Vec::new();
        let mut report_groups = Vec::with_capacity(groups.len());
        let score_element = score_element(state);
        let value_element = match score_element {
            "measureScore" => "value",
            _ => "valueCodeableConcept",
        };
        for (index, (group, scoring, by_resource)) in groups.iter().enumerate() {
            let (scoring, by_resource) = (*scoring, *by_resource);
            let mut tally = Tally::default();
            let mut strata: Vec<BTreeMap<String, Tally>> =
                vec![BTreeMap::new(); group.stratifiers.len()];
            for (defines, subject) in evaluated.iter().flatten().zip(references) {
                let classified = if by_resource {
                    group.resource_members(scoring, defines)
                } else {
                    vec![group.subject_member(scoring, defines)]
                };
                for member in classified {
                    tally.add(member, subject, group);
                    for (stratifier, by_value) in group.stratifiers.iter().zip(&mut strata) {
                        by_value
                            .entry(stratum_key(defines.get(*stratifier)))
                            .or_default()
                            .add(member, subject, group);
                    }
                }
            }
            let used: Vec<&str> = group.criteria.keys().copied().collect();
            let prefix = format!("group-{index}");
            let list = subject_list.then_some(prefix.as_str());
            if subject_list {
                contained.extend(contained_lists(&prefix, &used, &tally, &now));
            }
            let observations = group.observations.len();
            let mut result = json!({"population": tally.populations(&used, list, observations)});
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
                                    "population": tally.populations(&used, None, observations),
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

        let mut report = report(state, measure, request, &population, report_groups, &now);
        let reported = if individual {
            let resources = evaluated_resources(&groups, evaluated.iter().flatten().next());
            if !resources.is_empty() {
                report["evaluatedResource"] = json!(resources);
            }
            match evaluated.iter().flatten().next() {
                Some(defines) => supplemental::individual(&supplements, defines)?,
                None => supplemental::Reported::default(),
            }
        } else {
            let members = evaluated.iter().flatten().filter(|defines| {
                groups
                    .iter()
                    .any(|(group, ..)| group.in_define("initial-population", defines))
            });
            supplemental::summary(&supplements, members)?
        };
        contained.extend_from_slice(&reported.contained);
        reported.attach(&mut report, score_element != "measureScore");
        if !contained.is_empty() {
            report["contained"] = json!(contained);
        }
        Ok(report)
    }

    /// Who the request is about.
    async fn population(
        &self,
        state: &AppState,
        context: &OperationContext,
        request: &Request,
    ) -> Result<Population, OperationError> {
        let (subjects, individual) = match &request.members {
            Some(members) => (read_patients(state, context, members).await?, false),
            None => {
                self.subjects(state, context, request.subject.as_deref())
                    .await?
            }
        };
        Ok(Population {
            references: subjects
                .iter()
                .map(|patient| format!("Patient/{}", patient.id))
                .collect(),
            chunks: subjects
                .chunks(SUBJECT_CHUNK)
                .map(|chunk| chunk.iter().map(|p| p.resource.clone()).collect())
                .collect(),
            individual,
        })
    }

    /// A composite measure, from the populations of its components.
    async fn evaluate_composite(
        &self,
        state: &AppState,
        context: &OperationContext,
        service: &CqlService,
        measure: &Value,
        method: composite::Method,
        request: &Request,
    ) -> Result<Value, OperationError> {
        let references = composite::components(measure)?;
        let population = self.population(state, context, request).await?;
        let notation = composite::decreasing(measure, &Value::Null);
        let mut components = Vec::with_capacity(references.len());
        for reference in &references {
            let component = measure_by_url(state, context, reference.canonical).await?;
            let (index, group) = composite::component_group(&component, reference.group_id)?;
            if Scoring::of(&component, group)? != Scoring::Proportion
                || resource_based(&component, group)
            {
                return Err(OperationError::NotSupported(format!(
                    "Component {} is not a subject-based proportion",
                    reference.canonical
                )));
            }
            let parsed = Group::parse(index, group)?;
            let (_, source) = measure_library(service, &component).await?;
            let evaluated =
                evaluate_chunks(service, &source, population.chunks.clone(), request).await?;
            components.push(composite::Component {
                memberships: evaluated
                    .iter()
                    .flatten()
                    .map(|defines| parsed.subject_member(Scoring::Proportion, defines).0)
                    .collect(),
                weight: reference.weight,
                reversed: composite::decreasing(&component, group) != notation,
            });
        }

        let outcome = composite::evaluate(method, &components, &population.references);
        let now = Utc::now().to_rfc3339();
        let subject_list = request.report_type.as_deref() == Some("subject-list");
        let prefix = "group-0";
        let list = subject_list.then_some(prefix);
        let mut group = json!({});
        if !outcome.used.is_empty() {
            group["population"] = json!(outcome.tally.populations(
                outcome.used,
                list,
                outcome.observations
            ));
        }
        if let Some(score) = outcome.score {
            group[score_element(state)] = json!({"value": score});
        }
        let mut report = report(state, measure, request, &population, vec![group], &now);
        if subject_list && !outcome.used.is_empty() {
            report["contained"] =
                json!(contained_lists(prefix, outcome.used, &outcome.tally, &now));
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
    fn report_type(params: &Value, group: bool) -> Result<&'static str, OperationError> {
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

    /// The patients of a `subjectGroup`: a Group sent with the request, not stored here.
    fn subject_group(
        state: &AppState,
        params: &Value,
    ) -> Result<Option<Vec<String>>, OperationError> {
        let Some(parameter) = parameter(params, "subjectGroup") else {
            return Ok(None);
        };
        if text_parameter(params, "subject").is_some() {
            return Err(OperationError::InvalidParameters(
                "subject and subjectGroup cannot go together".into(),
            ));
        }
        let members = super::group::active_members(
            &parameter["resource"],
            &state.fhir_base_url(),
            OffsetDateTime::now_utc(),
        )?;
        members
            .into_iter()
            .map(|member| {
                if member.resource_type == "Patient" {
                    Ok(member.id)
                } else {
                    Err(OperationError::NotSupported(
                        "A subjectGroup lists Patients, not other Groups".into(),
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
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
        for name in ["reporterResource", "manifest"] {
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
        request.members = Self::subject_group(state, params)?;
        let group = request.members.is_some()
            || request
                .subject
                .as_deref()
                .is_some_and(|subject| subject.contains("Group/"));
        let report_type = Self::report_type(params, group)?;
        request.report_type = Some(report_type.to_owned());

        let mut bundles = Vec::new();
        if report_type == "individual" {
            let subjects = match request.members.take() {
                Some(members) => read_patients(state, context, &members).await?,
                None => {
                    EvaluateMeasureOperation
                        .subjects(state, context, request.subject.as_deref())
                        .await?
                        .0
                }
            };
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
        let source = json!({});
        let group = Group::parse(0, &source).unwrap();
        let mut tally = Tally::default();
        for (index, memberships) in subjects.iter().enumerate() {
            let classified = classify(scoring, |code| memberships.contains(&code));
            tally.add(
                (classified, [None; MAX_OBSERVATIONS]),
                &format!("Patient/{index}"),
                &group,
            );
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
    fn evaluated_resources_are_what_the_populations_hold() {
        let source = json!({"population": [
            {"code": {"coding": [{"code": "initial-population"}]}, "criteria": {"expression": "IP"}},
            {"code": {"coding": [{"code": "numerator"}]}, "criteria": {"expression": "N"}},
        ]});
        let groups = [(Group::parse(0, &source).unwrap(), Scoring::Cohort, false)];
        let defines = IndexMap::from([
            (
                "IP".to_owned(),
                json!([{"resourceType": "Encounter", "id": "1"}]),
            ),
            (
                "N".to_owned(),
                json!([{"resourceType": "Encounter", "id": "1"},
                    {"resourceType": "Condition", "id": "2"}]),
            ),
            (
                "Other".to_owned(),
                json!([{"resourceType": "Basic", "id": "3"}]),
            ),
        ]);
        assert_eq!(
            evaluated_resources(&groups, Some(&defines)),
            [
                json!({"reference": "Condition/2"}),
                json!({"reference": "Encounter/1"})
            ]
        );
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
            tally.add(member, "Patient/1", &group);
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
        assert_eq!(group.observations[0].aggregate, Aggregate::Median);
        assert_eq!(
            group.observation_defines(true).unwrap(),
            "define \"OctoFHIR Observation 0\": \"MP\" M return \
Tuple { id: M.id, value: \"Stay\"(M) }\n"
        );
        assert_eq!(
            group.observation_defines(false).unwrap(),
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
            tally.add(member, "Patient/1", &group);
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
        assert_eq!(observation[0], Some(7.0));
        assert!(memberships[8]);
    }

    fn ratio_group() -> Value {
        let observation = |id: &str, target: &str, function: &str| {
            json!({
                "id": id,
                "code": {"coding": [{"code": "measure-observation"}]},
                "criteria": {"expression": function},
                "extension": [
                    {"url": "http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-criteriaReference",
                     "valueString": target},
                    {"url": "http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-aggregateMethod",
                     "valueCode": "sum"},
                ],
            })
        };
        json!({"population": [
            {"code": {"coding": [{"code": "initial-population"}]}, "criteria": {"expression": "IP"}},
            {"id": "den", "code": {"coding": [{"code": "denominator"}]}, "criteria": {"expression": "D"}},
            {"code": {"coding": [{"code": "denominator-exclusion"}]}, "criteria": {"expression": "DX"}},
            {"id": "num", "code": {"coding": [{"code": "numerator"}]}, "criteria": {"expression": "N"}},
            {"code": {"coding": [{"code": "numerator-exclusion"}]}, "criteria": {"expression": "NX"}},
            observation("num-obs", "num", "NumberOf"),
            observation("den-obs", "den", "DaysOf"),
        ]})
    }

    #[test]
    fn a_ratio_divides_its_aggregates_without_the_excluded() {
        let source = ratio_group();
        let group = Group::parse(0, &source).unwrap();
        group.check_observations(Scoring::Ratio).unwrap();
        assert_eq!(group.observations[0].target, ObservationTarget::Numerator);
        assert_eq!(group.observations[1].target, ObservationTarget::Denominator);
        assert_eq!(
            group.observation_defines(false).unwrap(),
            "define \"OctoFHIR Observation 0 Numerator\": \"NumberOf\"()\n\
             define \"OctoFHIR Observation 0 Denominator\": \"DaysOf\"()\n"
        );

        let member = |ip: bool, d: bool, dx: bool, n: bool, nx: bool, num: f64, den: f64| {
            IndexMap::from([
                ("IP".to_owned(), json!(ip)),
                ("D".to_owned(), json!(d)),
                ("DX".to_owned(), json!(dx)),
                ("N".to_owned(), json!(n)),
                ("NX".to_owned(), json!(nx)),
                ("OctoFHIR Observation 0 Numerator".to_owned(), json!(num)),
                ("OctoFHIR Observation 0 Denominator".to_owned(), json!(den)),
            ])
        };
        let mut tally = Tally::default();
        for defines in [
            member(true, true, false, true, false, 2.0, 10.0),
            member(true, true, false, false, false, 5.0, 30.0),
            // Excluded on either side: nothing is observed there.
            member(true, true, true, true, true, 100.0, 100.0),
            // Outside the initial population.
            member(false, true, false, true, false, 100.0, 100.0),
        ] {
            tally.add(
                group.subject_member(Scoring::Ratio, &defines),
                "Patient/1",
                &group,
            );
        }
        assert_eq!(tally.observations[0], [2.0]);
        assert_eq!(tally.observations[1], [10.0, 30.0]);
        assert_eq!(group.score(&tally, Scoring::Ratio), Some(0.05));

        let used: Vec<&str> = group.criteria.keys().copied().collect();
        let rows = tally.populations(&used, None, 2);
        let observations: Vec<_> = rows
            .iter()
            .filter(|row| row["code"]["coding"][0]["code"] == OBSERVATION)
            .map(|row| row["count"].as_u64().unwrap())
            .collect();
        assert_eq!(observations, [1, 2]);
    }

    #[test]
    fn a_ratio_observes_both_sides_or_neither() {
        let mut source = ratio_group();
        source["population"].as_array_mut().unwrap().pop();
        let group = Group::parse(0, &source).unwrap();
        assert!(group.check_observations(Scoring::Ratio).is_err());
    }

    #[test]
    fn an_observation_refers_to_a_population_of_its_group() {
        let mut source = ratio_group();
        source["population"][5]["extension"][0]["valueString"] = json!("missing");
        assert!(Group::parse(0, &source).is_err());
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
        assert!(group.observation_defines(true).is_err());
    }
}
