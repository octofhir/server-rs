# Task 0008 — OAuth/OIDC & SMART Integration

## Objective
Achieve Stage E milestones from [Roadmap v1](../docs/ROADMAP.md) by integrating external IdP support, implementing OAuth 2.0 Authorization Code + PKCE and Client Credentials flows, and mapping SMART on FHIR scopes to AccessPolicy attributes.

## Context & Rationale
- **Roadmap Alignment**: Essential for security compliance and SMART application onboarding.
- **Gap Closure**: Resolves identity and authorization gaps identified in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Requires Gateway hooks from Task 0007 and policy evaluation from Task 0009; coordinates with Stage J infrastructure for IdP stubs.

## Deliverables
1. OAuth/OIDC provider integration supporting external IdPs per [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749) and [OIDC Core](https://openid.net/specs/openid-connect-core-1_0.html).
2. Token endpoint implementing Authorization Code + PKCE and Client Credentials, issuing SMART-compliant scopes from [SMART App Launch](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html).
3. Token introspection and revocation endpoints with Admin UI hooks.
4. Documentation describing security flows, consent, and linkage to AccessPolicy DSL.

## Acceptance Criteria (DoD)
- Access tokens validated on REST/search/operations requests with policy enforcement.
- SMART launch context recorded for audit purposes.
- OperationOutcome responses returned for invalid grants, scope violations, or token errors per [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html).

## Implementation Outline
- Integrate OAuth library or implement endpoints using async-first design; store client metadata via Admin UI.
- Map SMART scopes to policy attributes and enforce during request handling.
- Provide configuration toggles via Stage H merge engine for IdP endpoints and credentials.

## Risks & Mitigations
- **Security vulnerabilities**: Conduct threat modeling, ensure PKCE enforcement, and log all auth events.
- **External IdP variability**: Support JWKS refresh and configurable endpoints.
- **Scope mapping complexity**: Create unit tests verifying SMART scope to policy translation.

## References
- [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749)
- [OIDC Core](https://openid.net/specs/openid-connect-core-1_0.html)
- [SMART on FHIR Scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html)
- [FHIR OperationOutcome](https://hl7.org/fhir/operationoutcome.html)
