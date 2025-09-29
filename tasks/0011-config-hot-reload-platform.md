# Task 0011 — Configuration Merge & Hot-Reload Platform

## Objective
Implement Stage H milestones from [Roadmap v1](../docs/ROADMAP.md) by building the configuration merge service (ENV > DB > FILE), JSON Schema validation, snapshot/rollback store, and orchestrated hot-reload covering packages, search, operations, and gateway routes.

## Context & Rationale
- **Roadmap Alignment**: Provides foundational infrastructure for runtime adaptability and safe configuration changes.
- **Gap Closure**: Resolves configuration and hot-reload items tracked as `Missing` in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Depends on prior tasks for schema metadata, policy enforcement, and gateway operations to hook into reload notifications.

## Deliverables
1. Merge engine applying ENV > DB > FILE precedence with JSON Schema validation and OperationOutcome reporting.
2. Snapshot repository storing immutable configuration versions with rollback capabilities.
3. Hot-reload orchestrator triggering scoped refresh for schema, search parameters, indexes, operations, gateway routes, and policies.
4. Audit logging and Admin API endpoints to inspect configuration history.

## Acceptance Criteria (DoD)
- Configuration changes applied atomically with rollback on validation failure.
- Hot-reload actions do not interrupt in-flight requests and provide status telemetry.
- CapabilityStatement and Admin UI reflect updated configuration state post-reload.

## Implementation Outline
- Define configuration schema and validation pipeline leveraging JSON Schema libraries.
- Implement snapshot storage (e.g., PostgreSQL tables with blob storage) with retention policies.
- Wire reload triggers to downstream services via event bus or observer pattern with backpressure controls.

## Risks & Mitigations
- **Race conditions**: Serialize reload operations and monitor completion.
- **Validation gaps**: Maintain schema tests and align with ADR decisions.
- **Observability**: Expose metrics/logs for reload success/failure and duration.

## References
- [FHIR JSON](https://build.fhir.org/json.html)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
- [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html)
