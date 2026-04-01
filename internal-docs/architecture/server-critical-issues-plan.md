# Server Critical Issues Plan

## Scope

This plan focuses only on critical server problems:

- correctness bugs
- search and pagination behavior
- consistency risks in storage/indexing
- operational bottlenecks
- near-term production blockers

Explicitly deferred from this plan:

- XML support
- SDK generation
- broad product/platform expansion

## Priority Order

1. Search correctness
2. Query cache correctness
3. Search index consistency
4. Bulk import scalability
5. Conformance and production-readiness
6. Multi-tenancy isolation

## P0: Search Correctness

### Problem 1: Pagination is logically broken

Current behavior:

- search query uses `LIMIT count`
- `has_more` is computed as `entries.len() > limit`
- this can never become true unless the query fetches `count + 1`

Relevant files:

- [crates/octofhir-db-postgres/src/queries/search.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-db-postgres/src/queries/search.rs#L319)
- [crates/octofhir-search/src/sql_builder.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-search/src/sql_builder.rs#L912)
- [benchmark-results/search.json](/Users/alexanderstreltsov/work/octofhir/server-rs/benchmark-results/search.json)

Impact:

- `next` link generation is wrong
- paging clients can stop prematurely
- benchmark results are misleading

Plan:

1. Change main search execution to fetch `count + 1`.
2. Trim the extra row before bundle generation.
3. Base `next` link generation on true `has_more`, not inferred total.
4. Add integration tests for first page, middle page, last page.

Success criteria:

- `next` exists when additional matches are available
- pagination tests pass for both normal and `_include/_revinclude` searches

### Problem 2: Search bundle links are fed an incorrect total

Current behavior:

- when `_total=accurate` is not requested, bundle generation uses `result.entries.len()`
- this is page size, not dataset size

Relevant files:

- [crates/octofhir-server/src/handlers.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-server/src/handlers.rs#L2253)
- [crates/octofhir-api/src/lib.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-api/src/lib.rs#L1176)

Impact:

- `last` link may be misleading
- navigation semantics are unstable without accurate totals

Plan:

1. Decouple `next/previous` from `total`.
2. When total is unknown, emit only safe links:
- `self`
- `first`
- `previous` when offset > 0
- `next` only when `has_more = true`
3. Emit `last` only when accurate total is known.

Success criteria:

- search links stay spec-reasonable without fake total semantics

## P0: Query Cache Correctness

### Problem 3: Query cache ignores actual limit/offset values

Current behavior:

- cache key stores only `has_pagination`
- SQL embeds concrete `LIMIT/OFFSET` values as literals
- cached SQL can be reused across requests with different page settings

Relevant files:

- [crates/octofhir-search/src/query_cache.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-search/src/query_cache.rs#L81)
- [crates/octofhir-db-postgres/src/queries/search.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-db-postgres/src/queries/search.rs#L197)
- [crates/octofhir-search/src/sql_builder.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-search/src/sql_builder.rs#L912)
- [crates/octofhir-server/src/server.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-server/src/server.rs#L1701)

Impact:

- wrong page results can be returned
- cache becomes a correctness bug, not an optimization

Plan:

Option A:

- temporarily disable query cache for paginated searches

Option B:

- parameterize `LIMIT` and `OFFSET`
- include them in bind params
- keep cache keyed by structural shape

Recommended:

1. Immediate mitigation: disable cache usage when `count` or `offset` is present.
2. Proper fix: turn pagination into SQL bind params and restore caching safely.
3. Add regression tests:
- same search shape, different `_count`
- same search shape, different `_offset`

Success criteria:

- repeated searches with different paging settings return correct page windows

## P1: Search Index Consistency

### Problem 4: Resource writes and search index writes are not atomic

Current behavior:

- CRUD commits resource write first
- search indexes are written after that
- index write failures are logged and ignored

Relevant files:

- [crates/octofhir-db-postgres/src/storage.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-db-postgres/src/storage.rs#L148)
- [crates/octofhir-db-postgres/src/storage.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-db-postgres/src/storage.rs#L312)
- [crates/octofhir-db-postgres/src/search_index.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-db-postgres/src/search_index.rs#L21)

Impact:

- search can become stale or incorrect after successful writes
- reindex becomes mandatory recovery tooling
- debugging data mismatches becomes expensive

Plan:

1. Move search index writes into the same DB transaction as create/update/delete.
2. Add a consistency mode:
- strict mode: fail the write if index update fails
- degraded mode: allow write but enqueue repair job
3. Add index consistency checker command/admin endpoint.
4. Add repair workflow using `$reindex`.

Success criteria:

- no successful write can leave the index tables stale in strict mode

### Problem 5: Index rewrite path causes write amplification

Current behavior:

- delete old rows
- insert new rows
- repeated per resource

Impact:

- heavy update workloads pay extra churn
- import and reindex become slower than necessary

Plan:

1. Measure hot resource types under realistic update load.
2. Batch index operations where possible.
3. Consider transactional staging/upsert patterns for index rows.
4. Add perf tests specifically for write-heavy Observation workloads.

## P1: Bulk Import Scalability

### Problem 6: `$import` reads whole NDJSON payload into memory

Current behavior:

- remote file fetched with `reqwest`
- body materialized with `.text()`
- lines split in memory
- processing is largely sequential
- `skip_validation` is still TODO

Relevant file:

- [crates/octofhir-server/src/operations/bulk/import.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-server/src/operations/bulk/import.rs#L349)

Impact:

- memory spikes on large files
- throughput will degrade on real imports
- config flags imply behavior that is not fully implemented

Plan:

1. Replace full-body reads with streaming NDJSON parsing.
2. Use bounded worker concurrency for parse/validate/write.
3. Implement real validation gating for `skip_validation`.
4. Add backpressure and per-batch progress updates.
5. Add benchmarks for:
- 100 MB import
- 1 GB import
- mixed resource import

Success criteria:

- no whole-file memory load
- stable throughput under large imports

## P1: Search Performance Hotspots

### Problem 7: String search depends on expensive JSON traversal

Current behavior:

- `LOWER(...) LIKE`
- `jsonb_array_elements`
- narrative full-text over JSON text paths

Relevant files:

- [crates/octofhir-search/src/types/string.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-search/src/types/string.rs#L17)
- [internal-docs/architecture/search-indexing.md](/Users/alexanderstreltsov/work/octofhir/server-rs/internal-docs/architecture/search-indexing.md)

Impact:

- large Patient and Observation datasets will hit CPU-heavy scans
- p95 may look fine on small seeded sets but fail on realistic production data

Plan:

1. Collect `EXPLAIN ANALYZE` for top search shapes.
2. Identify repeated string fields that deserve expression or trigram indexes.
3. Add workload-driven indexes instead of blanket denormalization.
4. Publish guidance for deployment-time optional indexes.

Success criteria:

- top 3 slow string queries have explicit mitigation path and measured improvement

## P2: Production Credibility

### Problem 8: No published conformance results

Current state:

- Inferno setup exists
- no published pass matrix in repo
- README still says not production-ready

Relevant files:

- [README.md](/Users/alexanderstreltsov/work/octofhir/server-rs/README.md#L3)
- [docs/competitive/roadmap.md](/Users/alexanderstreltsov/work/octofhir/server-rs/docs/competitive/roadmap.md)

Impact:

- hard to claim reliability against HAPI, Aidbox, Firely, managed offerings

Plan:

1. Run a pinned Inferno suite.
2. Triage failures.
3. Publish pass rate and failing categories.
4. Gate regressions in CI.

Success criteria:

- reproducible conformance report committed or published

### Problem 9: Benchmark artifacts are not yet trustworthy enough

Current state:

- some artifacts show impossible percentile values like zero

Relevant file:

- [benchmark-results/crud.json](/Users/alexanderstreltsov/work/octofhir/server-rs/benchmark-results/crud.json)

Plan:

1. Fix benchmark export formatting/aggregation.
2. Separate warmup from measured interval.
3. Publish environment metadata with results.
4. Track p50, p95, p99, max, error rate consistently.

## P2: Multi-Tenancy

### Problem 10: Tenant concepts exist, data isolation does not

Current state:

- tenant context exists in config/auth flows
- core resource tables have no tenant column

Relevant files:

- [crates/octofhir-config/src/feature_flags.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-config/src/feature_flags.rs#L14)
- [crates/octofhir-auth/src/smart/launch.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-auth/src/smart/launch.rs#L76)
- [crates/octofhir-db-postgres/src/schema.rs](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-db-postgres/src/schema.rs#L132)

Impact:

- enterprise isolation story is incomplete
- this blocks real multi-tenant production use

Plan:

1. Design storage-level tenant model.
2. Add tenant-aware routing and auth enforcement.
3. Scope search, history, include/revinclude, bulk ops, and caches by tenant.
4. Add cross-tenant isolation tests.

This is important, but after P0/P1 correctness issues.

## Suggested Execution Sequence

### Week 1

1. Fix pagination logic.
2. Disable unsafe pagination query cache path.
3. Add regression tests.

### Week 2

1. Parameterize `LIMIT/OFFSET`.
2. Restore safe cache usage.
3. Fix search link semantics.

### Week 3

1. Move search index writes into transactional flow.
2. Add strict consistency mode.
3. Add repair tooling/tests.

### Week 4

1. Stream `$import`.
2. Implement true validation behavior.
3. Add import benchmarks.

### Week 5+

1. Inferno pass reporting.
2. Benchmark cleanup.
3. Multi-tenancy design.

## Non-Goals For This Track

- XML
- SDK generator
- plugin SDK
- new protocol surfaces
- broad UI work

These can resume after the server stops having correctness risks in core FHIR behavior.
