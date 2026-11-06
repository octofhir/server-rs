//! Composite measures: `Measure.compositeScoring` over the `composed-of` components.
//!
//! All-or-nothing and opportunity scoring work on the members of the component populations and
//! are proportions. Linear scoring is a continuous variable: what each subject achieved of what
//! they were eligible for, averaged. Weighted scoring combines the component scores. A
//! component whose improvement notation differs from the composite's has its numerator
//! reversed, or its score turned into one minus the score.

use serde_json::Value;

use super::super::OperationError;
use super::{Memberships, OBSERVATION, Scoring, Tally, classify};

const INITIAL: usize = 0;
const DENOMINATOR: usize = 1;
const EXCLUSION: usize = 2;
const EXCEPTION: usize = 3;
const NUMERATOR: usize = 4;
const NUMERATOR_EXCLUSION: usize = 5;
const MEASURE_POPULATION: usize = 6;

const PROPORTION_POPULATIONS: [&str; 6] = [
    "initial-population",
    "denominator",
    "denominator-exclusion",
    "denominator-exception",
    "numerator",
    "numerator-exclusion",
];
const LINEAR_POPULATIONS: [&str; 3] = ["initial-population", "measure-population", OBSERVATION];

/// `Measure.compositeScoring`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Method {
    AllOrNothing,
    Opportunity,
    Linear,
    Weighted,
}

fn code(concept: &Value) -> Option<&str> {
    concept["coding"]
        .as_array()?
        .iter()
        .find_map(|coding| coding["code"].as_str())
}

impl Method {
    /// The method of a composite measure, `None` for any other measure.
    pub(super) fn of(measure: &Value) -> Result<Option<Self>, OperationError> {
        if code(&measure["scoring"]) != Some("composite") {
            return Ok(None);
        }
        match code(&measure["compositeScoring"]) {
            Some("all-or-nothing") => Ok(Some(Self::AllOrNothing)),
            Some("opportunity") => Ok(Some(Self::Opportunity)),
            Some("linear") => Ok(Some(Self::Linear)),
            Some("weighted") => Ok(Some(Self::Weighted)),
            Some(other) => Err(OperationError::NotSupported(format!(
                "Composite scoring {other} is not supported"
            ))),
            None => Err(OperationError::InvalidParameters(
                "A composite Measure needs compositeScoring".into(),
            )),
        }
    }
}

/// A `composed-of` entry of `Measure.relatedArtifact`.
#[derive(Debug, PartialEq)]
pub(super) struct ComponentRef<'m> {
    pub(super) canonical: &'m str,
    pub(super) group_id: Option<&'m str>,
    pub(super) weight: f64,
}

fn extension<'a>(artifact: &'a Value, name: &str) -> Option<&'a Value> {
    artifact["extension"].as_array()?.iter().find(|extension| {
        extension["url"]
            .as_str()
            .is_some_and(|url| url.rsplit('/').next() == Some(name))
    })
}

pub(super) fn components(measure: &Value) -> Result<Vec<ComponentRef<'_>>, OperationError> {
    let components: Vec<ComponentRef<'_>> = measure["relatedArtifact"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|artifact| artifact["type"] == "composed-of")
        .map(|artifact| {
            Ok(ComponentRef {
                canonical: artifact["resource"].as_str().ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "A composed-of relatedArtifact has no resource".into(),
                    )
                })?,
                group_id: extension(artifact, "cqfm-groupId")
                    .and_then(|extension| extension["valueString"].as_str()),
                weight: extension(artifact, "cqfm-weight")
                    .and_then(|extension| extension["valueDecimal"].as_f64())
                    .unwrap_or(1.0),
            })
        })
        .collect::<Result<_, OperationError>>()?;
    if components.is_empty() {
        return Err(OperationError::InvalidParameters(
            "A composite Measure needs composed-of relatedArtifacts".into(),
        ));
    }
    Ok(components)
}

