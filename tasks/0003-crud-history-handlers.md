# Task 0003 — CRUD & History Handlers

## Objective
Complete Stage A Milestone 3 from [Roadmap v1](../docs/ROADMAP.md) by delivering RESTful CRUD endpoints and `_history` support that adhere to [FHIR REST](https://hl7.org/fhir/http.html#crud) and integrate with the diff persistence model defined in ADR-001.

## Context & Rationale
- **Roadmap Alignment**: Finalizes Stage A DoD, enabling higher layers (search, transactions) to depend on stable CRUD semantics.
- **Gap Closure**: Resolves `Missing` status for REST operations and history coverage in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Builds atop Tasks 0001 and 0002 for schema and validation; references ADR-002 (DDL Generation) for storage conventions.

## Deliverables
1. Async CRUD handlers (`create`, `read`, `update`, `delete`, `vread`, `_history`) honoring ETag/Last-Modified headers per [FHIR RESTful API](https://hl7.org/fhir/http.html#general).
2. Diff persistence pipeline writing snapshot-K pointers and JSON Patch/Merge payloads into `{resource}_history` tables.
3. CapabilityStatement update advertising supported interactions and history modes.
4. OperationOutcome mapping for error cases (conditional conflicts, validation failures, precondition checks).

## Acceptance Criteria (DoD)
- REST endpoints return [FHIR JSON](https://build.fhir.org/json.html) payloads with correct status codes.
- History queries paginate correctly and include version metadata aligned with [FHIR history](https://hl7.org/fhir/http.html#history).
- Unit/integration tests cover optimistic concurrency, conditional update/delete, and diff storage correctness.
- Admin instrumentation exposes metrics for CRUD latency and history volume.

## Implementation Outline
- Build service layer orchestrating validation (Task 0002), schema metadata (Task 0001), and storage writes.
- Implement conditional logic using query planners that leverage generated indexes when available.
- Extend Admin API or telemetry to surface CRUD/history statistics and integrate with CapabilityStatement builder.

## Risks & Mitigations
- **Race conditions**: Use transactions and row-level locking strategies to prevent diff gaps.
- **History bloat**: Document retention policies and implement cleanup toggles once config platform (Stage H) matures.
- **Spec compliance**: Cross-check responses against [FHIR REST](https://hl7.org/fhir/http.html) examples; add contract tests.

## References
- [FHIR REST](https://hl7.org/fhir/http.html)
- [FHIR JSON](https://build.fhir.org/json.html)
- [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
