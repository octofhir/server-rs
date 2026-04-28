# FHIR Server Performance & Rust Quality Review

## Executive Summary

Главные риски:
- Несколько FHIR-correctness дефектов находятся прямо в hot path: conditional update обновляет payload без принудительной подстановки matched id; transaction-response формируется в отсортированном, а не исходном порядке; conditional create продолжает create при ошибке search.
- Часть "raw" оптимизаций уже спроектирована, но не доведена до write path: `create_raw`/`update_raw` в PostgreSQL storage идут через `StoredResource` и затем сериализуют `serde_json::Value` обратно в строку.
- Search path уже использует raw JSON для обычного GET/POST search, но системный search, `_summary`/`_elements`, Bundle transaction и include/revinclude всё ещё делают крупные `Value` clone/serialize циклы.
- PostgreSQL модель имеет хорошие базовые таблицы и reference/date indexes, но common FHIR searches token/string/quantity остаются без денормализованных индексов; default `_lastUpdated` sort строится по JSONB `meta.lastUpdated`, хотя таблица уже имеет `updated_at` B-tree.
- Observability сейчас измеряет HTTP, pool и cache верхнего уровня, но не разделяет validation, SQL build, DB time, include/revinclude, serialization, Bundle phases и async search-index lag.

Quick wins с высоким ROI:
- Подключить существующие `crud::create_raw`/`crud::update_raw` к `PostgresStorage::{create_raw,update_raw}`.
- Исправить conditional update id handling, transaction-response order и conditional-create-on-search-error.
- Спец-обработать `_lastUpdated` через `updated_at`, а не JSONB path.
- Убрать full-bundle `serde_json::to_value` в system search без `_summary`/`_elements`.
- Исправить POST `_search` extractor, чтобы не терять repeated params.

Ожидаемый эффект до профилирования:
- Write latency/allocations: заметное снижение для `Prefer: return=representation` и default create/update за счёт устранения `Value -> String` round-trip.
- Search p95/p99: улучшение на больших таблицах после `_lastUpdated` column sort и token/string indexes.
- Bundle transaction: снижение allocator pressure на пакетах 10-100 entries после устранения full-entry clones и response-order refactor.
- Correctness: закрытие FHIR interoperability bugs, которые сейчас могут приводить к duplicate create, неверному update target и некорректному transaction-response.

Verification status:
- `cargo check --all-targets --all-features` сейчас падает в `crates/octofhir-cql-service/src/library_cache.rs:269-298` из-за `deadpool_redis::redis::AsyncCommands` import/type inference. Это блокирует workspace-wide CI gate.
- `cargo check -p octofhir-search -p octofhir-db-postgres -p octofhir-storage --all-targets` проходит, но есть warnings в тестовом коде `octofhir-search` и dead code warning `PG_UNDEFINED_FUNCTION`.
- Rust benches почти отсутствуют: найден только `crates/octofhir-auth/benches/quickjs_bench.rs`. В `justfile` есть k6 recipes для CRUD/search/transaction/concurrency/bulk.
- Доступны локально: `cargo-bloat`, `cargo-flamegraph`, `samply`. Не найдены: `cargo-llvm-lines`, `heaptrack`.

## Architecture Map

Crates/modules:
- `crates/octofhir-server`: Axum application, route graph, middleware, FHIR handlers, validation wiring, operation registry, CapabilityStatement, auth/authorization integration.
- `crates/octofhir-storage`: object-safe `FhirStorage`/`Transaction` traits, `StoredResource`/`RawStoredResource`, `EventedStorage`.
- `crates/octofhir-db-postgres`: PostgreSQL storage implementation, CRUD/history/search SQL, schema management, migrations, search index extraction/writes, async index writer.
- `crates/octofhir-search`: search parameter parser, registry, `SearchParams`, SQL builder, params converter, include/revinclude/chaining/reverse-chaining helpers.
- `crates/octofhir-core`: shared FHIR ids, errors, events, reference normalization, search-index extraction helpers.
- `crates/octofhir-api`: response helpers for Bundle, CapabilityStatement, OperationOutcome, `RawJson`.
- Neighbor crates: `octofhir-auth`, `octofhir-auth-postgres`, `octofhir-config`, `octofhir-graphql`, `octofhir-cql-service`, `octofhir-notifications`, `octofhir-sof`, `octofhir-cli`.

Request lifecycle:
- Client hits Axum router built in `crates/octofhir-server/src/server.rs`.
- Middleware stack includes trace metrics, dynamic CORS, auth, authorization, optional compression and body limit.
- Handler in `crates/octofhir-server/src/handlers.rs` parses JSON or form/query params.
- Structural validation runs via `validate_resource`; schema/FHIRPath validation runs via `ValidationService`.
- Storage is injected as `DynStorage = Arc<dyn FhirStorage>` through `AppState`.
- CRUD/search/history/transaction handlers call storage or raw PostgreSQL search functions.
- Response is built as `Json<Value>`, typed Bundle with `RawJson`, or raw JSON bytes depending on path.

DB lifecycle:
- `PostgresStorage` owns write `PgPool`, optional read replica pool, schema manager, optional query cache and optional async index writer.
- Create/update outside Bundle: open SQL transaction, insert/update resource and `_transaction`, commit, then enqueue async search-index write.
- Create/update inside Bundle: `PostgresTransaction` writes resources and search indexes inside one DB transaction.
- Reads/searches use `read_pool()` and can return raw JSON via `resource::text`.
- History uses per-resource history tables maintained by triggers.

FHIR operation lifecycle:
- CRUD: `/fhir/{type}`, `/fhir/{type}/{id}`, vread/history, conditional create/update/delete/patch.
- Search: type search GET/POST, system search with `_type`, include/revinclude resolution.
- Bundle: `/fhir` POST routes to transaction/batch/async transaction handling.
- CapabilityStatement: built once at startup, cached as `Arc<serde_json::Value>`, cloned per metadata request.
- Operations/GraphQL/Auth/CQL are integrated, but the primary hot path remains FHIR CRUD/search/Bundle.

