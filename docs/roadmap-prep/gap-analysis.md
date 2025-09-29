# GAP Analysis

| Requirement | Status | Closure Options | Risks / Dependencies |
| --- | --- | --- | --- |
| RESTful CRUD & history per [FHIR REST](https://hl7.org/fhir/http.html) | Missing | Implement async handlers with OperationOutcome error mapping; integrate diff history persistence. | Depends on PostgreSQL schema generation and AccessPolicy enforcement readiness. |
| Transaction bundles per [FHIR Bundle](https://hl7.org/fhir/bundle.html) | Missing | Add atomic transaction executor coordinating Canonical Manager validation. | Requires stable diff history storage and rollback semantics. |
| `fhirschema`-based validation | Partial | Upgrade to R5 artifacts, wire validation pipeline pre-storage. | Canonical package completeness; performance impact on high-volume writes. |
| Search parameter extraction per [FHIR Search](https://build.fhir.org/search.html) | Missing | Generate search config from StructureDefinition, plan indexes. | Dependent on Canonical Manager metadata ingestion and PostgreSQL adapter. |
| CapabilityStatement generation | Missing | Build composer reacting to package and config hot-reload events. | Requires metadata from all subsystems; risk of drift without tests. |
| OAuth/OIDC SMART support | Missing | Integrate external IdP connectors, SMART scopes, and AccessPolicy mapping. | External IdP availability; consent workflows; compliance review. |
| AccessPolicy DSL enforcement | Partial | Extend enforcement hooks across REST/search/operations/gateway. | Complexity of centralized decision caching; risk of performance regression. |
| API Gateway App routing | Missing | Implement dynamic routing registry, path param parsing, policy hooks. | Depends on hot-reload infrastructure and AccessPolicy evaluation latency. |
| Config merge ENV>DB>FILE with hot-reload | Missing | Build configuration service with JSON Schema validation and snapshot store. | Coordination with Admin UI; risk of partial reload causing inconsistency. |
| Admin UI extensions | Missing | Add modules for packages, policies, SMART clients, config snapshots. | Requires secure APIs and OAuth integration; UX resources. |
| Infrastructure docker-compose & Just automation | Partial | Add Postgres, IdP stub, Canonical Manager, gateway services; document commands. | Resource usage; ensuring parity with production architecture. |
