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
| 2.1 | App subscriptions that fire: `AppSubscription` triggers (type, event, FHIRPath) to their webhook channel and to notifications | M | reconcile stores them; nothing dispatches them yet. Notifications themselves (`$send`, delivery) work |
| 2.2 | Inferno Subscriptions test kit, run locally and published | S | `$events`, `$status`, heartbeat exist |
| 2.3 | Conformance evidence: Inferno US Core and Subscriptions before releases (`just inferno-*`, not CI), full CapabilityStatement | M | |
| 2.4 | UDAP Tiered OAuth (`udap_to`), CRL distribution points and OCSP | M | |

## 3. Tools (own repos)

| # | Tool | Next |
|---|---|---|
| 3.1 | fhir-diff | Impact on stored data as a bulk job; R4→R6 per resource; value set expansion diff |
| 3.3 | `@octofhir/wasm` | Publish to npm (release workflow ready, needs `NPM_TOKEN` and the `@octofhir` scope); one FHIR version per build for a smaller module |
| 3.4 | ccda-fhir | Provenance for authors; health status, intervention and outcome observations; slices and terminology in the C-CDA conformance check |
| 3.5 | fml-rs | LSP |