/// The group of a component measure a composite refers to, and its index.
pub(super) fn component_group<'m>(
    measure: &'m Value,
    group_id: Option<&str>,
) -> Result<(usize, &'m Value), OperationError> {
    let groups = measure["group"].as_array().map_or(&[][..], Vec::as_slice);
    match (group_id, groups) {
        (Some(id), _) => groups
            .iter()
            .enumerate()
            .find(|(_, group)| group["id"] == id || group["linkId"] == id)
            .ok_or_else(|| {
                OperationError::InvalidParameters(format!("A component has no group {id}"))
            }),
        (None, [group]) => Ok((0, group)),
        (None, _) => Err(OperationError::InvalidParameters(
            "A component with several groups needs cqfm-groupId".into(),
        )),
    }
}

/// Whether a measure counts a decrease as the improvement: from the group or the Measure.
pub(super) fn decreasing(measure: &Value, group: &Value) -> bool {
    code(&group["improvementNotation"]).or_else(|| code(&measure["improvementNotation"]))
        == Some("decrease")
}

/// One evaluated component: where each subject fell.
#[derive(Debug)]
pub(super) struct Component {
    pub(super) memberships: Vec<Memberships>,
    pub(super) weight: f64,
    /// Its improvement notation differs from the composite's.
    pub(super) reversed: bool,
}

impl Component {
    fn in_denominator(&self, subject: usize) -> bool {
        let member = &self.memberships[subject];
        member[DENOMINATOR] && !member[EXCLUSION] && !member[EXCEPTION]
    }

    fn in_numerator(&self, subject: usize) -> bool {
        let member = &self.memberships[subject];
        self.in_denominator(subject)
            && if self.reversed {
                !member[NUMERATOR]
            } else {
                member[NUMERATOR] && !member[NUMERATOR_EXCLUSION]
            }
    }

    fn score(&self) -> Option<f64> {
        let mut tally = Tally::default();
        for member in &self.memberships {
            tally.count(*member, "");
        }
        let score = tally.score(Scoring::Proportion)?;
        Some(if self.reversed { 1.0 - score } else { score })
    }
}

/// What a composite's report holds.
#[derive(Debug)]
pub(super) struct Outcome {
    pub(super) tally: Tally,
    pub(super) used: &'static [&'static str],
    pub(super) observations: usize,
    pub(super) score: Option<f64>,
}

/// Each population is the union over the components, and the numerator holds those in the
/// numerator of every component whose denominator they are in.
fn all_or_nothing(components: &[Component], subjects: &[String]) -> Outcome {
    let mut tally = Tally::default();
    for (index, subject) in subjects.iter().enumerate() {
        let any = |population: usize| {
            components
                .iter()
                .any(|component| component.memberships[index][population])
        };
        let eligible = components
            .iter()
            .any(|component| component.in_denominator(index));
        let member = classify(Scoring::Proportion, |code| match code {
            "initial-population" => any(INITIAL),
            "denominator" => any(DENOMINATOR),
            "denominator-exclusion" => any(EXCLUSION),
            "denominator-exception" => any(EXCEPTION),
            "numerator" => {
                eligible
                    && components.iter().all(|component| {
                        !component.in_denominator(index) || component.in_numerator(index)
                    })
            }
            _ => false,
        });
        tally.count(member, subject);
    }
    let score = tally.score(Scoring::Proportion);
    Outcome {
        tally,
        used: &PROPORTION_POPULATIONS,
        observations: 0,
        score,
    }
}

/// Each component a subject is in is a case of its own.
fn opportunity(components: &[Component], subjects: &[String]) -> Outcome {
    let mut tally = Tally::default();
    for component in components {
        for (index, subject) in subjects.iter().enumerate() {
            let mut member = component.memberships[index];
            member[NUMERATOR] = component.in_numerator(index);
            member[NUMERATOR_EXCLUSION] = false;
            tally.count(member, subject);
        }
    }
    let score = tally.score(Scoring::Proportion);
    Outcome {
        tally,
        used: &PROPORTION_POPULATIONS,
        observations: 0,
        score,
    }
}

