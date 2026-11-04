# Feature plan

Status: 2026-11-03. Only work still to do; everything here is decided and meant to be
implemented; order is by priority within each section. Finished work is in git history. Sources: the feature research
(`reports/Ресерч фич OctoFHIR — ноябрь 2026.md`), the FML research
(`reports/Ресерч FML и конвертера версий — ноябрь 2026.md`) and
[`sql-on-fhir-plan.md`](sql-on-fhir-plan.md). This file replaces the priorities of
[`competitive/roadmap.md`](competitive/roadmap.md), which predates these decisions.

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

| # | Item | Effort |
|---|---|---|
| 1.1 | Console: ViewDefinition page through `$sql-run`, SQLQuery editor with the banshee LSP, terminology dependencies visible | M |

## 2. FML

The engine is [fml-rs](https://github.com/octofhir/fml-rs); the server has `$transform`,
`$convert`, `QuestionnaireResponse/$extract`, StructureMap checks in validation, and the FML
console page. Left, from the library side: XML input for CDA maps, WASM build, LSP.

## 3. Quick wins (days each)

| # | Item | Notes |
|---|---|---|
| 3.1 | Minimal `$expunge`: one instance and its history, dry-run, admin scope only | later: `_cascade=delete` |
| 3.2 | Bulk Data STU3 additions: `_until`, POST kick-off with Parameters, `organizeOutputBy=Patient`, `allowPartialManifests` | over the existing `$export` |
| 3.3 | Explain in `$validate-code` and the Terminology workbench: which CodeSystem / ValueSet version and package was used | first step to 5.2 |
| 3.4 | Docs page "no `$reindex` needed": functional indexes, index advisor | positioning |
| 3.5 | Run the Inferno Subscriptions test kit and publish the result | `$events`, `$status`, heartbeat exist |
| 3.6 | FHIRPath 3.0 STU (2026-07-28) against fhirpath-rs, close the gaps | in fhirpath-rs |

## 4. Server features (larger)

| # | Item | Effort | Notes |
|---|---|---|---|
| 4.1 | Keyset pagination and paged POST `_search` (next links over 2048 chars) | S–M | replaces deep `_offset` |
| 4.2 | Bulk Data STU4 and `$bulk-submit` | M | CMS-0057-F, 2027-01-01 |
| 4.3 | MCP endpoint in the server: every tool call through SMART scopes and policies, AuditEvent, Provenance tag per the AI Transparency IG | M | not a CRUD wrapper |
| 4.4 | `Patient/$summary` (IPS 2.0.1, EU PS) | M | over `$everything` / `$graph` |
| 4.5 | Conformance evidence: Inferno US Core and Subscriptions in CI, full CapabilityStatement | M–L | Aligned Networks |
| 4.6 | R6 readiness: additional resources, new CapabilityStatement codes, `resource` search parameter type | M | with fhirschema and canonical-manager |
| 4.7 | UDAP / FAST Security STU2 (DCR, B2B, tiered OAuth) | L | TEFCA |
| 4.8 | `$evaluate-measure` on cql-rs | L | unblocked: cql-rs 0.2 reads `valueset`, `include`, `codesystem`, `code`, `concept`, `define function`; the CQL service loads includes from stored Libraries |

## 5. Standalone tools (new repos next to server-rs)

Each is its own repository in the octofhir org, built on our crates, CLI and CI first. The
server picks up what makes sense later.

| # | Tool | Effort | What |
|---|---|---|---|
| 5.1 | **fhir-diff**: semantic diff of packages and profiles with a breaking-change gate | M (MVP 1–2 weeks) | `fhir-diff us.core@6.1.0 us.core@9.0.0 [--profile …]`; Markdown, JSON, SARIF; breaking = min up, max down, new mustSupport, stricter binding, new invariant, removed type, closed slicing; `--fail-on breaking` for CI; diff on FHIR Schema (normalized element tree) via fhirschema + canonical-manager; UX of `banshee lint --breaking`. **MVP done 2026-11-04** (`octofhir/fhir-diff`): effective trees over the whole base chain, 31 rules, text/Markdown/JSON/SARIF, US Core 6.1.0 → 9.0.0 report (71 profiles, 66 breaking), 99% agreement with published snapshots. **Console view done** (Packages → Compare, `POST /api/packages/diff`; versions not installed are read from the registry, never installed). Next: impact on real data (which stored resources become invalid under the new profile version) as a bulk job, not a request: on large data it is a full validation pass; R4→R6 per resource; value set expansion diff |
| 5.2 | **octofhir-tx**: offline terminology for CI, with explain for "code not in ValueSet" | L (MVP M) | one binary on packages; pinned CodeSystem / ValueSet versions; explain trace |
| 5.3 | **Validator CLI and GitHub Action** with a public parity report, plus **validator-diff** (why two validations disagree) | M + S–M | fhirschema, targetProfile, QuestionnaireResponse, offline |
| 5.4 | **FHIR Toolbox MCP** (offline): find profiles and extensions, validate against an IG, evaluate FHIRPath, run views | M | standalone counterpart of 4.3 |
| 5.5 | **`@octofhir/wasm`**: validator, FHIRPath, UCUM, SoF and FML in the browser | M–L | |
| 5.6 | Example generator by profile, valid by construction, plus negative mutants per constraint | L | after 5.2 |
| 5.7 | capcheck: check a CapabilityStatement against the server's real behavior | M | |
| 5.8 | Debug trace for FHIRPath / CQL in the proposed cross-engine format | S–M | FML already has its trace |
| 5.9 | **ccda-fhir**: C-CDA ⇄ FHIR in both directions on FML | L (first stage ~4 months) | decided 2026-11-04 (`reports/Ресерч C-CDA ⇄ FHIR — 2026-11-04.md`, variant A): FML maps on fml-rs over the HL7 CDA logical models (`hl7.cda.uv.core`, `hl7.cda.us.ccda`), XML read/write as a new `fml-xml` crate; a pair of maps per domain (CDA→FHIR, FHIR→CDA) over shared data type maps, round-trip both ways in CI on HL7 C-CDA-Examples. First stage: header + Allergies, Problems, Medications, Immunizations, Results, Vital Signs, Procedures, both directions, R4 / US Core 6.1.0. Repo `octofhir/ccda-fhir` (maps, ConceptMaps, corpus, metrics, CLI); server `$ccda-to-fhir` / `$fhir-to-ccda` over `Binary`, API stays JSON-only |

## Not doing

- `$reindex` (not needed architecturally), multi-tenancy and horizontal scaling, FHIR XML.
- Shareable HTML export of notebooks, charts in SQL cells, materialized views of views
  (only with our own incremental-by-`txid` design, if ever).
- Further investment in the FHIRPath LSP / VS Code extension and a FSH LSP (maintain only).
- Traffic proxy / inspector; Da Vinci payer pack (maybe `$member-match` later).
- Synthea-style clinical simulation; SDC form renderer or builder.
