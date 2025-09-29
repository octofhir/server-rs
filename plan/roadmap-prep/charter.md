# FHIR Server Enhancement Charter & Definition of Ready

## Charter
- **Mission**: Deliver a FHIR R5-compliant, async-first server runtime leveraging `octofhir` components with PostgreSQL dynamic per-resource storage, canonical-driven models via [`fhirschema`](https://github.com/octofhir/fhirschema), and SMART-on-FHIR aligned security, prioritizing maintainability over over-engineering.
- **Objectives**:
  - Implement resource ingestion, validation, search, and history features according to [FHIR RESTful API](https://hl7.org/fhir/http.html) and [FHIR JSON](https://build.fhir.org/json.html) semantics.
  - Automate schema and search extraction from `StructureDefinition` sources coordinated by Canonical Manager, producing CapabilityStatement snapshots per [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html).
  - Establish an OAuth2/OIDC SMART-on-FHIR security posture referencing [OAuth2 RFC 6749](https://www.rfc-editor.org/rfc/rfc6749), [OpenID Connect Core](https://openid.net/specs/openid-connect-core-1_0.html), and [SMART scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html).
  - Provide Admin UI extensions for runtime package management, policy oversight, and hot-reload orchestration.
  - Ensure infrastructure tooling (docker-compose, Justfile commands) enables reproducible deployment and operational workflows.
- **Key Stakeholders**: Platform engineering, Compliance, Security, Admin UI team, Integrations, Clinical app developers.
- **Deliverables**: Roadmap v1, domain checklists, PoC criteria, GAP analysis, ADR drafts, prioritization framework, and template roadmap artifacts stored under `docs/roadmap-prep/`.

## Definition of Ready (Roadmap Inputs)
- ✅ **Requirements Traceability**: Spec baseline matrix capturing sources, priorities, and notes for all mandatory capabilities (REST, search, operations, security, config, UI).
- ✅ **Current State Inventory**: Documented state of REST handling, storage, search, Admin UI, security/OAuth, AccessPolicy, configuration, and hot-reload coverage.
- ✅ **Domain Checklists**: Validated question sets for fhirschema usage, Canonical Manager dynamics, PostgreSQL per-resource tables with `{resource}_history` diffing, search parameter extraction, REST headers/errors per [OperationOutcome](https://hl7.org/fhir/operationoutcome.html), API Gateway App routing, OAuth/OIDC + SMART scopes, AccessPolicy enforcement, config merge and scoped hot-reload, and UI responsibilities.
- ✅ **PoC Criteria**: Clear success metrics and evaluation points for diff history, indexing strategy, hot-reload behavior, and routing flexibility referencing [FHIR Bundle transactions](https://hl7.org/fhir/bundle.html) and [FHIR operations](https://build.fhir.org/operations.html).
- ✅ **GAP Analysis**: Status (Done/Partial/Missing) with closure options and risk flags for each requirement, informed by Canonical Manager and fhirschema constraints.
- ✅ **ADR Drafts**: Concise decision outlines for diff strategy, DDL generation, index planning, Admin API contracts, AccessPolicy enforcement points, config merge/hot-reload policy, and CapabilityStatement assembly.
- ✅ **Prioritization Model**: Agreed scoring approach (MoSCoW with dependency mapping) covering base server, search, transactions, gateway, security, policies, operations, configuration, UI, and infrastructure.
- ✅ **Template Roadmap**: Approved structural outline including Definition of Done placeholders for each major stream.