Key hot paths:
- `create_resource`, `update_resource`, `conditional_update_resource`.
- `search_resource`, `search_resource_post`, `system_search`.
- `process_transaction`, `process_transaction_entry_with_tx`, `process_batch`.
- `execute_search_raw_with_config`, `build_query_from_params_with_config`, include/revinclude resolvers.
- `PostgresStorage::{create,create_raw,update,update_raw,read_raw}`.
- `PostgresTransaction::{create,update,create_batch,search}`.
- `apply_result_params_to_resource`, `apply_result_params`, `apply_summary`, `apply_elements_filter`.
- `metadata`.

## Findings

### [P0] PostgreSQL write "raw" path is not raw
- Evidence: `crates/octofhir-server/src/handlers.rs:715-716` calls `state.storage.create_raw(&payload)` with a comment saying the raw path avoids serde round-trip. `crates/octofhir-db-postgres/src/storage.rs:351-357` implements `create_raw` as `self.create(resource).await?` followed by `stored_to_raw`. `storage.rs:412-419` does the same for `update_raw`. Real raw SQL helpers already exist in `crates/octofhir-db-postgres/src/queries/crud.rs:136-207` and `crud.rs:477-600`.
- Why it matters: create/update hot path pays for DB JSONB -> `serde_json::Value` decode and `Value` -> string serialization even when the handler wants raw JSON. This increases allocation bytes and p95 latency under write load.
- Current behavior: handler believes it is using raw response path, but storage returns a fully materialized `StoredResource` first.
- Recommended change: implement `PostgresStorage::create_raw` and `update_raw` using `queries::crud::{create_raw,update_raw}` or equivalent `*_with_tx_raw`; return `RawStoredResource` directly. Keep index extraction explicit: either extract from the original/injected payload, or parse returned raw JSON only when async indexer/events require it.
- Expected impact: lower allocation count and CPU for create/update responses; effect should be measurable with `Prefer: return=representation` and default create/update benchmarks.
- Risk: search-index extraction currently depends on a `Value` with final id/meta. Need preserve id/meta consistency between returned raw JSON and indexed payload.
- Acceptance criteria: criterion or k6 create/update benchmark shows at least 15% fewer allocated bytes or lower p95 at same throughput; integration tests verify id/meta/version, ETag/Location and search index rows after create/update.

### [P0] Conditional update can update the wrong target or fail valid requests
- Evidence: `crates/octofhir-server/src/handlers.rs:1647-1649` searches for matches; one-match branch stores matched id at `handlers.rs:1726-1728`; `validate_payload_structure` is called with `IdPolicy::Update { path_id: id.clone() }` at `handlers.rs:1746-1755`; then storage is called with the original `payload` at `handlers.rs:1757`. There is no mutation that injects the matched id into payload before `state.storage.update`.
- Why it matters: FHIR conditional update is defined by search criteria, not by the incoming resource id. A payload without id should update the matched resource; a mismatched id should be rejected deterministically. Current code depends on payload content and can fail or update an unintended id.
- Current behavior: matched id is used for validation and response headers, but not for the stored resource.
- Recommended change: in one-match conditional update, create `let mut payload = payload; payload["id"] = json!(id);` after mismatch validation, then call `update_raw`/`update` with that mutated payload. Add explicit behavior for payload id mismatch.
- Expected impact: correctness fix with negligible runtime cost; removes an interoperability bug in a hot write path.
- Risk: clients that currently rely on invalid mismatched payload behavior will break, correctly.
- Acceptance criteria: tests for `PUT /Patient?identifier=...` with no id, matching id, and mismatched id; no-id case updates matched resource and returns `200`; mismatched id returns `400` or `409` consistently.

