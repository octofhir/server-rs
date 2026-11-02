# Feature plan

Status: 2026-11-02. Everything here is decided and meant to be implemented; order is by
priority within each section. Sources: the feature research
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

Multi-year priority. Detailed plan, state and phases: [`sql-on-fhir-plan.md`](sql-on-fhir-plan.md).

| # | Item | Effort | State |
|---|---|---|---|
| 1.1 | Views as SQL over the server's data, `$viewdefinition-run`, `$viewdefinition-export` (phase 1) | — | done (50a4fe9) |
| 1.2 | `$sql-run`, SQLQuery / SQLView, sandbox (phases 2–3), `_format=fhir` | — | done (50a4fe9) |
| 1.3 | `memberOf()` in `$run` and the sof CLI: pass the server's `TerminologyProvider` into the evaluation context | S | todo |
| 1.4 | Terminology relations: ValueSet expansion and ConceptMap rows per job, with provenance (phase 4) | M | todo |
| 1.5 | `$sql-export`: async jobs, manifest, several subjects (phase 5) | M | todo |
| 1.6 | v3 canonicals (`org.sql-on-fhir.ig` and `hl7.fhir.uv.sql-on-fhir`), R6 ViewDefinition (phase 6) | S–M | todo |
| 1.7 | Parquet output, `Accept` format negotiation | S | todo |
| 1.8 | banshee 0.2.5 in the `$sql-run` guard, so `AS t(a int)` is accepted | S | parser in (9cead17); add record functions (`jsonb_to_recordset`, …) to the allowlist |
| 1.9 | Console: ViewDefinition page through `$sql-run`, SQLQuery editor with the banshee LSP (phase 7) | M | todo |
| 1.10 | DuckDB as an embedded `SqlExecutor` in sof (dialect exists) | M | todo |

## 2. FML and version conversion in the server

The engine is done and released: [fml-rs](https://github.com/octofhir/fml-rs) 0.1.0 (parse,
format, type-check, compile identical to the HL7 reference on 1,659 maps, run, debug trace;
converts 2,863 of the 2,881 R4 examples that have a map to R5). The research advised waiting
for R6; we built it anyway, so the server side is next.

| # | Item | Effort | Notes |
|---|---|---|---|
| 2.1 | `StructureMap/$transform` and `StructureMap/[id]/$transform` | M | `source`, `content`, `supportingMap`; maps resolved from the database and installed packages through canonical-manager (`MapResolver`); `translate()` through the server's ConceptMaps; `debug=true` returns the trace Parameters (`fml_engine::debug_parameters`) |
| 2.2 | `$convert` between FHIR versions | M | HL7 cross-version maps (R4↔R5, R5↔R6 as they mature); target version from `Accept` `fhirVersion`; values that did not carry over reported in an OperationOutcome next to the result (`Transformed.warnings`) |
| 2.3 | `Questionnaire/$extract` on StructureMap (`questionnaire-targetStructureMap`) | S | thin wrapper over 2.1; headless only, no form renderer (SDC stays deferred) |
| 2.4 | Console: FML tab with check, run, trace view and the loss report | M | not a visual map editor |
| 2.5 | Validate StructureMaps on write with `fml-check` (issues as OperationOutcome warnings) | S | definitions from the server's packages |

Later, from the FML library side: XML input for CDA maps, WASM build, LSP.

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
| 5.1 | **fhir-diff**: semantic diff of packages and profiles with a breaking-change gate | M (MVP 1–2 weeks) | `fhir-diff us.core@6.1.0 us.core@9.0.0 [--profile …]`; Markdown, JSON, SARIF; breaking = min up, max down, new mustSupport, stricter binding, new invariant, removed type, closed slicing; `--fail-on breaking` for CI; diff on FHIR Schema (normalized element tree) via fhirschema + canonical-manager; UX of `banshee lint --breaking`. Next: impact on real data (how many of your resources become invalid), R4→R6 per resource, console view. First demand check: publish the US Core 6.1 → 9 report. A CLI and CI gate, not a web UI |
| 5.2 | **octofhir-tx**: offline terminology for CI, with explain for "code not in ValueSet" | L (MVP M) | one binary on packages; pinned CodeSystem / ValueSet versions; explain trace |
| 5.3 | **Validator CLI and GitHub Action** with a public parity report, plus **validator-diff** (why two validations disagree) | M + S–M | fhirschema, targetProfile, QuestionnaireResponse, offline |
| 5.4 | **FHIR Toolbox MCP** (offline): find profiles and extensions, validate against an IG, evaluate FHIRPath, run views | M | standalone counterpart of 4.3 |
| 5.5 | **`@octofhir/wasm`**: validator, FHIRPath, UCUM, SoF and FML in the browser | M–L | |
| 5.6 | Example generator by profile, valid by construction, plus negative mutants per constraint | L | after 5.2 |
| 5.7 | capcheck: check a CapabilityStatement against the server's real behavior | M | |
| 5.8 | Debug trace for FHIRPath / CQL in the proposed cross-engine format | S–M | FML already has its trace |

## Done

- 2026-11-01: fml-rs 0.1.0 (engine, CLI, binaries for Linux, macOS, Windows); canonical-manager
  0.2.6 (registry fixes) and 0.2.7 (package folders laid out from the content store);
  fhirschema 0.3.33 (R6 ballot5); fhirpath-rs 0.5.1; ucum 0.9.2.
- SQL on FHIR phases 1–3, pushed (server 50a4fe9; sof 0.2.0 / 0.2.1).
- 2026-11-01/02: cql-rs 0.2.0 / 0.2.1 (libraries end to end, real `cql` CLI with release
  binaries, cql-tests 1693/1693); banshee 0.2.5; server on the published crates with no
  `[patch.crates-io]`; JWT keys on aws-lc-rs (no `rsa` crate); `cargo audit` clean except
  rkyv 0.7 (lockfile only); image `ghcr.io/octofhir/octofhir-server:41d1075`.

## Not doing

- `$reindex` (not needed architecturally), multi-tenancy and horizontal scaling, FHIR XML.
- Shareable HTML export of notebooks, charts in SQL cells, materialized views of views
  (only with our own incremental-by-`txid` design, if ever).
- Further investment in the FHIRPath LSP / VS Code extension and a FSH LSP (maintain only).
- Traffic proxy / inspector; Da Vinci payer pack (maybe `$member-match` later).
- Synthea-style clinical simulation; SDC form renderer or builder.
