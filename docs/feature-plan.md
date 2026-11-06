# Feature plan

Status: 2026-11-05. Only work still to do; everything here is decided and meant to be
implemented; order is by priority within each section. Finished work is in git history. Sources: the feature research
(`reports/Ресерч фич OctoFHIR — ноябрь 2026.md`), the FML research
(`reports/Ресерч FML и конвертера версий — ноябрь 2026.md`) and
[`sql-on-fhir-plan.md`](sql-on-fhir-plan.md).

Ground rules that apply to every item:

- Single instance first: no multi-tenancy, Redis or multi-replica work for now.
- JSON only: no FHIR XML, ever.
- Our own naming, design and feature shape. Lean on what sets us apart: offline, CLI, WASM,
  explain, public conformance numbers.
- Verify against the spec text. Bugs in our own crates (fhirpath-rs, fhirschema,
  canonical-manager, sof, fml-rs, banshee, ucum-rs, cql-rs) are fixed there and released.

Effort: S up to a week, M 2–4 weeks, L 1–2 months, XL more.

## 1. Strategic: SQL on FHIR

Multi-year priority. Remaining work and the spec baseline: [`sql-on-fhir-plan.md`](sql-on-fhir-plan.md).
The console (Views page on `$sql-run`, SQLQuery editing with the banshee LSP) is done.

## 2. FML

The engine is [fml-rs](https://github.com/octofhir/fml-rs); the server has `$transform`,
`$convert`, `QuestionnaireResponse/$extract`, StructureMap checks in validation, and the FML
console page (CDA input with the trace on the XML). Left, from the library side: WASM build, LSP.

## 3. Quick wins (days each)

| # | Item | Notes |
|---|---|---|
| 3.5 | Run the Inferno Subscriptions test kit and publish the result | `$events`, `$status`, heartbeat exist |

## 4. Server features (larger)

| # | Item | Effort | Notes |
|---|---|---|---|
| 4.5 | Conformance evidence: Inferno US Core and Subscriptions run locally (`just inferno-*`) before releases, results published; full CapabilityStatement | M | not in CI: long runs on the org's public runners |
| 4.7 | UDAP Tiered OAuth (`udap_to`), CRL distribution points and OCSP | M | TEFCA; registration, JWT authentication and B2B are done |
| 4.8 | Measures: supplemental data, composite scoring | S | scoring of all four types, ratios with observations, `subjectGroup`, `evaluatedResource` and `$data-requirements` with `where` filters are done |

## 5. Standalone tools (new repos next to server-rs)

Each is its own repository in the octofhir org, built on our crates, CLI and CI first. The
server picks up what makes sense later.

| # | Tool | Effort | What |
|---|---|---|---|
| 5.1 | **fhir-diff**: semantic diff of packages and profiles with a breaking-change gate | M (MVP 1–2 weeks) | `fhir-diff us.core@6.1.0 us.core@9.0.0 [--profile …]`; Markdown, JSON, SARIF; breaking = min up, max down, new mustSupport, stricter binding, new invariant, removed type, closed slicing; `--fail-on breaking` for CI; diff on FHIR Schema (normalized element tree) via fhirschema + canonical-manager; UX of `banshee lint --breaking`. **MVP done 2026-11-04** (`octofhir/fhir-diff`): effective trees over the whole base chain, 31 rules, text/Markdown/JSON/SARIF, US Core 6.1.0 → 9.0.0 report (70 profiles, 64 breaking), 99% agreement with published snapshots. **Console view done** (Packages → Compare, `POST /api/packages/diff`; versions not installed are read from the registry, never installed). Next: impact on real data (which stored resources become invalid under the new profile version) as a bulk job, not a request: on large data it is a full validation pass; R4→R6 per resource; value set expansion diff |
| 5.4 | **FHIR Toolbox MCP** (offline): find profiles and extensions, validate against an IG, evaluate FHIRPath, run views | M | standalone counterpart of 4.3 |
| 5.5 | **`@octofhir/wasm`**: validator, FHIRPath, UCUM, SoF and FML in the browser | M–L | |
| 5.6 | Example generator by profile, valid by construction, plus negative mutants per constraint | L |
| 5.8 | Debug trace for FHIRPath / CQL in the proposed cross-engine format | S–M | FML already has its trace |
| 5.9 | **ccda-fhir**: C-CDA ⇄ FHIR in both directions on FML | L (first stage ~4 months) | decided 2026-11-04 (`reports/Ресерч C-CDA ⇄ FHIR — 2026-11-04.md`, variant A): FML maps on fml-rs over the HL7 CDA logical models (`hl7.cda.uv.core`, `hl7.cda.us.ccda`), XML read/write as a new `fml-xml` crate; a pair of maps per domain (CDA→FHIR, FHIR→CDA) over shared data type maps, round-trip both ways in CI on HL7 C-CDA-Examples. First stage: header + Allergies, Problems, Medications, Immunizations, Results, Vital Signs, Procedures, both directions, R4 / US Core 6.1.0. Repo `octofhir/ccda-fhir` (maps, ConceptMaps, corpus, metrics, CLI); server `$ccda-to-fhir` / `$fhir-to-ccda` over `Binary`, API stays JSON-only. **First stage done 2026-11-05**: `fml-xml` in fml-rs (read/write by the logical model, positions, `fml run` on XML), templates in fml-check/fml-engine (`hasTemplateIdOf()`, group choice by templateId, R5 models next to R4 maps); `octofhir/ccda-fhir` 0.1.0 with 21 maps, 44 ConceptMaps, CLI `to-fhir|to-cda|check|coverage`, corpus metrics in CI (506 documents: entries recognised 55%, US Core valid 97%, C-CDA conformance 90% of documents, FHIR→CDA→FHIR 97%, CDA→FHIR→CDA 68%); server operations + FML console CDA input with the trace on the XML. **ccda-fhir 0.1.1 (2026-11-05)**: encounters, social history, notes, goals and plan of treatment, functional and mental status, family history, devices (UDI), health concerns, admission and discharge diagnoses and medications, the missing rows of the C-CDA on FHIR tables (Medication, specimens, participations as PractitionerRole); corpus: sections 95.4%, entries 94.8%, US Core valid 99.7%, C-CDA conformance 92.2%, FHIR→CDA→FHIR 98.7%, CDA→FHIR→CDA 73.1%; `docs/writing-maps.md` explains the maps. Next: Provenance for authors (the mapping gives no definitive guidance), health status / intervention / outcome observations, slices and terminology in the C-CDA conformance check |

## Not doing

- `$reindex` (not needed architecturally), multi-tenancy and horizontal scaling, FHIR XML.
- Shareable HTML export of notebooks, charts in SQL cells, materialized views of views
  (only with our own incremental-by-`txid` design, if ever).
- Further investment in the FHIRPath LSP / VS Code extension and a FSH LSP (maintain only).
- Traffic proxy / inspector; Da Vinci payer pack (maybe `$member-match` later).
- Synthea-style clinical simulation; SDC form renderer or builder.
- A separate validator CLI / GitHub Action: fhirschema is the validator and has its CLI.
- An offline terminology server (octofhir-tx).
