# ADR-0001: Conformance Testing Strategy

## Status

Proposed

## Date

2026-02-14

## Context

No competitor comparison is credible without conformance test results. Every FHIR server that claims production readiness publishes conformance test results:

- **HAPI FHIR**: Touchstone test results
- **Smile CDR**: Published conformance reports
- **Firely Server**: g10 certified
- **AWS HealthLake**: Validated via ONC Inferno test suite (US Core IG STU v7.0.0, SMART App Launch v2.0.0)
- **Medplum**: Inferno test results
- **Aidbox**: Published conformance documentation

OctoFHIR currently has **zero published conformance test results**. This is the single biggest credibility gap for a FHIR server. Without conformance evidence, potential adopters cannot evaluate whether OctoFHIR correctly implements the FHIR specification.

## Decision

**Adopt Inferno R4 test suite as the primary conformance validation tool.**

Supplement with focused internal integration tests for features Inferno does not cover (GraphQL, SQL on FHIR, CQL, subscriptions, automations).

## Options Considered

### Option 1: Inferno (Chosen)

**Inferno** is the HL7/ONC official testing tool, maintained by the MITRE Corporation.

| Aspect | Details |
|--------|---------|
| Authority | HL7/ONC official; used for g10 certification |
| Cost | Free and open source (Apache 2.0) |
| Coverage | FHIR R4 core interactions, US Core IG, SMART on FHIR, Bulk Data |
| Path to g10 | Direct path — same tool used for ONC g10 certification |
| Community | Active development, regular updates |
| Deployment | Ruby application, Docker image available |
| Automation | REST API for CI integration |

**Pros**:
- Free, authoritative, widely recognized
- Direct path to ONC g10 certification (P2 roadmap item)
- REST API enables CI/CD integration
- Same tool used by AWS HealthLake, Medplum, and other certified servers

**Cons**:
- Primarily focused on US Core IG (less coverage of international profiles)
- Ruby runtime dependency for test execution
- Test suite updates may introduce new failures

### Option 2: Touchstone (Rejected for Now)

**Touchstone** by AEGIS.net is a commercial conformance testing platform.

| Aspect | Details |
|--------|---------|
| Authority | HL7 endorsed |
| Cost | Commercial license required ($$$) |
| Coverage | Broadest test coverage (FHIR core + many IGs) |
| Community | Used by HAPI FHIR, IBM FHIR |

**Pros**:
- Broadest test coverage across FHIR versions and IGs
- Detailed test reports with HL7 branding

**Cons**:
- Commercial license cost
- No direct path to g10 certification
- Overkill for initial conformance validation

**Decision**: Consider Touchstone as a supplement after Inferno pass rate >90%.

### Option 3: Custom Test Suite (Rejected as Primary)

Build our own conformance test suite using Rust integration tests.

| Aspect | Details |
|--------|---------|
| Authority | Self-attested only |
| Cost | Engineering time (L-XL effort) |
| Coverage | Full control over what's tested |
| Community | None |

**Pros**:
- Full control over test scenarios
- Native Rust, no external runtime
- Can test OctoFHIR-specific features (GraphQL, SQL on FHIR, CQL)

**Cons**:
- Self-attested results carry no external credibility
- Massive effort to replicate Inferno/Touchstone coverage
- No path to certification

**Decision**: Use for supplementary testing of features Inferno doesn't cover.

## Implementation Plan

### Phase 1: Initial Run (Week 1-2)

1. Deploy Inferno via Docker alongside OctoFHIR
2. Configure Inferno to point at OctoFHIR's FHIR endpoint
3. Run R4 core test suite
4. Catalog all failures in a tracking issue
5. Categorize failures: (a) missing features, (b) incorrect behavior, (c) response format issues

### Phase 2: Fix & Iterate (Week 2-4)

1. Fix conformance failures by category priority:
   - Response format issues (low effort, high pass rate impact)
   - Incorrect behavior (medium effort, correctness)
   - Missing features (varies, may require new implementation)
2. Re-run after each batch of fixes
3. Track pass rate progression

### Phase 3: CI Integration (Week 4-5)

1. Add Inferno to CI pipeline (GitHub Actions)
2. Run on every PR merge to `main`
3. Fail CI if pass rate drops below threshold
4. Publish results badge in README

### Phase 4: Publish Results (Week 5-6)

1. Add conformance results page to documentation
2. Include pass rate, known failures, and roadmap for remaining fixes
3. Update competitive analysis with OctoFHIR conformance data

### Supplementary Testing (Ongoing)

Internal Rust integration tests for features Inferno doesn't cover:

| Feature | Test Approach |
|---------|--------------|
| GraphQL | `octofhir-graphql` crate tests + API-level tests |
| SQL on FHIR | ViewDefinition execution tests |
| CQL | CQL expression evaluation tests |
| Subscriptions | End-to-end subscription delivery tests |
| Automations | Trigger → execution → result tests |
| Admin API | User/client/policy CRUD tests |
| Cache invalidation | Multi-instance cache sync tests |

## Consequences

### Positive

- Establishes credibility for OctoFHIR conformance claims
- Direct path to ONC g10 certification (P2 roadmap)
- CI integration prevents conformance regressions
- Forces fixing edge cases in FHIR implementation

### Negative

- Estimated 2-4 weeks of focused engineering effort to reach >90% pass rate
- Inferno updates may introduce new failures requiring ongoing maintenance
- Ruby dependency in CI pipeline (mitigated by Docker)

### Risks

| Risk | Mitigation |
|------|-----------|
| Pass rate <50% initially | Expected; focus on quick wins (format, headers, error responses) first |
| Inferno test suite changes | Pin Inferno version in CI; upgrade deliberately |
| Features missing entirely | Track as roadmap items, not conformance failures |

## References

- [Inferno Framework](https://inferno-framework.github.io/inferno-core/)
- [ONC g10 Certification Program](https://www.healthit.gov/topic/certification-ehrs/about-onc-health-it-certification-program)
- [Touchstone by AEGIS](https://touchstone.aegis.net/)
- [FHIR R4 Specification](https://hl7.org/fhir/R4/)
