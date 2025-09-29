# Task 0009 — AccessPolicy DSL & Enforcement

## Objective
Complete Stage F milestones from [Roadmap v1](../docs/ROADMAP.md) by finalizing the AccessPolicy DSL, delivering evaluation APIs, and enforcing policy decisions across REST, search, `$` operations, and the API Gateway.

## Context & Rationale
- **Roadmap Alignment**: Ensures consistent authorization decisions leveraging SMART scopes and DSL expressions.
- **Gap Closure**: Addresses policy enforcement gaps recorded in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Depends on OAuth integration (Task 0008), Gateway routing (Task 0007), and configuration snapshots (Task 0011).

## Deliverables
1. DSL schema specification with validation returning [OperationOutcome](https://hl7.org/fhir/operationoutcome.html) diagnostics for syntax/semantic errors.
2. Policy evaluation service with test API for simulations and audit logging.
3. Middleware hooks in REST/search/operations/gateway enforcing decisions and logging denials.
4. Documentation describing DSL constructs, SMART scope mapping, and enforcement touchpoints.

## Acceptance Criteria (DoD)
- Policies evaluated prior to request processing; denials return OperationOutcome with SMART context references.
- Evaluation API supports dry-run mode used by Admin UI.
- Audit logs capture subject, scopes, decision, and policy version for compliance.

## Implementation Outline
- Finalize DSL grammar leveraging existing ADR drafts and integrate with configuration storage.
- Implement evaluation engine optimized for async workloads with caching for frequently used policies.
- Embed middleware into relevant services and ensure instrumentation meets observability goals.

## Risks & Mitigations
- **Performance**: Cache evaluation results and monitor latency.
- **Complex policies**: Provide debugging aids and detailed OperationOutcome extensions.
- **Consistency**: Ensure all entry points use same enforcement pipeline via shared library.

## References
- [SMART on FHIR Scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
- [FHIR REST](https://hl7.org/fhir/http.html)
