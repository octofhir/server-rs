# Roadmap v1 (FHIR R5 Server Enhancements)

## Overview
Establish an async-first, PostgreSQL-backed FHIR R5 server aligned with [`fhirschema`](https://github.com/octofhir/fhirschema), Canonical Manager dynamics, and SMART-on-FHIR security. All milestones respect [FHIR REST](https://hl7.org/fhir/http.html), [FHIR JSON](https://build.fhir.org/json.html), [FHIR Search](https://build.fhir.org/search.html), and supporting specifications.

## Stage A: Base Server & Storage Foundation
- **Scope**: Implement R5 CRUD handlers, resource validation, diff history persistence, and CapabilityStatement scaffolding.
- **Milestones**:
  1. `fhirschema` R5 upgrade with validation pipeline emitting [OperationOutcome](https://hl7.org/fhir/operationoutcome.html) diagnostics.
  2. Canonical Manager-driven schema generator producing per-resource tables and `{resource}_history` diff strategy (ADR-001/002).
  3. CRUD + `_history` handlers with ETag/Last-Modified compliance per [FHIR REST](https://hl7.org/fhir/http.html#ops).
- **Definition of Done**:
  - PostgreSQL storage created on demand with snapshot-K + JSON Patch/Merge diffs.
  - Validation failures surfaced via OperationOutcome; success returns FHIR JSON serialization.
  - CapabilityStatement shell published describing REST interactions per [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html).

## Stage B: Search & Index Enablement
- **Scope**: Enable R5 search using StructureDefinition metadata and PostgreSQL indexes.
- **Milestones**:
  1. Extract search parameters from Canonical Manager packages referencing [Search Parameter Registry](https://hl7.org/fhir/searchparameter-registry.html).
  2. Generate token/string/date/reference indexes guided by ADR-003.
  3. Implement search handlers with modifiers, chains, `_include`, `_revinclude` per [FHIR Search](https://build.fhir.org/search.html).
- **Definition of Done**:
  - Search parameters automatically exposed in CapabilityStatement.
  - Query latency targets met across representative workloads.
  - Admin UI lists searchable fields and index status.

## Stage C: Transactions & Bundles
- **Scope**: Support atomic bundles, conditional operations, and history integration.
- **Milestones**:
  1. Transaction executor enforcing [FHIR Bundle](https://hl7.org/fhir/bundle.html) sequencing and rollback semantics.
  2. Conditional create/update/delete honoring `If-Match` headers per [FHIR REST](https://hl7.org/fhir/http.html#condupdate).
  3. Bundle-level auditing with diff history snapshots and OperationOutcome aggregation.
- **Definition of Done**:
  - Transactions commit atomically with diff history entries for all affected resources.
  - Errors consolidated into OperationOutcome payloads per bundle entry.
  - CapabilityStatement advertises `transaction` and `batch` support.

## Stage D: API Gateway & App Resource
- **Scope**: Deliver dynamic routing, proxy integration, and hot-reload aware gateway.
- **Milestones**:
  1. Define `App` resource schema and Admin API for route registration.
  2. Implement runtime router supporting path parameters and wildcards with AccessPolicy checks.
  3. Integrate hot-reload pipeline for route changes with immutable snapshots.
- **Definition of Done**:
  - Gateway updates routes without restart; failures return OperationOutcome diagnostics.
  - Route metadata surfaced in CapabilityStatement extensions.
  - Monitoring exports latency/error metrics for proxied services.

## Stage E: OAuth/OIDC & SMART Enablement
- **Scope**: Integrate external IdPs, OAuth flows, and SMART scopes.
- **Milestones**:
  1. Implement Authorization Code + PKCE and Client Credentials flows per [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749) and [OIDC Core](https://openid.net/specs/openid-connect-core-1_0.html).
  2. Map SMART scopes following [SMART App Launch](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html) to AccessPolicy attributes.
  3. Expose token introspection and revocation endpoints for Admin UI tooling.
- **Definition of Done**:
  - Tokens validated, scopes enforced on REST/search/operations.
  - SMART launch context stored for session auditing.
  - Admin UI supports client registration and consent review.

## Stage F: AccessPolicy DSL & Enforcement
- **Scope**: Finalize DSL, evaluation APIs, and enforcement across entry points.
- **Milestones**:
  1. Publish DSL schema with validation errors using OperationOutcome.
  2. Embed policy middleware in REST, search, `$` operations, and gateway.
  3. Provide policy evaluation/test API with audit logging.
- **Definition of Done**:
  - Policy decisions logged with trace identifiers.
  - Denied requests return OperationOutcome with SMART scope context.
  - Admin UI shows policy status and simulation outcomes.

## Stage G: Operations ($) Framework
- **Scope**: Support Canonical Manager registered `$` operations and custom logic.
- **Milestones**:
  1. Implement operation registry honoring [FHIR Operations](https://build.fhir.org/operations.html).
  2. Provide hot-reload for operation handlers with rollback safeguards.
  3. Document capability exposure in CapabilityStatement and Admin UI.
- **Definition of Done**:
  - `$validate`, `$meta`, and package-provided operations executable with policy enforcement.
  - Error handling via OperationOutcome per operation spec.
  - CapabilityStatement auto-updates after operation changes.

## Stage H: Configuration & Hot-Reload Platform
- **Scope**: Deliver ENV>DB>FILE merge, JSON Schema validation, snapshots, and reload orchestration.
- **Milestones**:
  1. Build configuration service applying merge order with schema validation.
  2. Implement snapshot/rollback store with immutable artifacts.
  3. Integrate scoped hot-reload for packages, search, operations, and gateway.
- **Definition of Done**:
  - Config changes applied atomically with audit trail.
  - Admin UI exposes history and rollback controls.
  - CapabilityStatement and Gateway refresh automatically post-change.

## Stage I: Admin UI Expansion
- **Scope**: Extend UI to manage packages, policies, SMART clients, and configuration.
- **Milestones**:
  1. Add package lifecycle views reflecting Canonical Manager state.
  2. Implement AccessPolicy editor/test console and SMART client management.
  3. Provide config snapshot visualization and hot-reload status dashboard.
- **Definition of Done**:
  - UI actions backed by Admin API using OperationOutcome for feedback.
  - SMART-aware authentication integrated with [SMART scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html).
  - UX validated with stakeholder walkthroughs.

## Stage J: Infrastructure & Tooling
- **Scope**: Provide end-to-end environment automation.
- **Milestones**:
  1. Update docker-compose with PostgreSQL, Canonical Manager, Gateway, IdP stub.
  2. Extend Justfile with commands for build/run/test, migrations, policy eval.
  3. Define CI workflows covering lint, tests, and security checks.
- **Definition of Done**:
  - Local environment spins up full stack with seeded data.
  - CI validates REST/search bundles and security flows.
  - Documentation references [FHIR JSON](https://build.fhir.org/json.html) payload expectations for testers.

## Cross-Cutting Deliverables
- ADRs finalized (diff strategy, schema, indexes, Admin API, policy enforcement, config, CapabilityStatement).
- GAP analysis tracked and closed per stage entry criteria.
- PoC results documented before promoting to production workstreams.
