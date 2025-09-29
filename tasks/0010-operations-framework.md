# Task 0010 — Operations Framework

## Objective
Deliver Stage G milestones from [Roadmap v1](../docs/ROADMAP.md) by building an extensible `$` operations registry managed by Canonical Manager, enabling hot-reloadable handlers, and ensuring CapabilityStatement synchronization.

## Context & Rationale
- **Roadmap Alignment**: Expands server functionality beyond CRUD/Search and satisfies canonical package operation requirements.
- **Gap Closure**: Addresses operations support marked `Missing` in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Requires policy enforcement (Task 0009), configuration snapshots (Task 0011), and canonical metadata from Tasks 0001/0004.

## Deliverables
1. Operation registry ingesting Canonical Manager definitions aligned with [FHIR Operations](https://build.fhir.org/operations.html).
2. Execution engine supporting built-in `$validate`, `$meta`, and package-provided operations.
3. Hot-reload integration for operation handlers with rollback safeguards.
4. CapabilityStatement auto-update reflecting active operations and associated parameters.

## Acceptance Criteria (DoD)
- Operations enforce AccessPolicy and return [OperationOutcome](https://hl7.org/fhir/operationoutcome.html) on failure.
- Handlers can be added/removed without downtime via immutable snapshot + atomic swap.
- Admin UI receives operation catalog for display and testing hooks.

## Implementation Outline
- Model operation definitions and parameter schemas from canonical packages.
- Implement dispatcher executing async handlers and streaming responses where applicable.
- Provide telemetry capturing invocation counts, latency, and errors.

## Risks & Mitigations
- **Handler isolation**: Sandbox custom operations and enforce resource access limits.
- **Reload consistency**: Validate handler compatibility before activation.
- **Spec compliance**: Add conformance tests referencing official examples.

## References
- [FHIR Operations](https://build.fhir.org/operations.html)
- [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
