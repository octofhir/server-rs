# Task 0006 — Transaction & Bundle Executor

## Objective
Fulfill Stage C Milestone 1 from [Roadmap v1](../docs/ROADMAP.md) by implementing a transaction executor that enforces [FHIR Bundle](https://hl7.org/fhir/bundle.html) sequencing, atomicity, and rollback semantics, integrating with diff history persistence.

## Context & Rationale
- **Roadmap Alignment**: Enables transactional workflows required for batch ingest and complex client operations.
- **Gap Closure**: Targets `Missing` bundle handling noted in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Builds atop Tasks 0003 and 0005 to reuse CRUD storage and index capabilities.

## Deliverables
1. Transaction orchestrator handling `transaction` and `batch` bundle types with OperationOutcome aggregation.
2. Conditional create/update/delete logic honoring [FHIR REST](https://hl7.org/fhir/http.html#condupdate) preconditions inside bundles.
3. Diff history integration ensuring every entry writes version metadata consistent with standalone CRUD.
4. CapabilityStatement update reflecting bundle support and documented failure behaviors.

## Acceptance Criteria (DoD)
- Atomicity ensured via database transactions; partial failures roll back and return aggregated OperationOutcome issues.
- Supports references across bundle entries, resolving dependencies per spec.
- Integration tests cover success/failure scenarios and demonstrate compliance with [FHIR Bundle](https://hl7.org/fhir/bundle.html) processing rules.

## Implementation Outline
- Extend REST layer with transaction endpoint invoking orchestrator.
- Implement resolver for relative/urn references and manage provisional IDs per spec.
- Add metrics capturing transaction throughput and failure reasons for observability.

## Risks & Mitigations
- **Deadlocks**: Sequence operations deterministically and monitor lock timeouts.
- **Complex references**: Validate referential integrity pre-commit and provide descriptive OperationOutcome issues.
- **Performance**: Benchmark large bundles and optimize index usage.

## References
- [FHIR Bundle](https://hl7.org/fhir/bundle.html)
- [FHIR REST Conditional Operations](https://hl7.org/fhir/http.html#condupdate)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
