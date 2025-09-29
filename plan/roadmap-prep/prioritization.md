# Prioritization & Dependencies

## Methodology
- Adopt **MoSCoW** prioritization for roadmap items (Must, Should, Could, Won't for now).
- Secondary ranking uses dependency count and risk weight; items with external dependencies flagged.

## Priority Table

| Workstream | Description | Priority | Key Dependencies |
| --- | --- | --- | --- |
| Base REST Server | CRUD, history, OperationOutcome per [FHIR REST](https://hl7.org/fhir/http.html). | Must | PostgreSQL schema, AccessPolicy, fhirschema validation. |
| PostgreSQL Storage | Dynamic tables + `{resource}_history` diff strategy. | Must | Canonical Manager metadata, ADR-001/002 implementation. |
| Search Engine | StructureDefinition-derived search and indexes per [FHIR Search](https://build.fhir.org/search.html). | Must | PostgreSQL storage, Canonical Manager, ADR-003. |
| Transactions | Bundle processing per [FHIR Bundle](https://hl7.org/fhir/bundle.html). | Must | Base REST, storage, search. |
| CapabilityStatement | Dynamic publication per [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html). | Must | Canonical Manager, operations, search coverage. |
| API Gateway (`App`) | Dynamic routing, proxy, integration with policies. | Should | AccessPolicy, config service, hot-reload. |
| OAuth/OIDC + SMART | Auth flows per [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749) and [SMART scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html). | Must | AccessPolicy, Gateway, Admin UI. |
| AccessPolicy DSL | Enforcement middleware and evaluation APIs. | Must | OAuth tokens, config merge, gateway hooks. |
| `$` Operations Framework | Canonical Manager registered operations per [FHIR Operations](https://build.fhir.org/operations.html). | Should | Base REST, hot-reload, CapabilityStatement. |
| Config Service & Hot-Reload | Merge ENV > DB > FILE with JSON Schema validation. | Must | PostgreSQL storage, Admin UI, ADR-006. |
| Admin UI Enhancements | Manage packages, policies, config, SMART clients. | Should | Admin APIs, OAuth integration. |
| Infrastructure | docker-compose + Just automation for Postgres, IdP, Canonical Manager. | Must | Storage, OAuth, Gateway readiness. |

## Dependency Graph (Textual)
1. **Canonical Manager Enhancements** → enables Schema Generation (ADR-002) → required for PostgreSQL Storage → prerequisite for REST CRUD + Search + Transactions.
2. **AccessPolicy DSL** → required before Gateway, OAuth/SMART enforcement, `$` operations, and Admin UI policy views.
3. **Config Service & Hot-Reload** → underpins CapabilityStatement regeneration, Gateway route updates, Admin UI controls.
4. **Infrastructure Stack** → needed to validate PostgreSQL, OAuth, and Gateway features in integrated environments.
