# Task 0007 — API Gateway & App Resource

## Objective
Advance Stage D milestones from [Roadmap v1](../docs/ROADMAP.md) by defining the custom `App` resource, extending the Admin API for route registration, and implementing dynamic proxy routing with AccessPolicy checks.

## Context & Rationale
- **Roadmap Alignment**: Establishes the API Gateway foundation required for downstream SMART and policy enforcement.
- **Gap Closure**: Addresses routing and gateway items marked `Missing` in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Depends on Tasks 0003 and 0009 (policy DSL) for enforcement integration and Stage H for hot-reload mechanics.

## Deliverables
1. Canonical definition of `App` resource stored via Canonical Manager and surfaced in CapabilityStatement extensions.
2. Admin API endpoints to register/update/remove routes with validation returning [OperationOutcome](https://hl7.org/fhir/operationoutcome.html) payloads.
3. Runtime router supporting path parameters and wildcard segments with AccessPolicy checks per request.
4. Observability hooks capturing latency/error metrics for each proxied route.

## Acceptance Criteria (DoD)
- Route changes propagate through hot-reload pipeline without restart and roll back on failure.
- Gateway enforces AccessPolicy decisions before proxying and returns compliant OperationOutcome on denial.
- Admin UI receives metadata for display (Task 0012).

## Implementation Outline
- Model `App` resource schema referencing ADR-004 (Admin API contracts) and integrate with Canonical Manager.
- Build router capable of dynamic updates using immutable snapshot + atomic swap pattern.
- Add instrumentation (metrics/logging) tagged by route and client identity.

## Risks & Mitigations
- **Security gaps**: Ensure AccessPolicy enforcement runs prior to proxy dispatch and audit denials.
- **Reload race conditions**: Use staged configuration snapshots to swap routes atomically.
- **Spec alignment**: Document CapabilityStatement extensions referencing [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html) guidelines.

## References
- [FHIR REST](https://hl7.org/fhir/http.html)
- [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
