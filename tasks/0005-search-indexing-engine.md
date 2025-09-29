# Task 0005 — Search Indexing Engine

## Objective
Deliver Stage B Milestone 2 from [Roadmap v1](../docs/ROADMAP.md) by generating PostgreSQL token/string/date/reference indexes informed by ADR-003 and the descriptors produced in Task 0004.

## Context & Rationale
- **Roadmap Alignment**: Provides infrastructure for performant search queries and enables Stage B DoD.
- **Gap Closure**: Closes outstanding storage/index concerns in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Requires metadata from Tasks 0001, 0004 and schema generation from Task 0001; coordinates with Canonical Manager reload workflow.

## Deliverables
1. Index planner that maps search descriptors to concrete PostgreSQL index statements with lifecycle metadata.
2. Migration executor applying indexes transactionally with rollback support per hot-reload strategy.
3. Telemetry capturing index build duration and status for Admin UI display.
4. Documentation linking each index type to [FHIR Search](https://build.fhir.org/search.html) parameter behaviors.

## Acceptance Criteria (DoD)
- Token/string/date/reference parameters supported with selective use of GIN/B-Tree indexes as per cardinality guidance.
- Index definitions exported to CapabilityStatement extensions or Admin API for discoverability.
- Hot-reload orchestrator batches index updates alongside schema adjustments without downtime.

## Implementation Outline
- Convert descriptors into index templates, selecting appropriate data types and operators.
- Implement dependency graph to sequence indexes relative to table availability and constraints.
- Add integration coverage verifying search latency improvements on seeded datasets.

## Risks & Mitigations
- **Lock contention**: Use `CONCURRENTLY` builds when feasible and monitor via Admin UI.
- **Storage bloat**: Track index size and expose metrics; document pruning strategies.
- **Spec drift**: Keep descriptors versioned and ensure CapabilityStatement sync.

## References
- [FHIR Search](https://build.fhir.org/search.html)
- [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html)
- [FHIR Bundle](https://hl7.org/fhir/bundle.html) (for transaction safety considerations)
