# Proof of Concept Criteria

## Diff History Strategy
- **Metric**: Time to persist `Bundle` transaction with history entries; storage footprint vs snapshot-only.
- **Input**: Sample `Patient`/`Observation` create/update bundles using [FHIR Bundle transactions](https://hl7.org/fhir/bundle.html).
- **Success Criteria**:
  - History table records snapshot-K baseline plus JSON Patch & MergePatch deltas.
  - Retrieval via `_history` meets [FHIR REST history](https://hl7.org/fhir/http.html#history) format.
  - Rollback simulation restores resource via `OperationOutcome`-based confirmation.

## Index Planning
- **Metric**: Query latency and plan cost for representative search parameters (token/string/date/reference).
- **Input**: Search definitions extracted from `StructureDefinition` using Canonical Manager.
- **Success Criteria**:
  - Generated DDL aligns with `fhirschema` types and [FHIR Search](https://build.fhir.org/search.html) expectations.
  - Index coverage validated using PostgreSQL `EXPLAIN` on key queries.
  - Document trade-offs for composite/multi-column indexes.

## Hot-Reload Mechanics
- **Metric**: Downtime duration during package/config reload; consistency of in-flight requests.
- **Input**: Canonical package update triggering new `$` operation and search parameter.
- **Success Criteria**:
  - Immutable snapshot stored before reload; atomic swap ensures no partial state.
  - CapabilityStatement regenerated post-reload referencing [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html).
  - Config rollback reinstates prior state through ENV > DB > FILE merge policy.

## Routing Flexibility
- **Metric**: Route propagation latency and correctness of path parameter resolution.
- **Input**: Admin-defined `App` resource with wildcard and parameterized paths.
- **Success Criteria**:
  - Gateway updates route table without restart; AccessPolicy enforcement engaged before proxy.
  - SMART scopes validated per [SMART App Launch](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html).
  - Errors surfaced via [OperationOutcome](https://hl7.org/fhir/operationoutcome.html) on invalid routes or policies.
