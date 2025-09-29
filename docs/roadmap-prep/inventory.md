# Current State Inventory

## REST API & HTTP Semantics
- Existing `octofhir-server` handles limited R4 interactions; R5 alignment pending.
- OperationOutcome generation basic; needs expansion for full [FHIR REST](https://hl7.org/fhir/http.html) compliance.
- Capability for conditional updates and transactions not yet wired.

## Storage (PostgreSQL)
- In-memory backend active; PostgreSQL adapter planned but incomplete.
- No dynamic per-resource tables or `{resource}_history` diff structures yet.
- Schema generation from Canonical Manager not automated.

## Search
- `octofhir-search` parses limited parameters; lacks automatic extraction from `StructureDefinition`.
- Index planning and query translation for PostgreSQL absent.
- Lacks advanced modifiers and chaining support per [FHIR Search](https://build.fhir.org/search.html).

## Canonical Manager & Dynamic Content
- Package manager scaffolding present; does not yet emit hot-reload events or generate CapabilityStatement.
- No automated `$` operation registration.

## Admin UI
- Current UI focuses on observability dashboards; no workflows for package install, AccessPolicy, or config management.
- No visualization for SMART client registration.

## Security & OAuth/OIDC
- OAuth integration stubbed; external IdP support and SMART scopes not implemented.
- AccessPolicy DSL parser exists but enforcement integration minimal.

## API Gateway (App Resource)
- Gateway service skeleton exists; dynamic route provisioning and policy hooks missing.
- No unified diagnostics for proxied calls.

## Configuration & Hot-Reload
- Config loaded from file with env overrides; database layer not integrated.
- No snapshot/rollback, JSON-Schema validation, or scoped hot-reload.

## Infrastructure
- Justfile contains build/test basics; docker-compose lacks Postgres + IdP services.
- CI pipelines for security/compliance not defined.
