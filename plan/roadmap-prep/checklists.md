# Domain Checklists

## fhirschema & Validation
- [ ] Confirm `fhirschema` version compatibility with FHIR R5; document R4B fallbacks.
- [ ] Validate generated Rust models against [`StructureDefinition` snapshots](https://hl7.org/fhir/structuredefinition.html).
- [ ] Ensure JSON serialization matches [FHIR JSON](https://build.fhir.org/json.html) requirements (order, extensions, choice types).
- [ ] Integrate validation errors into [OperationOutcome](https://hl7.org/fhir/operationoutcome.html) responses.

## Canonical Manager & Dynamic Content
- [ ] Support package install/update/remove with dependency resolution.
- [ ] Emit events for search parameter extraction and `$` operation registration.
- [ ] Regenerate [CapabilityStatement](https://build.fhir.org/capabilitystatement.html) after package changes.
- [ ] Provide hot-reload triggers with rollback fallback.

## PostgreSQL Schema & History
- [ ] Generate per-resource tables on demand with canonical-derived column sets.
- [ ] Maintain `{resource}_history` storing snapshot baseline + JSON Patch/Merge diffs per [FHIR history](https://hl7.org/fhir/http.html#history).
- [ ] Implement retention, compaction, and K-snapshot policies.
- [ ] Align migrations with docker-compose lifecycle and Justfile commands.

## Search from StructureDefinition
- [ ] Extract search parameter definitions from `StructureDefinition` and registry ([reference](https://hl7.org/fhir/searchparameter-registry.html)).
- [ ] Map parameters to PostgreSQL indexes (token/string/date/reference).
- [ ] Support modifiers, prefixes, chains, and `_include`/`_revinclude` as per [FHIR Search](https://build.fhir.org/search.html).
- [ ] Expose search capabilities in CapabilityStatement and Admin UI.

## REST Headers & OperationOutcome
- [ ] Enforce HTTP headers per [FHIR REST](https://hl7.org/fhir/http.html) (ETag, Last-Modified, Prefer).
- [ ] Normalize error handling with OperationOutcome codes, severity, and diagnostics.
- [ ] Provide transaction rollback semantics following [FHIR Bundle](https://hl7.org/fhir/bundle.html) rules.
- [ ] Support conditional interactions (`If-Match`, `If-None-Match`).

## API Gateway (`App` Resource)
- [ ] Model dynamic routes with path params and wildcard support.
- [ ] Configure per-route auth, throttling, and logging policies.
- [ ] Integrate AccessPolicy enforcement hooks before proxy dispatch.
- [ ] Surface metrics and health for proxied services.

## OAuth/OIDC & SMART on FHIR
- [ ] Implement Authorization Code + PKCE and Client Credentials per [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749).
- [ ] Validate ID tokens using [OIDC Core](https://openid.net/specs/openid-connect-core-1_0.html).
- [ ] Map SMART scopes ([spec](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html)) to AccessPolicy DSL.
- [ ] Support dynamic client registration and consent logging.

## AccessPolicy DSL & Enforcement
- [ ] Define DSL schema and parser with validation feedback via OperationOutcome.
- [ ] Provide policy evaluation API with test harness endpoints.
- [ ] Enforce policies at REST, search, `$` operations, and gateway ingress.
- [ ] Log policy decisions for audit trails.

## Config Merge & Scoped Hot-Reload
- [ ] Implement merge order ENV > DB > FILE with JSON Schema validation.
- [ ] Capture configuration snapshots with rollback capabilities.
- [ ] Support scoped hot-reload for packages, search parameters, operations, and gateway routes.
- [ ] Document configuration change audit workflow in Admin UI.

## Admin UI
- [ ] Extend UI to manage canonical packages, AccessPolicies, SMART clients, and config snapshots.
- [ ] Visualize CapabilityStatement, active operations, and search parameters.
- [ ] Provide hot-reload controls with status feedback.
- [ ] Integrate authentication/authorization consistent with SMART scopes.
