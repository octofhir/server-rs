# SQL on FHIR in the server — plan

Status: 2026-11-01. Priority: strategic (multi-year). Quality bar: the server path
over its own PostgreSQL is the product; the CLI / in-memory paths follow it.

## Spec baseline (verify against these, not memory)

| Source | What it fixes |
|---|---|
| SoF 2.0.0 / 2.1.0-pre (`org.sql-on-fhir.ig`) | ViewDefinition, `$viewdefinition-run`, `$viewdefinition-export`, FHIRPath subset (`getResourceKey`, `getReferenceKey` required; Shareable subset SHOULD) |
| SoF 3.0.0-ballot (2026-08-14, `hl7.fhir.uv.sql-on-fhir`, based on R6) — http://hl7.org/fhir/uv/sql-on-fhir/3.0.0-202609-ballot | New canonical base `http://hl7.org/fhir/uv/sql-on-fhir`; ViewDefinition as R6 additional resource; `identifier` repeats; one of forEach/forEachOrNull/repeat per select; `SQLQuery` / `SQLView` (Library profiles, `relatedArtifact` depends-on + `label` = table name); system operations `$sql-run` (sync) and `$sql-export` (async) over views **and** queries |
| SoF CI build (post-ballot) — https://build.fhir.org/ig/HL7/sql-on-fhir/en/terminology-in-sql.html (trial-use) | Terminology in SQL: a SQLQuery/SQLView declares a ValueSet / ConceptMap as a `depends-on` dependency; the runner exposes it as a relation. ValueSet → `system, version, code, display, inactive` (unique on first three, abstract entries excluded, nested entries flattened). ConceptMap → 9 columns `source_system … relationship`, noMap rows kept with null target, `dependsOn`/`product`/`valueSet`/source-less groups rejected, `unmapped` never applied. One resolution per job, before any SQL; unresolvable → 404, undeterminable content → 422. "The SQL never names a canonical URL or a terminology server." The ViewDefinition FHIRPath subset is unchanged. |

`memberOf()` in a ViewDefinition path is outside the required and shareable
subsets: we keep it as an extension of our runners (in-memory, needs a
TerminologyProvider), never as the way terminology reaches SQL.

## Current state (2026-11-01, phase 1 done)

- **Views over the server's data run as SQL** on its PostgreSQL: the
  generated SELECT reads `WITH sof_source AS (<authorized search>)` — the same
  admission, grants and patient membership as `search` (`plan_search` shared by
  both). A per-resource policy, or a path outside the SQL lowering, falls back
  to the in-memory engine over `search`.
- `$viewdefinition-run` (system / type / instance) as the IG defines it:
  `viewReference` | `viewResource`, `resource` (inline, in memory), `patient`,
  `group`, `_since` (Common Operation Behavior filters, Patient compartment),
  `_limit`, `_format` ndjson (default) / json / csv, `header`; the rows are
  returned raw in the format's media type; unresolvable patient/group → 400;
  `source` not supported. Registered in code, independent of the IG download.
- `ViewDefinition/$run` (console) and `$viewdefinition-export` use the same
  engine; export pages by source id with authorization per page.
- sof: undeclared column types keep their JSON type on every engine; rows keep
  column order; SQL lowering covers, beyond the required subset, `upper`,
  `lower`, `length`, `trim`, `startsWith`, `endsWith`, `contains`, `indexOf`,
  `substring`, `replace`, `iif`, `select`, `all`, `skip`, `take`, `tail`,
  `distinct`, `abs` (integer), `ceiling`/`floor`/`truncate` (typed), and `~`
  per FHIRPath 2026 string equivalence — each pinned by an extra case that
  runs on in-memory, PostgreSQL and DuckDB.
- Fixed: SQL injection through a `join()` separator (all view text now goes
  through one quoting helper); fhirpath-rs `memberOf()` argument order,
  empty-input results of `all`/`allTrue`/`anyTrue`/`isDistinct`/`subsetOf`/
  `union`/`coalesce`/`aggregate`, `aggregate()` without `init`.
- `_format=fhir` (Parameters, one `row` per row, SQL→FHIR type mapping, 422
  for unmapped types) on both operations. Parquet still to do.

## Current state (2026-11-01, phases 2–3 done)

