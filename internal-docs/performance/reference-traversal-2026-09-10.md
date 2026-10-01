# Chain / _has SQL traversal — 2026-09-10

The native IR renderer now extracts reference IDs before matching target rows.
Forward chains use `target.id = ANY(ARRAY(SELECT ...))`, allowing target PK probes.
Reverse chains use an `IN` subquery over matching sources and their reference IDs,
allowing a semijoin without multiplying reference scans by target candidates.
No schema migration or new index is introduced.

## Reproduce

From the repository root, with Docker available:

```sh
cargo test -p octofhir-db-postgres --test reference_traversal_test --locked
cargo test -p octofhir-db-postgres --test reference_traversal_test --locked reference_sql_ab_benchmark -- --ignored --nocapture
```

Both tests create and destroy their own `postgres:16-alpine` container. They do
not consume DATABASE_URL or use an existing database. The manual benchmark is
ignored during ordinary test runs and was explicitly executed for these results.
Legacy SQL snapshots in `crates/octofhir-db-postgres/tests/fixtures/reference_*_legacy.sql`
were captured from the actual renderer before this change. The A/B test runs
those snapshots and current generated SQL against the same database and binds,
alternating execution order. It checks page sizes, unique IDs, and whether every
returned ID matches the deterministic fixture. With no sort, the two plans may
return different valid pages. Exact ID sets, accurate totals, and has_more are
asserted by the separate semantic test.

## SQL measurements

20,000 Patient and 20,000 Observation; each Observation references its Patient.
10% of patients have family Smith, and 10% of observations have code hit.
The database has the existing Patient.family and Observation.subject functional
indexes, plus the standard resource schema indexes. No experimental index is added.
Pool size 1; JIT and parallel workers off; ANALYZE after inserts. One warm-up pair
followed by five alternating pairs; values below are EXPLAIN ANALYZE execution
milliseconds, not HTTP latency, throughput, or production percentiles.

| Query | Legacy median (range), ms | New median (range), ms |
|---|---:|---:|
| Observation?subject:Patient.family=Smith&_count=10 | 267.271 (261.651–269.815) | 3.190 (3.062–3.347) |
| Patient?_has:Observation:subject:code=hit&_count=10 | 224.558 (221.969–228.965) | 10.567 (9.924–10.894) |
| Same _has plus _id=p-10 | 0.029 (0.028–0.030) | 0.028 (0.025–0.030) |

Representative plans: chain reference Function Scan loops fall from 198,066 to
110, with 110 `patient_pkey` probes. Reverse chain reference loops fall from
198,066 to 2,000, with 11 `patient_pkey` probes. The selective _id case keeps one
PK probe and one reference scan. SQL builder pagination still emits LIMIT 11;
this change does not alter the separate raw executor lookahead behavior.

## Semantics and limits

The initial reference-form fixture passed before and after the rewrite. Extended
positive controls subsequently exposed a pre-existing terminal `_id` dispatch bug:
`_has:Observation:subject:_id=local` treated an ID as CodeableConcept JSON and
returned no match. Terminal built-in `_id` now uses the same typed ID renderer as
top-level search against the linked row alias. The failing positive test is kept;
negative `_id` tests can no longer pass merely because all IDs fail to match.

Tests cover relative,
absolute, remote, versioned, wrong-type, contained, dangling, identifier-only,
null and empty references; scalar and array references; duplicate references;
deleted sources/targets; nested chain and nested _has; independent repeated
chains, and mixed bind parameters. Some array fixtures intentionally exercise the
renderer independent of a complete clinical resource validation pipeline.

This is an optimization of the existing chain resolution contract, not a claim
that its resolution policy is fully FHIR-conformant. In particular, the existing
chain/_has paths match absolute/remote suffix references against local rows and
reject versioned references. Direct reference search normalizes version suffixes;
that discrepancy remains explicit in regression expectations. Resolution policy
and nested repeating-parent/union path support are not changed here.

_revinclude is not rewritten in this patch. Its absolute/remote matching must be
preserved when using the existing base-free reference GIN; substituting only a
relative Type/id probe would lose matches. No write-cost benchmark is necessary
for this patch because no index or write path changes; broader workload and HTTP
measurements remain future work.
