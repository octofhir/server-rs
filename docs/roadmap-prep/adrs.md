# ADR Drafts

## ADR-001 Diff History Strategy
- **Decision**: Maintain per-resource `{resource}_history` tables storing snapshot checkpoints every K versions plus JSON Patch (RFC 6902) and MergePatch (RFC 7386) entries for intermediate revisions, aligned with [FHIR history](https://hl7.org/fhir/http.html#history).
- **Alternatives**: (a) Snapshot every version (high storage); (b) Store only deltas (slow replay); (c) External event log (operational overhead).
- **Rationale**: Balances read performance for `_history` and `_history/{vid}` retrieval with manageable storage; enables rollback and auditing.
- **Consequences**: Requires patch generation tooling and integrity checks; migrations must enforce FK to primary resource table.

## ADR-002 DDL Generation from StructureDefinition
- **Decision**: Use Canonical Manager + `fhirschema` metadata to produce PostgreSQL DDL on demand, mapping element types to JSONB columns and scalar indexes.
- **Alternatives**: (a) Static migrations per resource; (b) Generic JSONB bucket with runtime filtering; (c) ORM-driven manual schema.
- **Rationale**: Ensures schema stays synchronized with canonical packages and supports hot-installation of new resource types.
- **Consequences**: Need migration runner integrated with docker-compose and Just commands; must handle versioned upgrades gracefully.

## ADR-003 Index Planning Strategy
- **Decision**: Define canonical index templates per search parameter type (token/string/date/reference) with support for composite indexes for chained searches, following [FHIR Search](https://build.fhir.org/search.html).
- **Alternatives**: (a) Index on demand after observing queries; (b) Use full-text search only; (c) Denormalized search tables.
- **Rationale**: Provides predictable query performance and aligns with extracted search parameters from StructureDefinition.
- **Consequences**: Increases migration complexity; must monitor PostgreSQL size and vacuum overhead.

## ADR-004 Admin API Contracts
- **Decision**: Expose Admin API endpoints for packages, AccessPolicies, SMART clients, config snapshots, and CapabilityStatement inspection via REST returning [OperationOutcome](https://hl7.org/fhir/operationoutcome.html) for errors.
- **Alternatives**: (a) Direct database access; (b) CLI-only management; (c) gRPC interface.
- **Rationale**: Enables Admin UI and automation to manage runtime features consistently through Gateway.
- **Consequences**: Requires OAuth scopes and AccessPolicy coverage; documentation for internal consumers.

## ADR-005 AccessPolicy Enforcement Points
- **Decision**: Centralize policy evaluation in middleware invoked by REST handlers, search engine, `$` operations, and API Gateway, with caching keyed by subject/scope/context.
- **Alternatives**: (a) Distribute checks per service; (b) Use database row-level security; (c) Rely solely on SMART scopes.
- **Rationale**: Keeps DSL semantics consistent and auditable while leveraging SMART scopes as inputs.
- **Consequences**: Introduces latency overhead; must provide tracing for deny decisions.

## ADR-006 Config Merge & Hot-Reload Policy
- **Decision**: Implement configuration service layering ENV > DB > FILE, validating with JSON Schema and producing immutable snapshots for hot-reload operations.
- **Alternatives**: (a) File-only config; (b) DB-only with migrations; (c) Service restart required for changes.
- **Rationale**: Satisfies requirement for hot-reload without downtime and ensures governance via Admin UI.
- **Consequences**: Requires transaction-safe updates and rollback logs; interplay with docker-compose secrets.

## ADR-007 CapabilityStatement Assembly
- **Decision**: Generate CapabilityStatement via Canonical Manager aggregator after each package install or config change, referencing [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html).
- **Alternatives**: (a) Static YAML file; (b) Partial manual update; (c) Generate at runtime per request.
- **Rationale**: Ensures accuracy while avoiding per-request cost; updates triggered by hot-reload events keep statement current.
- **Consequences**: Need caching invalidation; Admin UI must display version metadata.
