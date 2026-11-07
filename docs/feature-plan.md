# Feature plan

Status: 2026-11-07. Only open work, by priority within each section. Finished work is in git
history and the docs.

Ground rules:

- Single instance first: no multi-tenancy, Redis or multi-replica work for now.
- JSON only: no FHIR XML.
- Verify against the spec text. Bugs in our own crates are fixed there and released.

Effort: S up to a week, M 2–4 weeks, L 1–2 months.

## 1. SQL on FHIR

Multi-year priority; nothing open right now. Spec baseline: [`sql-on-fhir-plan.md`](sql-on-fhir-plan.md).

## 2. Server

| # | Item | Effort | Notes |
|---|---|---|---|
| 2.3 | Conformance evidence before releases (`just inferno-*`, not CI, SQL on FHIR off): rerun US Core and Bulk Data on the current build | S | Subscriptions: 18 of 18. Results in `conformance.mdx` |
| 2.4 | UDAP Tiered OAuth (`udap_to`), CRL distribution points and OCSP | M | |

## 3. Tools (own repos)

| # | Tool | Next |
|---|---|---|
| 3.1 | fhir-diff | Impact on stored data as a bulk job; R4→R6 per resource; value set expansion diff |
| 3.4 | ccda-fhir | Provenance for authors; health status, intervention and outcome observations; slices and terminology in the C-CDA conformance check |
