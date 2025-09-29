# Task 0002 — fhirschema R5 Validation Pipeline

## Objective
Implement Stage A Milestone 1 from [Roadmap v1](../docs/ROADMAP.md) by upgrading to the latest [`fhirschema`](https://github.com/octofhir/fhirschema) R5 package set and establishing a validation service that returns [OperationOutcome](https://hl7.org/fhir/operationoutcome.html)-compliant diagnostics for resource ingestion.

## Context & Rationale
- **Roadmap Alignment**: Unblocks CRUD handlers and CapabilityStatement population described in Stage A.
- **Gap Closure**: Addresses validation and diagnostics gaps marked `Missing` within the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Requires ADR-001 (Diff Strategy) notes on snapshot handling and ensures compatibility with Canonical Manager package lifecycle.

## Deliverables
1. `fhirschema` integration upgraded to R5 snapshots with supplemental R4B notes for divergence tracking.
2. Validation service exposing async entry points used by REST, bundle, and search workflows.
3. Diagnostic mapper translating `fhirschema` errors into OperationOutcome issues with references to the triggering [FHIR JSON](https://build.fhir.org/json.html) elements.
4. Developer guide describing validation lifecycle and ties to [FHIR REST](https://hl7.org/fhir/http.html#summary) semantics.

## Acceptance Criteria (DoD)
- Validation covers at least `Patient`, `Observation`, and `Practitioner` canonical examples and surfaces multiple severity levels per [FHIR OperationOutcome.issue.severity](https://hl7.org/fhir/operationoutcome-definitions.html#OperationOutcome.issue.severity).
- Canonical Manager package reload triggers validation schema refresh without server restart per hot-reload policy.
- CapabilityStatement reflects validation interactions in conformance with [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html).

## Implementation Outline
- Wire `fhirschema` to ingest Canonical Manager snapshots and produce reusable validators.
- Implement async validation API returning OperationOutcome plus structured metadata for auditing.
- Add integration coverage confirming errors propagate through REST create/update and [FHIR Bundle](https://hl7.org/fhir/bundle.html) transaction preflight checks.

## Risks & Mitigations
- **Schema drift**: Track package hash in metadata store and block inconsistent reloads.
- **Performance**: Cache compiled validators and measure warm/cold latency.
- **Version parity**: Capture R4B delta report for resources with dual-version usage.

## References
- [FHIR REST](https://hl7.org/fhir/http.html)
- [FHIR JSON](https://build.fhir.org/json.html)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
- [FHIR Bundle](https://hl7.org/fhir/bundle.html)
- [`fhirschema` library](https://github.com/octofhir/fhirschema)
