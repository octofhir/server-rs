# Spec Baseline Matrix

| Source | Requirement | Notes | Priority |
| --- | --- | --- | --- |
| [FHIR RESTful API R5](https://hl7.org/fhir/http.html) | Support CRUD + conditional interactions, transaction bundles, and correct HTTP semantics (headers, status codes). | Include `Prefer:return=representation`, ETag/versioning, and align errors with [OperationOutcome](https://hl7.org/fhir/operationoutcome.html). | Must |
| [FHIR Bundle R5](https://hl7.org/fhir/bundle.html) | Process batch/transaction bundles atomically with diff history updates. | Use Canonical Manager for resource resolution during bundle commits. | Must |
| [FHIR JSON Format](https://build.fhir.org/json.html) | Serialize/deserialize with `fhirschema` generated models ensuring strict property handling. | Validate required/optional fields and handle extensions. | Must |
| [FHIR Search](https://build.fhir.org/search.html) & [Search Parameter Registry](https://hl7.org/fhir/searchparameter-registry.html) | Implement parameter extraction from `StructureDefinition` and support token/string/date/reference combos. | Derive indexes and query plans per resource; include modifiers (`:exact`, `:contains`). | Must |
| [FHIR Operations Framework](https://build.fhir.org/operations.html) | Enable Canonical Manager to register custom and standard `$` operations. | Provide hot-reload for operation handlers via package updates. | Should |
| [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html) | Publish dynamic CapabilityStatement reflecting installed packages, search, and operations. | Auto-generate after hot-reload or package changes. | Must |
| [`fhirschema` library](https://github.com/octofhir/fhirschema) | Use for model validation, canonical ingestion, and JSON Schema generation. | Align version with R5; document R4B deviations where necessary. | Must |
| Internal Canonical Manager spec | Manage lifecycle of StructureDefinition/OperationDefinition packages. | Provide API for install/remove/list, hot-reload triggers. | Must |
| PostgreSQL storage strategy | Dynamic per-resource tables plus `{resource}_history` diff records (snapshot + JSON Patch/Merge). | Need migrations, retention policies, and rollback support. | Must |
| OAuth2 RFC 6749 & [OIDC Core](https://openid.net/specs/openid-connect-core-1_0.html) | Support Authorization Code + PKCE and Client Credentials flows with external IdPs. | Map tokens to SMART scopes and AccessPolicy DSL. | Must |
| [SMART on FHIR Scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html) | Implement SMART scope parsing, audience validation, and context. | Provide Admin UI for client registration and scope management. | Must |
| AccessPolicy DSL (internal) | Define, evaluate, and enforce policies centrally across REST/search/operations. | Provide test harness API and enforcement hooks in gateway/server. | Must |
| Config Merge Policy | Merge ENV > DB > FILE with JSON-Schema validation and snapshots/rollback. | Hot-reload partial config changes without downtime. | Must |
| API Gateway `App` resource | Dynamically configure routes and proxy settings. | Support per-route auth, throttling, and capability reflection. | Should |
| Admin UI requirements | Expose package management, policy visualization, config history, hot-reload controls. | Use capability and config metadata endpoints. | Should |
| Infrastructure guidelines | Provide docker-compose services and Justfile commands for developers. | Include local Postgres, IdP stub, Canonical Manager, gateway. | Must |