- `$sql-run` (system): `subjectCanonical` | `subjectReference` |
  `subjectResource`; ViewDefinition subjects run as `$viewdefinition-run`;
  SQLQuery / SQLView subjects resolve `relatedArtifact` dependencies
  (`context` first, then the server's Library / ViewDefinition by canonical;
  ViewDefinition has no `url` search parameter, so ids are looked up by
  `resource->>'url'` and read as the caller), labels must map to one artifact
  per job, cycles and depth > 16 → 422; `parameters` bound by name and
  declared type (`:name` → `$n::type`), unknown / mistyped / missing required
  → 400; `patient`, `group`, `_since` applied to every ViewDefinition
  dependency before the SQL; dialect `postgresql` attachment, else portable.
- Sandbox (`operations/sof/sql_guard.rs`, `octofhir-db-postgres` `sql_job`):
  banshee checks one SELECT, relations = labels + own CTEs, unqualified
  functions from an allowlist, no `$n`, no `;`; at run time a REPEATABLE READ
  transaction materializes each dependency as `pg_temp._sof_dep_N` (exposed
  to the SQL as a CTE named by its label, so our generated SQL is never
  shadowed), then `transaction_read_only`, `statement_timeout` 30s and
  `SET LOCAL ROLE octofhir_sof_query` (NOLOGIN, no table privileges; created
  on first use, SQL subjects refused if the DB user cannot). Verified:
  `set_config('role', …)` would escape the role, which is why the function
  allowlist is mandatory.
- Tests: `tests/sql_run.rs` (own binary: two servers in one test process
  collide on global state).
- Gaps: banshee does not parse column definition lists (`AS t(a int)`), so
  such SQL is rejected (fail-closed) — fix in banshee; parquet; Accept-header
  format negotiation; `$sql-export`.

## Authorization of the SQL path (the hard part)

`OperationContext::search` layers: SMART scopes → admission policy →
`query_visibility` (compartment / filtered-grant `SearchConstraint`, SQL) →
JS policies that may need a per-resource snapshot. A view run over raw SQL must
see exactly the rows `search` would return. Per source resource type:

1. **Unrestricted** (admission Allow, no visibility constraint, no snapshot
   policy): `FROM <table> base` directly.
2. **SQL-restricted** (constraint expressible in SQL): the generator's source
   becomes `(SELECT … FROM <table> WHERE <constraint SQL>) base` — reuse the
   search crate's constraint → SQL lowering; needs a source-relation hook in
   `SqlGenerator`.
3. **Snapshot policies**: fall back to the in-memory path over `search` (today's
   behavior) and say so in the response (`OperationOutcome` information issue).

`patient` / `group` / `_since` filters are additional source predicates of the
same kind (spec: applied before projection / before the SQL).

## Phases

1. **SQL execution for views** — server `SqlExecutor` over `read_db_pool`,
   source-relation hook in sof, authorization tiers above, `_limit`, output
   formats (ndjson default, csv, json, parquet, fhir). Keep `$run` working,
   switch it to the SQL path when tier 1/2 applies. Tests: parity with the
   in-memory path on the v2 conformance suite through the server.
2. **`$sql-run` (system)** — subjectCanonical / subjectReference / subjectResource,
   `resource` (views only, in-memory), `context`, `patient`, `group`, `_since`,
   `_limit`, `_format`, `header`; ViewDefinition subjects first.
3. **SQLQuery / SQLView** — dependency graph from `relatedArtifact`
   (labels = CTE names), `parameter` binding as bound parameters (never
   interpolated), dialect content selection (`application/sql;dialect=postgresql`),
   cycle detection, one data snapshot per job (single transaction,
   `REPEATABLE READ`).
4. **Terminology relations** — ValueSet dependency → expansion from the server
   terminology layer into a per-job relation (`VALUES`/temp table), ConceptMap
   → rows per spec rules incl. rejections; one resolution per job; record
   provenance (expansion identifier/timestamp/parameters); 404/422 per spec.
5. **`$sql-export`** — async jobs + manifest, multiple subjects, per-subject
   output names; reuse bulk writer.
6. **v3 canonicals** — accept both `org.sql-on-fhir.ig` and
   `hl7.fhir.uv.sql-on-fhir` artifacts; R6 additional-resource ViewDefinition.
7. **Console** — ViewDefinition page runs through `$sql-run`; SQLQuery editor
   with banshee LSP; terminology dependencies visible.

Later / explicitly not now: maintained materialized views
(`$viewdefinition-materialize`) — only with our own incremental-by-`txid`
design.

## Release chain

Done (2026-11-02): fhirpath-rs 0.5.1 → sof 0.2.0 (features, CompileOptions,
source hook) → sof 0.2.1 (patched dependencies) → server-rs on the published
crates, no `[patch.crates-io]`. Next sof release follows the next lowering
work; banshee 0.2.5 parses column definition lists, so the server can move to
it when the `$sql-run` guard admits `AS t(a int)`.