### [P0] Transaction-response entries are returned in sorted processing order, not request order
- Evidence: `process_transaction` sorts entries at `crates/octofhir-server/src/handlers.rs:3533-3534`; all subsequent indexes are indexes in `sorted_entries` (`handlers.rs:3546`, `3647`, `3651`, `3722`); response entries are collected directly at `handlers.rs:3735-3738`. FHIR R4 HTTP states transaction/batch response Bundle contains one entry per request entry "in the same order" ([HL7 R4 HTTP 3.1.0.11.3](https://www.hl7.org/fhir/R4/http.html#transaction-response)).
- Why it matters: clients map transaction responses back to request entries by position. Returning sorted order can corrupt client-side id mapping and retry logic.
- Current behavior: execution order and response order are coupled.
- Recommended change: store `(original_index, entry)` before sorting. Execute in dependency-safe order, but write each response into `response_entries[original_index]`. Keep `matched_conditional` keyed by original index or by stable entry id.
- Expected impact: correctness fix; small memory overhead for original indexes.
- Risk: existing tests may assert current sorted order if they were written from implementation behavior.
- Acceptance criteria: integration test with mixed DELETE/POST/PUT/GET request order verifies response entries align with original order; FHIR spec link is cited in test comment.

### [P0] Conditional create proceeds with create when search fails
- Evidence: `create_resource` parses `If-None-Exist` at `crates/octofhir-server/src/handlers.rs:630-635`; on `state.storage.search` error it logs and proceeds with create at `handlers.rs:696-699`. FHIR conditional create requires the server to perform the search and decide by match count ([HL7 R4 HTTP 3.1.0.8.1](https://www.hl7.org/fhir/R4/http.html#ccreate)); search failure should be an error, not a create.
- Why it matters: transient DB/search errors can create duplicates for identifiers that were meant to be idempotent.
- Current behavior: search failure is treated as "no blocker".
- Recommended change: return `ApiError` on conditional search failure. Prefer `500` for backend failure and `400` for invalid search parameter. Do not commit create.
- Expected impact: correctness and data quality; no positive throughput impact, but prevents duplicate writes under partial failure.
- Risk: if current deployments intentionally tolerate index/search outages, this changes write availability semantics. Add a config flag only if product explicitly accepts duplicate risk.
- Acceptance criteria: fault-injection test where conditional search returns error proves no resource is created; metrics counter `fhir_conditional_create_search_errors_total` increments.

### [P1] Default `_lastUpdated` sort does not use the indexed `updated_at` column
- Evidence: default sort is JSONB path `meta.lastUpdated` at `crates/octofhir-search/src/params_converter.rs:265-269`. Per-resource tables have B-tree `updated_at` index at `crates/octofhir-db-postgres/src/schema.rs:219-226`.
- Why it matters: common search without explicit sort can force JSONB expression sort instead of using the table timestamp index, increasing DB CPU, memory and p99 latency on large resource tables.
- Current behavior: query builder sorts by JSONB path even though `updated_at` is maintained as a column.
- Recommended change: add a `SortSpec::Column("r.updated_at")` path for default sort and explicit `_sort=_lastUpdated`. Also special-case `_lastUpdated` filter to table column where semantics match persisted meta.
- Expected impact: lower DB sort cost and fewer temp files; measurable via `EXPLAIN (ANALYZE, BUFFERS)` on `Patient?_count=50` and `Observation?_sort=-_lastUpdated`.
- Risk: must guarantee `meta.lastUpdated` and `updated_at` are always identical. CRUD SQL injects meta, but this should be covered by tests.
- Acceptance criteria: generated SQL uses `r.updated_at`; planner uses `idx_{table}_updated_at`; p95 search latency improves on a dataset of at least 1M rows or plan cost drops with no semantic regression.

### [P1] POST `_search` loses repeated parameters
- Evidence: `search_resource_post` extracts `axum::Form<HashMap<String, String>>` at `crates/octofhir-server/src/handlers.rs:2469-2473`, rebuilds a query string via `format!` and `Vec<String>::join` at `handlers.rs:2481-2486`. FHIR R4 says POST search parameters have same semantics as GET and parameters may repeat ([HL7 R4 HTTP search](https://www.hl7.org/fhir/R4/http.html#search), lines 391-397 in the referenced page).
- Why it matters: repeated `_include`, `_revinclude`, token OR values, repeated code filters and mixed URL/body params can be silently collapsed. It is both correctness and performance risk.
- Current behavior: HashMap keeps only one value per key and allocates a new encoded query string before parsing.
- Recommended change: extract raw form body as `Bytes`, parse with `url::form_urlencoded::parse`, and feed pairs directly into `SearchParams`. Merge URL query params and body params preserving repetition.
- Expected impact: correct semantics and fewer allocations in POST search.
- Risk: extractor changes handler signature and tests around content type.
- Acceptance criteria: POST `_search` test with two `_include` params and two `identifier` params produces same `SearchParams` as GET; allocation benchmark shows removal of intermediate `Vec<String>`.

### [P1] System search is serial, over-fetches and paginates in memory
- Evidence: `system_search` loops types sequentially at `crates/octofhir-server/src/handlers.rs:2596-2624`, pushes all returned entries into `all_entries` at `handlers.rs:2588-2617`, then applies combined `skip/take` in memory at `handlers.rs:2638-2661`. It always converts typed Bundle to `Value` through `apply_result_params` at `handlers.rs:2676-2679`, even when no `_summary`/`_elements`.
- Why it matters: `_type=Patient,Observation,...` does N DB round-trips, over-fetches per type, gives unstable global ordering/pagination, and allocates all intermediate raw JSON strings.
- Current behavior: no DB-level global limit/order; no include/revinclude handling; no fast path for plain system search response.
- Recommended change: for P1, run per-type queries concurrently with a bounded semaphore and fetch only `offset+count+1` candidates per type. For P2, build a UNION ALL query over selected resource tables with global `ORDER BY updated_at DESC LIMIT/OFFSET`. Return typed Bundle directly when there are no result params.
- Expected impact: lower wall-clock latency for multi-type search; lower memory for high `_count`/many `_type`.
- Risk: UNION query has dynamic table list and needs careful SQL generation/plan validation.
- Acceptance criteria: benchmark `_type=Patient,Observation,Encounter&_count=50` over large tables; p95 improves and result ordering/pagination is deterministic across pages.

### [P1] `_summary` and `_elements` force full Bundle/resource materialization
- Evidence: single-resource result filtering parses raw JSON to `Value` and serializes back at `crates/octofhir-server/src/handlers.rs:886-900`. Search Bundle filtering calls `serde_json::to_value(bundle)` at `handlers.rs:2700-2707`, then clones fields in `apply_summary`/`apply_elements_filter` at `handlers.rs:2744-2801`.
- Why it matters: `_summary` is intended to reduce load, but current implementation often adds server CPU and allocation work after DB retrieval.
- Current behavior: raw search/read paths lose their raw advantage whenever `_summary`/`_elements` are present.
- Recommended change: introduce a resource projection helper that writes filtered JSON to `serde_json::Serializer` or uses `RawValue` plus `serde_json::Map` only for the resource being filtered. Longer term, cache summary projections for large resource types or use generated summary element metadata from StructureDefinition.
- Expected impact: lower allocations for `_summary=true`, `_summary=text`, `_elements=...`; p95 improves for large resources.
- Risk: projection must preserve mandatory/modifier elements and SUBSETTED tag semantics.
- Acceptance criteria: JSON serialization benchmark for 10 KB/100 KB resources; `_summary` allocated bytes must not exceed full-resource response by more than 10%.

### [P1] `_summary=true` is not semantically accurate
- Evidence: `apply_summary("true")` returns only `resourceType`, `id`, `meta`, `text` at `crates/octofhir-server/src/handlers.rs:2744-2758`. FHIR R4 defines `_summary=true` as elements marked `ElementDefinition.isSummary`; `_summary=text` is the text/id/meta form ([HL7 R4 Search 3.1.1.5.8](https://www.hl7.org/fhir/R4/search.html#summary)).
- Why it matters: clients requesting summary may miss summary fields such as Patient identifiers, Observation code/status/effective, etc. Trying to make this fast by hardcoding a tiny subset is incorrect.
- Current behavior: `_summary=true` behaves closer to an underspecified text/minimal response.
- Recommended change: load summary element paths from canonical StructureDefinitions already used by the server, generate per-resource summary projections, and test against representative R4/R4B resources. Keep an escape hatch to return full resource, which FHIR allows, if projection metadata is missing.
- Expected impact: correctness improvement; performance depends on projection implementation.
- Risk: generated projections for polymorphic/repeating elements need careful coverage.
- Acceptance criteria: tests for Patient, Observation, Encounter summary include expected `isSummary` fields; SUBSETTED tag is added when response is incomplete.

### [P1] Bundle transaction clones full entries/resources and batches clone again
- Evidence: `process_transaction` clones each sorted entry at `crates/octofhir-server/src/handlers.rs:3613-3632`; clones POST resources at `handlers.rs:3683-3686`; clones the whole batch into a new `Vec<Value>` at `handlers.rs:3696-3700`; `resolve_bundle_references` clones a full resource at `handlers.rs:4933-4940`; transaction response clones optional resource at `handlers.rs:4995-4997`. `EventedTransaction` clones stored resources into events at `crates/octofhir-storage/src/evented.rs:334-371`.
- Why it matters: transaction Bundles are allocation-heavy by shape. Full `Value` cloning across 10-100 entries amplifies allocator pressure and p99 tail latency.
- Current behavior: reference resolution and batching are implemented with whole-entry clones.
- Recommended change: retain original entry index, mutate only resources that need id/reference changes, avoid cloning request wrappers, pass slices/iterators to `create_batch`, and skip event resource cloning when broadcaster subscriber count is zero. Consider `Arc<Value>` or raw payload events for subscribers.
- Expected impact: fewer allocations and lower p99 for Bundle transaction.
- Risk: reference resolution is correctness-sensitive; must keep transaction atomicity and fullUrl behavior.
- Acceptance criteria: transaction benchmark with 10 and 100 POST entries plus UUID refs shows at least 25% fewer allocated bytes; response-order tests remain green.

### [P1] Bundle transaction conditional pre-scan happens outside the DB transaction
- Evidence: conditional create pre-scan calls `state.storage.search` before `begin_transaction` at `crates/octofhir-server/src/handlers.rs:3555-3561`; DB transaction begins later at `handlers.rs:3641-3645`.
- Why it matters: another request can create a matching resource between pre-scan and commit. This defeats conditional-create duplicate protection in high-concurrency ingestion.
- Current behavior: pre-scan optimizes away duplicate search but opens a race window.
- Recommended change: resolve conditional creates inside the DB transaction with appropriate isolation/retry, or enforce unique constraints on indexed identifiers used for idempotency. If using async indexer outside transaction, do not rely on stale search indexes for transactional conditional decisions.
- Expected impact: correctness under concurrency; possible additional DB locking cost.
- Risk: SERIALIZABLE isolation or explicit locking can reduce throughput if not scoped narrowly.
- Acceptance criteria: concurrent transaction test with same `If-None-Exist identifier=...` creates one resource and returns existing for the rest; no duplicates after 100 parallel attempts.

### [P1] Batch processing shares transaction-style reference map across independent entries
- Evidence: `process_batch` creates one `reference_map` at `crates/octofhir-server/src/handlers.rs:4215-4216` and passes `&mut reference_map` to each entry at `handlers.rs:4220-4221`, despite comment saying each entry is independent. FHIR describes batch as independent HTTP operations, unlike transaction atomic processing ([HL7 R4 Bundle transaction/batch](https://www.hl7.org/fhir/R4/bundle.html#bundle)).
- Why it matters: a batch entry can resolve `urn:uuid` from a previous batch entry as if it were a transaction, which is not valid batch isolation.
- Current behavior: batch entries may leak fullUrl mappings across entries.
- Recommended change: use a fresh reference map per batch entry, or reject inter-entry UUID references in batch with an OperationOutcome for that entry.
- Expected impact: correctness fix; negligible performance impact.
- Risk: clients incorrectly using batch as transaction will see failures.
- Acceptance criteria: batch test with second entry referencing first entry `urn:uuid` fails that entry or leaves reference unresolved according to chosen policy; transaction equivalent succeeds.

### [P1] Search index is eventually consistent for normal writes and failures are only logged
- Evidence: `PostgresStorage::create` commits resource at `crates/octofhir-db-postgres/src/storage.rs:320-328`, then calls `dispatch_index_write` and only logs failure at `storage.rs:331-345`. Update follows same pattern at `storage.rs:380-407`. Async writer logs flush errors without surfacing them in `crates/octofhir-db-postgres/src/index_writer.rs:137-144`.
- Why it matters: immediate search-after-create/update may miss new resources if index write lags or fails. This is a correctness/performance tradeoff that should be explicit and measurable.
- Current behavior: normal writes are fast, but reference/date index consistency is eventual. Bundle transaction writes indexes inside the transaction.
- Recommended change: expose config modes: `search_index.consistency = async|sync|transactional_for_conditional`. Conditional operations should use authoritative SQL or synchronous index path. Add queue depth, enqueue failures, flush failures and lag metrics.
- Expected impact: predictable semantics; sync mode costs write latency, async mode gets observable lag.
- Risk: sync indexing increases write DB time.
- Acceptance criteria: search-after-create test passes in sync mode; async mode exposes `search_index_queue_depth`, `search_index_lag_seconds`, `search_index_flush_errors_total`.

### [P1] Common token/string/quantity searches are not denormalized
- Evidence: `extract_search_index_rows` only handles `SearchParameterType::Reference` and `Date` at `crates/octofhir-db-postgres/src/search_index.rs:17-61`. Migration creates only `search_idx_reference` and `search_idx_date` at `crates/octofhir-db-postgres/migrations/20241213000001_consolidated_schema.sql:1282-1344`.
- Why it matters: high-cardinality FHIR searches like `Patient?identifier=`, `Observation?code=`, `Observation?status=`, `Patient?family=`, `Encounter?subject=` are production hot paths. JSONB fallback can be much slower and harder to plan than normalized indexes.
- Current behavior: reference/date searches can use denormalized tables; token/string/quantity rely on JSONB/search builder behavior.
- Recommended change: add staged index tables: `search_idx_token(resource_type,param_code,system,code,resource_id)`, `search_idx_string(resource_type,param_code,value_norm,value_exact,resource_id)`, later `search_idx_quantity`. Start with Patient.identifier/name, Observation.code/status/category, Encounter.subject/class/status.
- Expected impact: lower DB CPU and p99 for common clinical search params.
- Risk: index extraction semantics for FHIR token/string modifiers are complex; storage overhead increases.
- Acceptance criteria: EXPLAIN for selected queries uses new B-tree indexes; p95 improves on generated clinical dataset; index write overhead stays under agreed write-latency budget.

### [P1] Include query shape lacks an include-specific index and `:iterate` is not implemented
- Evidence: include resolver collects source ids at `crates/octofhir-db-postgres/src/queries/search.rs:858-865` and queries `search_idx_reference` by `resource_type`, `resource_id = ANY`, `param_code`, `ref_kind`, `target_type` at `queries/search.rs:887-893`. Existing migration indexes optimize forward target lookup and revinclude, but not `(resource_type,param_code,resource_id,target_type,target_id)` for include source-id fanout (`migrations/20241213000001_consolidated_schema.sql:1298-1311`). FHIR `:iterate` support is specified in R4 search ([HL7 R4 Search include](https://www.hl7.org/fhir/R4/search.html#include), lines 753-762), but resolver does one include/revinclude pass at `queries/search.rs:807-839`.
- Why it matters: include/revinclude can dominate p99 for clinical searches. Missing include-specific index causes extra index/table work. Ignoring `:iterate` is a correctness issue when advertised or parsed.
- Current behavior: includes are resolved concurrently across include specs, but per target type lookup is sequential and non-iterative.
- Recommended change: add partial index `ON search_idx_reference(resource_type,param_code,resource_id,target_type,target_id) WHERE ref_kind=1`. Implement bounded iterative include/revinclude with max depth/resource cap and dedupe by `(type,id,version)`.
- Expected impact: faster direct includes; correct bounded iterative behavior.
- Risk: iterative wildcard includes can explode result size. Must cap and add metrics.
- Acceptance criteria: EXPLAIN uses new include index; tests for `_include:iterate` and `_revinclude:iterate` pass with depth limit; metrics report included resource count and iteration count.

### [P1] History pagination has `_offset` typo and instance history does extra DB round trip
- Evidence: `HistoryQueryParams.offset` is renamed as `"__offset"` at `crates/octofhir-server/src/handlers.rs:918-920`. Instance history first calls `state.storage.read_raw` at `handlers.rs:1103-1122` and then `history_raw` at `handlers.rs:1137-1142`.
- Why it matters: clients using `_offset` will not paginate history as expected. Extra read adds latency and pool pressure to every instance history request.
- Current behavior: only non-standard `__offset` works for history extractor. Instance history checks current resource existence before querying history.
- Recommended change: rename to `"_offset"` and add compatibility for `"__offset"` only if needed. Remove the pre-read by making `history_raw` distinguish "no such resource/history" from empty page, or query history first with a cheap existence check.
- Expected impact: correctness and one fewer DB round trip for instance history.
- Risk: if current clients accidentally depend on `__offset`, compatibility may be needed temporarily.
- Acceptance criteria: history integration tests for `_count`/`_offset`; DB span count for instance history drops by one.

### [P1] CapabilityStatement clone and advertised capability accuracy need tightening
- Evidence: `metadata` clones the full cached `Value` on every request at `crates/octofhir-server/src/handlers.rs:183-199`. Builder advertises conditional create/update/delete/read/history flags in `handlers.rs:328-349` (from local inspection). CapabilityStatement fields are contractual: conditional flags, searchInclude/searchRevInclude and searchParam definitions are defined in HL7 R4 CapabilityStatement ([HL7 R4 CapabilityStatement definitions](https://hl7.org/fhir/R4/capabilitystatement-definitions.html)).
- Why it matters: `/metadata` is frequently polled by clients and monitors; cloning a large CapabilityStatement is avoidable. More importantly, advertised conditional/search/include behavior must match actual semantics.
- Current behavior: cached construction avoids startup rebuild per request, but response still clones. Several advertised capabilities are at risk while conditional update/search POST/include iterate issues exist.
- Recommended change: cache serialized `Bytes` or `Arc<RawValue>` variants for full/text/data/count summaries. Add CapabilityStatement golden tests derived from route/handler support. Do not advertise include/revinclude iterate unless implemented and bounded.
- Expected impact: lower allocation on metadata path; fewer client interoperability surprises.
- Risk: serialized cache must vary by FHIR version/base URL if those are configurable.
- Acceptance criteria: metadata benchmark shows near-zero per-request JSON allocations; conformance tests compare advertised interactions to working endpoint tests.

### [P2] Transaction abstraction uses async-trait, dynamic dispatch and Mutex in serial hot path
- Evidence: `FhirStorage`/`Transaction` are object-safe async traits in `crates/octofhir-storage/src/traits.rs`. `PostgresTransaction` wraps `PgTransaction` in `Mutex<Option<Box<PgTransaction<'static>>>>` and has a second `Mutex<Option<i64>>` at `crates/octofhir-db-postgres/src/transaction.rs:25-40`; every create locks both at `transaction.rs:95-105`.
- Why it matters: Bundle transaction operations are serial and already require `&mut self` for writes. Mutex and boxed dyn futures add overhead and obscure ownership.
- Current behavior: abstraction favors object safety and lifetime erasure over monomorphic fast path.
- Recommended change: keep public object-safe trait if needed, but add a PostgreSQL-native transaction fast path for server Bundle handling, or refactor `Transaction` so read/search also use `&mut self` and `PostgresTransaction` can hold `PgTransaction` directly without Mutex.
- Expected impact: modest CPU/allocation reduction in Bundle hot path; simpler correctness around transaction state.
- Risk: trait API change touches storage wrappers and tests.
- Acceptance criteria: bundle transaction microbenchmark shows reduced instruction count/allocations; no public API break outside workspace or migration guide is written.

### [P2] Query cache still rebuilds conversion and fragments by pagination
- Evidence: raw search clones `SearchParams` at `crates/octofhir-db-postgres/src/queries/search.rs:213-215`, allocates an empty registry at `queries/search.rs:217-220` when none is supplied, constructs cache key strings at `queries/search.rs:254-318`, and includes pagination via `.with_pagination(...)` at `queries/search.rs:311-318`. On cache hit it still extracts fresh params from a rebuilt builder at `queries/search.rs:335-350`.
- Why it matters: query cache reduces SQL string build only partially. Pagination in the cache key reduces hit rate for page-heavy clients.
- Current behavior: conversion/build path runs before cache lookup; LIMIT/OFFSET appear to be query-shape data.
- Recommended change: cache earlier normalized query shape, bind LIMIT/OFFSET as SQL params, and avoid allocating default registry by requiring registry in production search path.
- Expected impact: lower CPU in search build path, higher cache hit rate for paginated search.
- Risk: prepared template binding must remain correct across parameter type shapes.
- Acceptance criteria: add metrics for query cache hit/miss/build duration; repeated page requests hit same template; flamegraph shows reduced `build_query_from_params_with_config` share.

### [P2] HTTP metrics path allocates labels per request and lacks operation-level spans
- Evidence: `record_http_request` converts method/status/path labels to owned Strings at `crates/octofhir-server/src/metrics.rs:72-91`. `trace_metrics_middleware` clones method/uri and builds path strings at `crates/octofhir-server/src/middleware.rs:547-569` (local inspection). Metrics module exposes HTTP/pool/cache/FHIR counters but no validation/search/serialization/index histograms.
- Why it matters: current metrics are useful for external symptoms, not root-cause isolation. Per-request label allocation is minor but measurable at high RPS.
- Current behavior: only whole-request latency is recorded by route-like normalized path.
- Recommended change: add low-cardinality histograms/spans: validation duration, storage operation duration, SQL build duration, DB query duration by query kind, search include duration/count, serialization duration, bundle phase duration, index queue depth/lag/failures. Optimize label creation after root-cause metrics are in place.
- Expected impact: bottlenecks become attributable without full flamegraph in production.
- Risk: high-cardinality labels if resource ids/search params leak into metrics.
- Acceptance criteria: dashboards can split p95 request latency into validation, DB, search build/include and serialization buckets; labels limited to operation/resource_type/query_kind/status_class.

### [P2] Workspace quality gate is not currently green
- Evidence: `cargo check --all-targets --all-features` fails in `crates/octofhir-cql-service/src/library_cache.rs:269-298` because `deadpool_redis::redis::AsyncCommands` is not in scope and return type inference fails around `get`/`set_ex`.
- Why it matters: performance refactors across shared crates need a green baseline. A red workspace check hides regressions and blocks automated clippy/test gates.
- Current behavior: targeted storage/search check passes, but workspace-wide check does not.
- Recommended change: fix CQL Redis import/type annotations before broad refactors, then require `cargo check --all-targets --all-features` and `cargo clippy --all-targets --all-features -- -D warnings` in CI.
- Expected impact: no runtime performance effect, but improves refactor safety.
- Risk: CQL service may have version skew between `redis` and `deadpool-redis`.
- Acceptance criteria: workspace check and clippy pass from clean checkout.

## Allocation Reduction Plan

Concrete removal targets:
- `crates/octofhir-db-postgres/src/storage.rs:351-357` and `412-419`: use raw SQL return path instead of `StoredResource -> RawStoredResource`.
- `crates/octofhir-server/src/handlers.rs:183-199`: cache serialized CapabilityStatement variants; avoid cloning full `serde_json::Value`.
- `crates/octofhir-server/src/handlers.rs:886-900`: avoid parse/filter/serialize for raw single-resource `_summary`/`_elements`; use a projection writer.
- `crates/octofhir-server/src/handlers.rs:2700-2740`: avoid `serde_json::to_value(bundle)` for search bundles; keep typed/RawJson bundle fast path unless projection is requested.
- `crates/octofhir-server/src/handlers.rs:2744-2801`: remove repeated `Value::clone` of meta/text/elements where a borrowed serializer can write selected fields.
- `crates/octofhir-server/src/handlers.rs:2469-2492`: replace `HashMap<String,String> -> Vec<String> -> join -> parse` with direct form pair parsing.
- `crates/octofhir-server/src/handlers.rs:2588-2661`: avoid `all_entries` collection in system search by bounded merge/UNION and direct Bundle entry construction.
- `crates/octofhir-server/src/handlers.rs:3613-3700`: avoid whole-entry clones in transaction; keep original indexes and only own mutated resources.
- `crates/octofhir-server/src/handlers.rs:4933-4940`: change `resolve_bundle_references` to mutate an owned resource only when caller already owns it; avoid clone helper for read-only cases.
- `crates/octofhir-storage/src/evented.rs:334-371`: skip event `Value` clone when no subscribers; consider event payload `Arc<Value>` or raw payload for subscribers.
- `crates/octofhir-search/src/params_converter.rs:228-239`: reduce `SqlValue` string clones by making builder params move into final query or use `Cow<'a, str>` where API boundaries allow.
- `crates/octofhir-db-postgres/src/queries/search.rs:213-220`: avoid per-search `SearchParams` clone and empty registry `Arc` allocation in production path.
- Validation: `ValidationService::validate` currently builds a new profile `Vec<String>` per call (local inspection in `crates/octofhir-server/src/validation.rs:187-204`). Change validator API or cache profile lists per resource type.

Measurement:
- Add `dhat`/allocator-count benchmark mode for CRUD/search/Bundle.
- On macOS, use `samply record` and Instruments Allocations for end-to-end HTTP workloads.
- Save allocated bytes/op, allocations/op and p95/p99 latency before and after each allocation PR.

## Database Performance Plan

SQL/query shape:
- Special-case `_lastUpdated` sort/filter to `updated_at`; verify generated SQL and `EXPLAIN (ANALYZE, BUFFERS)`.
- Bind LIMIT/OFFSET instead of baking pagination into cached SQL shape where feasible.
- Remove dead query construction in non-raw search path if still present (`execute_query` locally builds an unused `sqlx_query` before using `bind_all_params`).
- For system search, move from serial per-type queries to bounded concurrent queries, then to UNION ALL with global order/limit.

Indexes:
- Add include-specific partial index:
  `CREATE INDEX ... ON search_idx_reference(resource_type,param_code,resource_id,target_type,target_id) WHERE ref_kind=1`.
- Keep current revinclude index `(target_type,target_id,resource_type,param_code)`; validate it with EXPLAIN for `Patient?_revinclude=Observation:subject`.
- Add token index table for `identifier`, `code`, `status`, `category`, `_tag`, `_security`.
- Add string index table for normalized Patient/Practitioner names and common text fields; define exact/contains semantics explicitly.
- Add composite history indexes: `(id, updated_at DESC)` for instance history and `(updated_at DESC, id)` for type history. Current schema has separate `updated_at` and `id` indexes only.
- Review idempotent delete CTE: current delete creates `_transaction` row even when no resource row is updated (assumption from CTE shape in `queries/crud.rs:602-635`; verify with DB test). Avoid transaction-table bloat for repeated deletes.

Connection pool/transactions:
- Add histograms for pool acquire wait, DB query duration, rows returned and query kind.
- Conditional create/update/delete should not rely on stale async indexes for uniqueness decisions. Use authoritative indexed SQL inside the write transaction or enforce unique business indexes where configured.
- Bundle transaction should keep search-index writes inside transaction, as it currently does, but reduce `Mutex`/dyn overhead after correctness fixes.

Batch behavior:
- Keep `create_batch_with_tx` UNNEST approach, but remove resource clones before batch insert where possible.
- Add batch size metrics and max batch guardrails.
- For async index writer, expose queue depth, batch size, flush duration, failures and lag. Define backpressure behavior when queue is full.

Prepared statements/query caching:
- PostgreSQL dynamic table names limit prepared statement reuse. Cache per resource type/query shape, not per pagination.
- Track query cache hit/miss/build time. A cache that still rebuilds `ConvertedQuery` should be treated as partial.

## FHIR Semantics Risk Plan

Spec references used:
- Conditional create/search behavior: [HL7 FHIR R4 HTTP conditional create](https://www.hl7.org/fhir/R4/http.html#ccreate).
- POST search repeated params and GET/POST equivalence: [HL7 FHIR R4 HTTP search](https://www.hl7.org/fhir/R4/http.html#search).
- Transaction/batch response order: [HL7 FHIR R4 HTTP transaction response](https://www.hl7.org/fhir/R4/http.html#transaction-response).
- Bundle request/response entry rules: [HL7 FHIR R4 Bundle](https://www.hl7.org/fhir/R4/bundle.html).
- Include/revinclude and `:iterate`: [HL7 FHIR R4 Search include](https://www.hl7.org/fhir/R4/search.html#include).
- `_summary`/`_elements`/SUBSETTED: [HL7 FHIR R4 Search summary and elements](https://www.hl7.org/fhir/R4/search.html#summary).
- CapabilityStatement conditional/search fields: [HL7 FHIR R4 CapabilityStatement definitions](https://hl7.org/fhir/R4/capabilitystatement-definitions.html).

Risks to resolve before performance tuning changes semantics:
- Conditional create must not create if equivalence search fails.
- Conditional update must update the matched resource id, not whatever id is in payload.
- Transaction-response and batch-response entries must align to request order.
- Batch entries must remain independent; transaction fullUrl reference behavior must not leak into batch.
- `_include:iterate`/`_revinclude:iterate` should either be implemented with caps or not advertised as supported.
- `_summary=true` should use `ElementDefinition.isSummary` fields or safely return full resources; current tiny subset is not enough.
- `_elements` should include mandatory and modifier elements where required and mark incomplete resources with SUBSETTED.
- CapabilityStatement should not advertise conditional/search/include capabilities until tests prove the exact behavior.
- Search POST must preserve repeated params and merge URL/body params.
- History pagination should use `_offset` consistently if the server supports offset-style paging.

## Benchmark & Profiling Plan

Common setup:
- Dataset: generated realistic R4/R4B data with at least 1M Patient, 5M Observation, 1M Encounter, references Patient/Encounter/Practitioner, identifiers and codes distributed with realistic cardinality.
- Baseline metadata: commit SHA, Rust version, target triple, allocator, DB version, DB config, pool size, CPU, memory, dataset cardinality, enabled validation/index consistency mode.
- Metrics to save: RPS, p50/p95/p99/max latency, error rate, allocated bytes/op, allocations/op, DB query duration p50/p95/p99, pool acquire wait, rows scanned/returned, shared/local blocks, temp files, serialization duration.
- Regression thresholds: p95 +10%, p99 +15%, allocated bytes/op +10%, DB shared read blocks +20%, or planner switching from index scan to seq scan on benchmarked queries requires review.

Single resource read:
- Measure: `GET /fhir/Patient/{id}` uncached/cached, normal and `_summary=true`.
- Run: k6 or `oha` against local server; add Criterion handler/storage benchmark for `read_raw`.
- Baseline: HTTP latency, DB time, serialization time, resource cache hit rate.
- Threshold: p95 +10%, alloc +10%.

Create/update:
- Measure: `POST /Patient`, `PUT /Patient/{id}`, conditional create/update, `Prefer: return=minimal|representation`.
- Run: k6 CRUD recipe plus Criterion storage bench for `create_raw`/`update_raw`.
- Baseline: validation time, DB write time, index enqueue/flush time, allocated bytes.
- Threshold: p95 +10%, index lag > configured SLO, duplicate conditional creates = 0 tolerance.

Search by common params:
- Measure: `Patient?family=`, `Patient?identifier=system|value`, `Observation?code=system|code`, `Observation?subject=Patient/{id}`, `_sort=-_lastUpdated`, `_total=accurate`.
- Run: k6 search recipe plus DB-only generated SQL `EXPLAIN (ANALYZE, BUFFERS)`.
- Baseline: SQL build time, main query time, count query time, rows scanned/returned.
- Threshold: seq scan on large table for indexed param requires review.

Bundle transaction:
- Measure: transaction with 10 and 100 entries, UUID fullUrl refs, conditional creates, mixed methods.
- Run: k6 transaction recipe and Criterion for `process_transaction` with mocked storage where possible.
- Baseline: bundle parse time, pre-scan time, DB transaction time, allocation bytes, response serialization, transaction rollback error rate.
- Threshold: p99 +15%, allocated bytes +10%, response order mismatch = 0 tolerance.

Include/revinclude:
- Measure: `Observation?subject=Patient/{id}&_include=Observation:subject`, `Patient?_revinclude=Observation:subject`, `:iterate` graph with depth cap.
- Run: DB-only include query benches and HTTP k6.
- Baseline: included count, iterations, include query duration, DB buffers, dedupe count.
- Threshold: include query p95 +15%, unbounded included count disallowed.

JSON serialization/deserialization:
- Measure: parse/write full resources at 10 KB and 100 KB; Bundle searchset with 10/50/100 entries; `_summary` projections.
- Run: Criterion bench using `serde_json::from_slice`, `to_writer`, `RawJson` Bundle serialization and projection helper.
- Baseline: bytes/op, allocs/op, ns/op.
- Threshold: alloc +10% or ns/op +10%.

DB-only query benchmarks:
- Measure: generated SQL for CRUD read, default search, token/string/reference/date search, history, include/revinclude, count.
- Run: `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)` saved under `benchmark-results/db-plans/{commit}`.
- Baseline: plan type, actual rows, buffers, temp usage, planning/execution time.
- Threshold: plan regression from index scan to seq scan or sort spill requires review.

Profiling:
- CPU flamegraph: `cargo flamegraph` on local load profile after server binary starts.
- macOS profile: `samply record -- cargo run --bin octofhir-server` or profile external process while k6 runs.
- Binary/code size: `cargo bloat -p octofhir-server --release --crates` after release build.
- Allocation: `dhat` feature for microbenches, Instruments Allocations for end-to-end HTTP.

## Refactoring Roadmap

P0: safe/high ROI:
- Fix conditional update matched-id handling.
- Fix transaction-response original order while keeping sorted execution.
- Fail conditional create when equivalence search fails.
- Wire `create_raw`/`update_raw` to raw SQL helpers.
- Fix history `_offset` rename with compatibility if needed.
- Make plain system search avoid `apply_result_params` when no result params.
- Add tests for conditional create/update, transaction response order, batch reference isolation and POST `_search` repeated params.

P1: structural improvements:
- Introduce resource projection helper for `_summary`/`_elements` with correct summary metadata.
- Refactor POST `_search` extractor to preserve repeated params and URL/body merge.
- Special-case `_lastUpdated` to `updated_at`; add plan tests.
- Add include-specific search index and bounded `:iterate`.
- Add token/string search index tables for top clinical params.
- Make async search-index consistency configurable and observable.
- Reduce Bundle transaction clones and event clones.
- Add operation-level spans and histograms.
- Fix workspace-wide `cargo check --all-targets --all-features`.

P2: longer-term architecture:
- PostgreSQL-native transaction fast path without `Mutex<Option<Box<PgTransaction>>>` in Bundle handling.
- Query cache redesign around normalized query shape and bindable pagination.
- UNION-based system search with global order/pagination.
- Generated per-resource summary projection from canonical StructureDefinitions.
- Configurable uniqueness/index policies for conditional writes by business identifier.
- Broader DB plan regression harness in CI with representative seeded data.

## Non-goals

- No rewrite of the server or storage layer.
- No replacement of Axum/Tokio/sqlx.
- No premature removal of `serde_json::Value` from all APIs; target hot paths first.
- No speculative unsafe code for JSON projection or SQL binding.
- No broad FHIR feature expansion before conditional/search/Bundle correctness gaps are fixed.
- No benchmark claims without saved baseline data and reproducible commands.
- No high-cardinality metrics labels containing resource ids, patient identifiers, raw search strings or PHI.