/// The average, over subjects, of numerators reached out of denominators entered.
fn linear(components: &[Component], subjects: &[String]) -> Outcome {
    let mut tally = Tally::default();
    for (index, subject) in subjects.iter().enumerate() {
        let eligible = components
            .iter()
            .filter(|component| component.in_denominator(index))
            .count();
        let reached = components
            .iter()
            .filter(|component| component.in_numerator(index))
            .count();
        let mut member = Memberships::default();
        member[INITIAL] = components
            .iter()
            .any(|component| component.memberships[index][INITIAL]);
        member[MEASURE_POPULATION] = eligible > 0;
        tally.count(member, subject);
        if eligible > 0 {
            tally.observations[0].push(reached as f64 / eligible as f64);
        }
    }
    let values = &tally.observations[0];
    let score = (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64);
    Outcome {
        tally,
        used: &LINEAR_POPULATIONS,
        observations: 1,
        score,
    }
}

/// The weighted average of the component scores; a component without a score is left out.
fn weighted(components: &[Component]) -> Outcome {
    let (sum, weights) = components
        .iter()
        .filter_map(|component| Some((component.score()?, component.weight)))
        .fold((0.0, 0.0), |(sum, weights), (score, weight)| {
            (sum + score * weight, weights + weight)
        });
    Outcome {
        tally: Tally::default(),
        used: &[],
        observations: 0,
        score: (weights > 0.0).then(|| sum / weights),
    }
}

