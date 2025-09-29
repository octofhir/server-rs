# Task 0012 — Admin UI Expansion

## Objective
Execute Stage I milestones from [Roadmap v1](../docs/ROADMAP.md) by extending the Admin UI to manage Canonical Manager packages, AccessPolicy lifecycle, SMART client administration, and configuration snapshots.

## Context & Rationale
- **Roadmap Alignment**: Provides operator-facing tools essential for governance of new capabilities.
- **Gap Closure**: Addresses UI gaps noted in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Builds on backend capabilities from Tasks 0007-0011 and requires telemetry endpoints for status data.

## Deliverables
1. UI modules for package lifecycle (install, activate, rollback) with status derived from Canonical Manager and Task 0011 snapshots.
2. AccessPolicy editor/test console leveraging Task 0009 evaluation APIs.
3. SMART client management screens handling OAuth metadata, consent, and audit logs per [SMART App Launch](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html).
4. Configuration dashboard visualizing hot-reload status, operation catalogs, and index coverage.

## Acceptance Criteria (DoD)
- UI interactions call Admin APIs and display [OperationOutcome](https://hl7.org/fhir/operationoutcome.html)-formatted feedback.
- Role-based access ensures only authorized admins manage policies and clients.
- Usability validated through stakeholder walkthrough with recorded feedback.

## Implementation Outline
- Extend existing UI component library with domain-specific panels and real-time status indicators.
- Integrate WebSocket or SSE channels for live reload status updates.
- Provide localization-ready copy referencing specification links for operator guidance.

## Risks & Mitigations
- **Data consistency**: Poll backend for authoritative state and handle stale data gracefully.
- **Security**: Ensure admin routes require elevated AccessPolicy permissions.
- **UX complexity**: Run iterative reviews with target operators.

## References
- [SMART on FHIR Scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
- [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html)
