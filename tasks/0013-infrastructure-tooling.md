# Task 0013 — Infrastructure & Tooling Automation

## Objective
Deliver Stage J milestones from [Roadmap v1](../docs/ROADMAP.md) by updating `docker-compose`, enriching the `Justfile`, and defining CI workflows that cover linting, testing, and security checks for the full stack.

## Context & Rationale
- **Roadmap Alignment**: Ensures reproducible environments and automated verification to support feature delivery.
- **Gap Closure**: Closes infrastructure automation gaps captured in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md).
- **Dependencies**: Requires prior tasks for application functionality to hook into the composed environment and test flows.

## Deliverables
1. `docker-compose` stack including PostgreSQL, Canonical Manager, API Gateway, OAuth provider stub, and Admin UI.
2. `Justfile` commands covering build/run/test, migrations, policy evaluation, and hot-reload workflows.
3. CI pipeline specification executing `just lint`, `just test`, security scans, and packaging checks.
4. Documentation guiding developers through environment setup and referencing relevant FHIR specs for payload validation.

## Acceptance Criteria (DoD)
- Local stack starts successfully with seeded data and exposes required services for browser-based Admin UI validation.
- CI pipeline gates merges with lint/test/security status badges.
- Documentation links to [FHIR JSON](https://build.fhir.org/json.html), [FHIR REST](https://hl7.org/fhir/http.html), and [SMART on FHIR](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html) expectations for QA.

## Implementation Outline
- Extend docker-compose definitions with health checks and environment variable wiring.
- Update `Justfile` to wrap docker-compose actions and developer tooling commands.
- Author CI configuration (e.g., GitHub Actions) orchestrating tests, lint, and security scans.

## Risks & Mitigations
- **Environment drift**: Version-lock container images and document update cadence.
- **Resource usage**: Provide lightweight profiles for CI vs local development.
- **Security scanning gaps**: Integrate SBOM and dependency audit steps.

## References
- [FHIR REST](https://hl7.org/fhir/http.html)
- [FHIR JSON](https://build.fhir.org/json.html)
- [SMART on FHIR Scopes](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html)