pub(super) fn evaluate(method: Method, components: &[Component], subjects: &[String]) -> Outcome {
    match method {
        Method::AllOrNothing => all_or_nothing(components, subjects),
        Method::Opportunity => opportunity(components, subjects),
        Method::Linear => linear(components, subjects),
        Method::Weighted => weighted(components),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Subjects as `(in the denominator, in the numerator)`.
    fn component(subjects: &[(bool, bool)]) -> Component {
        Component {
            memberships: subjects
                .iter()
                .map(|(denominator, numerator)| {
                    let mut member = Memberships::default();
                    member[INITIAL] = *denominator;
                    member[DENOMINATOR] = *denominator;
                    member[NUMERATOR] = *denominator && *numerator;
                    member
                })
                .collect(),
            weight: 1.0,
            reversed: false,
        }
    }

    fn subjects(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("Patient/{index}")).collect()
    }

    fn close(left: Option<f64>, right: f64) {
        assert!(
            (left.unwrap() - right).abs() < 1e-9,
            "{left:?} is not {right}"
        );
    }

    /// The breast and colorectal screening example: a man is not eligible for the first, and
    /// that does not keep him from the numerator of the composite.
    fn screenings() -> [Component; 2] {
        [
            // Subject 0 has both done, 1 only the first, 2 is not eligible for the first.
            component(&[(true, true), (true, true), (false, false)]),
            component(&[(true, true), (true, false), (true, true)]),
        ]
    }

    #[test]
    fn all_or_nothing_asks_for_every_numerator_a_subject_is_eligible_for() {
        let outcome = evaluate(Method::AllOrNothing, &screenings(), &subjects(3));
        assert_eq!(outcome.tally.counts[DENOMINATOR], 3);
        assert_eq!(outcome.tally.counts[NUMERATOR], 2);
        close(outcome.score, 2.0 / 3.0);
    }

    #[test]
    fn all_or_nothing_counts_nobody_who_was_eligible_for_nothing() {
        let mut excepted = component(&[(true, false)]);
        excepted.memberships[0][EXCEPTION] = true;
        let outcome = evaluate(Method::AllOrNothing, &[excepted], &subjects(1));
        assert_eq!(outcome.tally.counts[NUMERATOR], 0);
        assert_eq!(outcome.tally.counts[EXCEPTION], 1);
        assert_eq!(outcome.score, None);
    }

    #[test]
    fn opportunity_makes_each_component_a_case() {
        let outcome = evaluate(Method::Opportunity, &screenings(), &subjects(3));
        assert_eq!(outcome.tally.counts[DENOMINATOR], 5);
        assert_eq!(outcome.tally.counts[NUMERATOR], 4);
        close(outcome.score, 0.8);
    }

    #[test]
    fn linear_averages_what_each_subject_reached() {
        let outcome = evaluate(Method::Linear, &screenings(), &subjects(3));
        assert_eq!(outcome.tally.observations[0], [1.0, 0.5, 1.0]);
        close(outcome.score, 2.5 / 3.0);
        assert_eq!(outcome.tally.counts[MEASURE_POPULATION], 3);
    }

    #[test]
    fn weighted_is_the_weighted_average_of_the_scores() {
        let [mut first, mut second] = screenings();
        first.weight = 0.25; // score 1.0
        second.weight = 0.75; // score 2/3
        let outcome = evaluate(Method::Weighted, &[first, second], &subjects(3));
        close(outcome.score, 0.25 + 0.75 * 2.0 / 3.0);
    }

    /// The table of the implementation guide: 80/100, 80/100 and a decrease measure at 20/100.
    #[test]
    fn a_reversed_component_counts_as_one_minus_its_score() {
        let of_100 = |numerators: usize| {
            let rows: Vec<(bool, bool)> = (0..100).map(|row| (true, row < numerators)).collect();
            component(&rows)
        };
        let mut decrease = of_100(20);
        decrease.reversed = true;
        let components = [of_100(80), of_100(80), decrease];
        close(evaluate(Method::Weighted, &components, &[]).score, 0.8);
        // The individual table: the absence from the numerator is what counts.
        let outcome = evaluate(Method::Opportunity, &components, &subjects(100));
        assert_eq!(outcome.tally.counts[NUMERATOR], 240);
        close(outcome.score, 0.8);
    }

    #[test]
    fn a_component_without_a_score_is_left_out_of_the_average() {
        let empty = component(&[(false, false)]);
        let done = component(&[(true, true)]);
        close(evaluate(Method::Weighted, &[empty, done], &[]).score, 1.0);
    }

    #[test]
    fn the_method_comes_from_a_composite_measure_only() {
        let measure = |scoring: &str, method: &str| {
            json!({"scoring": {"coding": [{"code": scoring}]},
                "compositeScoring": {"coding": [{"code": method}]}})
        };
        assert_eq!(Method::of(&measure("proportion", "linear")).unwrap(), None);
        assert_eq!(
            Method::of(&measure("composite", "opportunity")).unwrap(),
            Some(Method::Opportunity)
        );
        assert!(Method::of(&json!({"scoring": {"coding": [{"code": "composite"}]}})).is_err());
    }

    #[test]
    fn components_carry_their_group_and_weight() {
        let measure = json!({"relatedArtifact": [
            {"type": "citation", "display": "not a component"},
            {"type": "composed-of", "resource": "http://example.org/Measure/a|1", "extension": [
                {"url": "http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-groupId",
                 "valueString": "group-1"},
                {"url": "http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-weight",
                 "valueDecimal": 0.2},
            ]},
            {"type": "composed-of", "resource": "http://example.org/Measure/b"},
        ]});
        assert_eq!(
            components(&measure).unwrap(),
            [
                ComponentRef {
                    canonical: "http://example.org/Measure/a|1",
                    group_id: Some("group-1"),
                    weight: 0.2
                },
                ComponentRef {
                    canonical: "http://example.org/Measure/b",
                    group_id: None,
                    weight: 1.0
                },
            ]
        );
        assert!(components(&json!({})).is_err());
    }

    #[test]
    fn a_group_is_picked_by_id_and_needed_when_there_are_several() {
        let single = json!({"group": [{"id": "only"}]});
        assert_eq!(component_group(&single, None).unwrap().0, 0);
        let several = json!({"group": [{"id": "a"}, {"id": "b"}]});
        assert_eq!(component_group(&several, Some("b")).unwrap().0, 1);
        assert!(component_group(&several, None).is_err());
        assert!(component_group(&several, Some("c")).is_err());
    }

    #[test]
    fn the_improvement_notation_of_a_group_comes_first() {
        let decrease = json!({"coding": [{"code": "decrease"}]});
        let increase = json!({"coding": [{"code": "increase"}]});
        let measure = json!({"improvementNotation": decrease});
        assert!(decreasing(&measure, &json!({})));
        assert!(!decreasing(
            &measure,
            &json!({"improvementNotation": increase})
        ));
    }
}
