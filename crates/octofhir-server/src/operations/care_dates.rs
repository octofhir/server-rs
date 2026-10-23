//! Clinical time selection for Patient $everything, independent of record time.

use serde_json::Value;
use time::OffsetDateTime;

use super::OperationError;
use octofhir_search::types::date::parse_date_range;

pub(crate) fn matches(
    resource: &Value,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<bool, OperationError> {
    if start.is_none() && end.is_none() {
        return Ok(true);
    }
    let lower = start
        .map(parse_date_range)
        .transpose()
        .map_err(invalid)?
        .map(|range| range.start);
    let upper = end
        .map(parse_date_range)
        .transpose()
        .map_err(invalid)?
        .map(|range| range.end);
    let fields: &[&str] = match resource["resourceType"].as_str().unwrap_or_default() {
        "Observation" | "DiagnosticReport" => &[
            "effectiveDateTime",
            "effectivePeriod",
            "effectiveTiming",
            "effectiveInstant",
        ],
        "Procedure" => &["performedDateTime", "performedPeriod", "performedTiming"],
        "Encounter" | "EpisodeOfCare" | "CarePlan" | "Coverage" => &["period"],
        "Condition" => &["onsetDateTime", "onsetPeriod"],
        "AllergyIntolerance" => &["onsetDateTime", "onsetPeriod", "lastOccurrence"],
        "Immunization" => &["occurrenceDateTime"],
        "ImmunizationEvaluation" => &["date"],
        "MedicationAdministration" => &["effectiveDateTime", "effectivePeriod"],
        "MedicationStatement" => &["effectiveDateTime", "effectivePeriod"],
        "MedicationDispense" => &["whenHandedOver", "whenPrepared"],
        "MedicationRequest" => &["dispenseRequest.validityPeriod", "dosageInstruction.timing"],
        "ServiceRequest" | "DeviceRequest" | "SupplyRequest" => {
            &["occurrenceDateTime", "occurrencePeriod", "occurrenceTiming"]
        }
        "Communication" => &["sent", "received"],
        "CommunicationRequest" => &["occurrenceDateTime", "occurrencePeriod"],
        "Appointment" | "Slot" => &["start"],
        "AppointmentResponse" => &["start"],
        "Schedule" => &["planningHorizon"],
        "NutritionOrder" => &[
            "oralDiet.schedule",
            "supplement.schedule",
            "enteralFormula.administration.schedule",
        ],
        "ClinicalImpression" => &["effectiveDateTime", "effectivePeriod"],
        "RiskAssessment" => &["occurrenceDateTime", "occurrencePeriod"],
        "FamilyMemberHistory" => &["condition.onsetPeriod"],
        "QuestionnaireResponse" => &["authored"],
        "DocumentReference" => &["context.period"],
        "Composition" => &["date"],
        "List" => &["date"],
        "Media" => &["createdDateTime", "createdPeriod"],
        "ImagingStudy" => &["started", "series.started"],
        "Specimen" => &["collection.collectedDateTime", "collection.collectedPeriod"],
        "SupplyDelivery" => &["occurrenceDateTime", "occurrencePeriod", "occurrenceTiming"],
        "DeviceUseStatement" => &["timingDateTime", "timingPeriod", "timingTiming"],
        "Task" => &["executionPeriod"],
        "ChargeItem" => &["occurrenceDateTime", "occurrencePeriod", "occurrenceTiming"],
        "Claim" | "ExplanationOfBenefit" => {
            &["billablePeriod", "item.servicedDate", "item.servicedPeriod"]
        }
        "AdverseEvent" => &["date"],
        "ResearchSubject" => &["period"],
        // Demographics, definitions and supporting records have no clinical
        // care interval. Patient.birthDate and meta.lastUpdated are not care dates.
        _ => &[],
    };
    let mut intervals = Vec::new();
    if resource["resourceType"] == "Condition" {
        let mut onset = Vec::new();
        let mut abatement = Vec::new();
        for field in ["onsetDateTime", "onsetPeriod"] {
            collect_path(&resource[field], &[], &mut onset)?;
        }
        for field in ["abatementDateTime", "abatementPeriod"] {
            collect_path(&resource[field], &[], &mut abatement)?;
        }
        if !onset.is_empty() || !abatement.is_empty() {
            let first = onset.iter().filter_map(|interval| interval.0).min();
            let last = abatement.iter().filter_map(|interval| interval.1).max();
            return Ok(
                lower.is_none_or(|lower| last.is_none_or(|last| last > lower))
                    && upper.is_none_or(|upper| first.is_none_or(|first| first < upper)),
            );
        }
    }
    for field in fields {
        collect_path(
            resource,
            &field.split('.').collect::<Vec<_>>(),
            &mut intervals,
        )?;
    }
    Ok(intervals.is_empty()
        || intervals.into_iter().any(|(first, last)| {
            lower.is_none_or(|lower| last.is_none_or(|last| last > lower))
                && upper.is_none_or(|upper| first.is_none_or(|first| first < upper))
        }))
}

type Interval = (Option<OffsetDateTime>, Option<OffsetDateTime>);

fn collect_path(
    value: &Value,
    path: &[&str],
    intervals: &mut Vec<Interval>,
) -> Result<(), OperationError> {
    if let Some(values) = value.as_array() {
        for value in values {
            collect_path(value, path, intervals)?;
        }
    } else if let Some((field, rest)) = path.split_first() {
        collect_path(&value[*field], rest, intervals)?;
    } else if let Some(value) = value.as_str() {
        let range = parse_date_range(value).map_err(invalid)?;
        intervals.push((Some(range.start), Some(range.end)));
    } else if value.is_object() {
        // Period endpoints are inclusive at their specified precision; a missing
        // endpoint is unbounded. Timing contributes events and boundsPeriod.
        if value.get("start").is_some() || value.get("end").is_some() {
            let first = value["start"]
                .as_str()
                .map(parse_date_range)
                .transpose()
                .map_err(invalid)?
                .map(|range| range.start);
            let last = value["end"]
                .as_str()
                .map(parse_date_range)
                .transpose()
                .map_err(invalid)?
                .map(|range| range.end);
            intervals.push((first, last));
        }
        collect_path(&value["event"], &[], intervals)?;
        collect_path(&value["repeat"]["boundsPeriod"], &[], intervals)?;
    }
    Ok(())
}

fn invalid(error: impl std::fmt::Display) -> OperationError {
    OperationError::InvalidParameters(format!("Invalid clinical date: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn care_time_overlaps_periods_and_never_uses_record_metadata() {
        let procedure = json!({"resourceType":"Procedure","performedPeriod":{"start":"2024","end":"2025-02"},"meta":{"lastUpdated":"2026-01-01T00:00:00Z"}});
        assert!(matches(&procedure, Some("2025-02-28"), Some("2025-02-28")).unwrap());
        assert!(!matches(&procedure, Some("2025-03"), None).unwrap());
        let patient = json!({"resourceType":"Patient","birthDate":"1930-01-01"});
        assert!(matches(&patient, Some("2025"), None).unwrap());
        let observation = json!({"resourceType":"Observation","effectiveDateTime":"2024-01-01","issued":"2025-01-01T00:00:00Z"});
        assert!(!matches(&observation, Some("2025"), None).unwrap());
        let request = json!({"resourceType":"MedicationRequest","dosageInstruction":[{"timing":{"repeat":{"boundsPeriod":{"start":"2025-01"}}}}]});
        assert!(matches(&request, Some("2026"), None).unwrap());
        let ongoing = json!({"resourceType":"Condition","onsetDateTime":"2020"});
        assert!(matches(&ongoing, Some("2025"), None).unwrap());
        let resolved =
            json!({"resourceType":"Condition","onsetDateTime":"2020","abatementDateTime":"2023"});
        assert!(!matches(&resolved, Some("2025"), None).unwrap());
    }
}
