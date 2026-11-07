# SQL on FHIR in the server — plan

Status: 2026-11-07. Priority: strategic (multi-year). Quality bar: the server path over its own
PostgreSQL is the product; the CLI / in-memory paths follow it.

## Spec baseline (verify against these, not memory)

| Source | What it fixes |
|---|---|
| SoF 2.0.0 / 2.1.0-pre (`org.sql-on-fhir.ig`) | ViewDefinition, `$viewdefinition-run`, `$viewdefinition-export`, FHIRPath subset (`getResourceKey`, `getReferenceKey` required; Shareable subset SHOULD) |
| SoF 3.0.0-ballot (2026-08-14, `hl7.fhir.uv.sql-on-fhir`, based on R6) — http://hl7.org/fhir/uv/sql-on-fhir/3.0.0-202609-ballot | New canonical base `http://hl7.org/fhir/uv/sql-on-fhir`; ViewDefinition as R6 additional resource; `identifier` repeats; one of forEach/forEachOrNull/repeat per select; `SQLQuery` / `SQLView` (Library profiles, `relatedArtifact` depends-on + `label` = table name); system operations `$sql-run` (sync) and `$sql-export` (async) over views **and** queries |
| SoF CI build (post-ballot) — https://build.fhir.org/ig/HL7/sql-on-fhir/en/terminology-in-sql.html (trial-use) | Terminology in SQL: a SQLQuery/SQLView declares a ValueSet / ConceptMap as a `depends-on` dependency; the runner exposes it as a relation. ValueSet → `system, version, code, display, inactive` (unique on first three, abstract entries excluded, nested entries flattened). ConceptMap → 9 columns `source_system … relationship`, noMap rows kept with null target, `dependsOn`/`product`/`valueSet`/source-less groups rejected, `unmapped` never applied. One resolution per job, before any SQL; unresolvable → 404, undeterminable content → 422. "The SQL never names a canonical URL or a terminology server." The ViewDefinition FHIRPath subset is unchanged. |

`memberOf()` in a ViewDefinition path is outside the required and shareable
subsets: we keep it as an extension of our runners (in-memory, needs a
TerminologyProvider), never as the way terminology reaches SQL.

## To do

Nothing open.
