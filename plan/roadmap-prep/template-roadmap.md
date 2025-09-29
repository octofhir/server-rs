# Roadmap Template

## 1. Vision & Scope
- _Placeholder for summary of objectives, referencing key specs._

## 2. Workstreams
### 2.1 Base Server & Storage
- _Describe milestones, dependencies, references to [FHIR REST](https://hl7.org/fhir/http.html)._ 
- **DoD**: _Define acceptance criteria for CRUD, history, OperationOutcome._

### 2.2 Search & Indexing
- _Outline tasks for StructureDefinition extraction, PostgreSQL indexes._
- **DoD**: _Parameters supported, performance baselines, CapabilityStatement updates._

### 2.3 Transactions & Bundles
- _Plan for atomic transactions per [FHIR Bundle](https://hl7.org/fhir/bundle.html)._ 
- **DoD**: _Transaction rollback, diff history integration._

### 2.4 API Gateway & App Resource
- _Detail dynamic routing, proxy behavior, policy hooks._
- **DoD**: _Hot-reload propagation, AccessPolicy coverage, monitoring._

### 2.5 OAuth/OIDC & SMART
- _Summarize auth flows referencing [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749) and [SMART scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html)._ 
- **DoD**: _Token validation, scope enforcement, audit trails._

### 2.6 AccessPolicy DSL & Enforcement
- _Describe DSL evolution and enforcement integration._
- **DoD**: _Policy lifecycle, evaluation APIs, logging._

### 2.7 Operations ($)
- _Plan for Canonical Manager operations per [FHIR Operations](https://build.fhir.org/operations.html)._ 
- **DoD**: _Registration workflow, capability exposure, test coverage._

### 2.8 Configuration & Hot-Reload
- _Outline config merge strategy and runtime reload process._
- **DoD**: _ENV>DB>FILE merge verified, snapshot/rollback tests._

### 2.9 Admin UI Enhancements
- _Identify UI modules and data sources._
- **DoD**: _User flows for packages, policies, SMART clients, config history._

### 2.10 Infrastructure & Tooling
- _Plan docker-compose, Justfile, CI updates._
- **DoD**: _Environment parity, automated setup, health checks._

## 3. Dependencies & Risks
- _List key cross-team dependencies and mitigations._

## 4. Testing & Quality Strategy
- _Summarize approach to unit/integration tests, including OperationOutcome validation and SMART security._

## 5. Open Questions
- _Track unknowns pending ADR finalization or external inputs._
