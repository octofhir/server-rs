# Read-path and Rust runtime performance for OctoFHIR

Scope: cut per-request CPU and latency on read/search, and tune the runtime and binary. Every code claim cites `file:line` in `/Users/alexanderstreltsov/work/octofhir/server-rs` (repo at commit 536cfc2, 2026-10-30). Web claims cite URLs. The codegraph MCP server was unavailable, so code was located with grep.

## Q1. Zero-copy JSON from Postgres to the HTTP body. Does OctoFHIR parse into serde_json::Value on reads and searches? Where, and how many copies?

### Takeaway
Most of the zero-copy work is already done. Reads and searches select `resource::text`, store versionId and lastUpdated inside the JSONB at write time, and build search Bundles from `Box<RawValue>`. Five per-request costs remain:
1. A full `String` clone of the body on every cached read.
2. A full JSON validation pass per search entry (`RawValue::from_string`).
3. A full `serde_json::Value` parse whenever an `AuthorizedRead` extension is present.
4. A full Value round-trip of the whole Bundle when `_summary` or `_elements` is used.
5. Axum's `Json` writing into a 128-byte buffer that grows by repeated reallocation.

All five can be removed in safe Rust.

### Cited Findings (code)
- **Read SQL.** `read_raw` runs `SELECT id, txid, created_at, updated_at, resource::text, status::text FROM "{table}" WHERE id = $1` and decodes into `(String, i64, DateTime, DateTime, String, String)`. No `Value` is built. See `crates/octofhir-db-postgres/src/queries/crud.rs:752-766`. The doc comment says "The resource JSON in the DB already contains correct meta (versionId, lastUpdated) from create/update operations" (`crud.rs:749-750`), so meta is stored, not spliced in.
- **Raw text elsewhere.** Create uses `RETURNING ... resource::text` (`crud.rs:188`). vread uses `resource::text` from the live table UNION the history table (`crud.rs:1032-1040`). History uses `resource::text` (`queries/history.rs:316,335,460,609`). Search "SQL already emits resource::text" (`queries/search.rs:1288,1464,1471,1731,2011`).
- **Read handler (`crates/octofhir-server/src/handlers.rs:1525-1621`):**
  - It checks the moka resource cache first and falls back to `read_raw` (`1533-1571`).
  - If an `AuthorizedRead` extension exists, it parses the full body, `serde_json::from_str::<Value>(&stored.resource_json)`, and runs `authorization.prepare(..).authorize(&state, &body)` (`1575-1583`).
  - The 304 check `snapshot_not_modified` comes after authorization (`1587-1589`). Authorization therefore runs before any 304 is returned, which is correct.
  - The body is built with `Body::from(stored.resource_json.clone())` (`1607-1610`). `stored` is an `Arc<RawStoredResource>` with a `String` field, so every read does a full heap copy of the resource (cache hit or miss).
- **When `AuthorizedRead` is inserted.** It is added for GET/HEAD on a registered `/fhir/{type}/{id}` read or vread path (`crates/octofhir-server/src/authorization.rs:657-675`). `PreparedRead::authorize` opens a DB transaction only when `constrained`, that is, when a compartment or visibility constraint exists (`authorization.rs:209-240`). Otherwise it evaluates the policy against the parsed `body`.
- **`_summary` / `_elements` on a single read.** `apply_result_params_to_resource` does `from_str` to Value, applies the filter, then `to_vec` (`handlers.rs:1390-1414`). This is a full parse plus reserialize, used only when those params are present.
- **Search handler (`handlers.rs:3009-3058`):**
  - Each entry becomes `octofhir_api::RawJson::from_string(e.resource_json)` (`3014`), and `_include` entries are handled the same way (`3022`).
  - `RawJson` is `Arc<Box<RawValue>>` (`crates/octofhir-api/src/lib.rs:20`). `from_string` calls `RawValue::from_string(s).expect("valid JSON")` (`lib.rs:25-28`), which validates the whole document with serde_json's parser. That is a full scan per entry, even though no tree is built.
  - Serialization "is zero-copy" (`lib.rs:84-92`) in the sense that it writes the raw text into the output, which is one memcpy into the response buffer.
  - The Bundle is returned through `Json(bundle)` (`handlers.rs:3058`).
  - If `_summary` or `_elements` is present, `apply_result_params(bundle, &params)` converts the entire Bundle to a Value (`3051-3054`).
- **Axum `Json` buffer.** Axum's `Json<T>` response does `BytesMut::with_capacity(128).writer()` and then `serde_json::to_writer` ([axum json.rs](https://raw.githubusercontent.com/tokio-rs/axum/main/axum/src/json.rs)). A 20-entry Bundle of several KB per resource therefore grows by repeated doubling reallocation and copying.
- **Other per-request parses of `resource_json`:**
  - vread with authorization (`handlers.rs:1638`).
  - The history authorization filter, per row (`authorization.rs:138`).
  - Operation context (`operations/context.rs:608`).
  - Authorization snapshots (`authorization/snapshots.rs:149`).

### Cited Findings (web)
- PostgreSQL docs on storage formats: "`json` ... stores an exact copy of the input text, which processing functions must reparse on each execution; while `jsonb` data is stored in a decomposed binary format that makes it slightly slower to input ... but significantly faster to process." They also say `jsonb` "does not preserve white space, does not preserve the order of object keys, and does not keep duplicate object keys" — [PostgreSQL JSON types](https://www.postgresql.org/docs/current/datatype-json.html).
  - Consequence: `resource::text` output is canonicalized by `jsonb_out`. Key order is the jsonb order (shorter keys first), not the client's order. That is allowed for FHIR JSON, but it means the stored text is not byte-identical to what was submitted.
- On numbers, the same page says `jsonb` prints numbers "according to the behavior of the underlying `numeric` type", so `1.230e-5` comes back as `0.00001230`. This is relevant if you ever compare bytes or hash bodies — [PostgreSQL JSON types](https://www.postgresql.org/docs/current/datatype-json.html).
- I found no published benchmark that isolates `jsonb_out` (`jsonb::text`) cost against storing a parallel `json`/`text` column. Searches returned only general json-vs-jsonb blog posts ([dbvis](https://www.dbvis.com/thetable/json-vs-jsonb-in-postgresql-a-complete-comparison/), [cybrosys](https://www.cybrosys.com/research-and-development/postgres/jsonb-vs-text-in-postgresql-which-performs-better)). The Cybrosys write-up reports JSONB and text performing "nearly identically at moderate data sizes" and attributes this to memory bandwidth. Treat it as low-quality evidence.
- On how Aidbox assembles Bundles: its docs confirm only that each resource is stored in a single JSONB column and queried directly ([Aidbox DB overview](https://www.health-samurai.io/docs/aidbox/database/overview)). I found no public source showing that Aidbox builds Bundles with `json_agg`/`jsonb_build_object` in SQL.
- A PostgreSQL mailing-list report found `jsonb_object_agg()` slower than `to_jsonb()`/`jsonb_build_object()` for single-row input ([pgsql thread](https://www.postgresql.org/message-id/CAL2enjK1KXy54miYKt_Myg9NYo1Tg4e7WQaNsVu0ufqudpwn7Q%40mail.gmail.com)). This is old (PG 9.5/9.6) and only tangentially relevant.

### Inferences
- **Remove the clone on reads.** Change `RawStoredResource.resource_json` (`crates/octofhir-storage/src/types.rs:51`) from `String` to `bytes::Bytes`, or store `Bytes` alongside it in the cache entry. Then `Body::from(bytes.clone())` is a refcount bump instead of a memcpy plus malloc/free of the whole resource. This is safe Rust; `Bytes::from(String)` reuses the allocation. Expected gain: one alloc plus one memcpy per read. Small per request, but it is on every cache hit, where it is likely the dominant remaining CPU besides HTTP framing.
- **Write search Bundles as bytes.** Instead of `RawJson::from_string` per entry followed by `Json(bundle)`, write the Bundle into a single pre-sized `Vec<u8>`/`BytesMut`:
  - capacity = sum of entry lengths plus about 200 bytes per entry plus the header;
  - emit the static prefix (`{"resourceType":"Bundle","type":"searchset","total":N,"link":[...],"entry":[`);
  - for each entry, write `{"fullUrl":"...","resource":` + raw text + `,"search":{"mode":"match"}}`;
  - close the arrays.

  This removes one validation parse per entry (the input is PostgreSQL `jsonb_out` output, which is valid JSON by construction) and the 128-byte-start growth. It needs no `unsafe`; plain byte concatenation of trusted text is enough. Keep `RawJson::from_string` for anything that does not come straight from `jsonb::text`.
- **A cheaper fallback.** If byte-building is too invasive, implement `IntoResponse` for the Bundle with `serde_json::to_writer` into `Vec::with_capacity(estimated)`. Without `unsafe`, RawValue validation cannot be skipped (`RawValue::from_string` always validates; serde_json has no public unchecked constructor), so the byte-builder is the only safe way to drop the validation pass.
- **Parse lazily in authorization.** On reads with `AuthorizedRead`, only parse the body if the effective policy needs resource content. The pattern used for search (`constrained`) could extend to read: if the policy result is decidable from scopes, request, and user alone, skip `from_str::<Value>`. Whether the benchmark config (`ANONYMOUS_ACCESS=true`, `DEFAULT_DENY=false`) takes this path is unverified; see Gaps.
- **SQL-side Bundle assembly** (`SELECT jsonb_build_object('resourceType','Bundle','entry', jsonb_agg(...))::text`) moves CPU from Rust into the PostgreSQL backend. PG would build a new jsonb tree (`jsonb_agg`) and run `jsonb_out` over the whole Bundle, which is roughly the same text-generation cost as today plus aggregate/tree-building cost. On a CPU-limited DB container this shifts load to the component that is harder to scale. Rust-side concatenation of `resource::text` rows looks strictly cheaper; this is inference, with no published measurement found. One variant could help: `json_agg(resource)` over `json` would not re-parse, but would still pay `jsonb_out` per row.
- **Experiment to measure `jsonb_out` cost.** On the benchmark DB, run `EXPLAIN (ANALYZE, BUFFERS)` comparing `SELECT resource FROM patient LIMIT 1000` (binary jsonb to sqlx) with `SELECT resource::text FROM patient LIMIT 1000`, and add a `pg_stat_statements` timing comparison. Also try sqlx's binary-protocol `Json<Box<RawValue>>` decoding of jsonb. In binary format, jsonb is sent as a version byte (1) followed by the text, which PG produces with the same `jsonb_out`, so the PG cost should be identical. This is from knowledge of PG's `jsonb_send`, not fetched in this session.

### Gaps
- No public measurement of `jsonb_out` cost per KB, or of jsonb vs json/text read throughput in PG 17/18.
- No primary source on how Aidbox builds search Bundles (SQL-side vs JVM-side).
- Not verified: whether the benchmark configuration (anonymous access, default allow) inserts `AuthorizedRead` and thus triggers the per-read Value parse at `handlers.rs:1577`. Check with a span or counter, or by reading `auth_middleware` in `crates/octofhir-server/src/authorization.rs` around line 600-680.

## Q2. Response compression (gzip/br/zstd): cost vs benefit, and what k6 sends

### Takeaway
The k6 benchmark asks for gzip on every request, but OctoFHIR has compression off by default and the benchmark does not turn it on. Responses are therefore uncompressed, which is the right choice for a localhost/docker-network benchmark: gzip spends server CPU to save bandwidth that is not the bottleneck.

### Cited Findings
- The benchmark's k6 `headers()` sends `"Accept-Encoding": "gzip"` and `"Cache-Control": "no-cache"` on every request (`/Users/alexanderstreltsov/work/octofhir/fhir-server-performance-benchmark/k6/util.js:76-88`).
- The search test uses `_count=20` (`fhir-server-performance-benchmark/k6/search.js:42,135`). `_elements=id` is used only in setup `sampleIds` (`search.js:48`).
- OctoFHIR's `ServerConfig.compression` defaults to `false` (`crates/octofhir-server/src/config.rs:219-220,265`). When it is enabled, `CompressionLayer::new().compress_when(DefaultPredicate::new().and(SizeAbove::new(1024)))` is used (`crates/octofhir-server/src/server.rs:2329-2337`).
- The workspace enables only `compression-gzip` on tower-http (`Cargo.toml:72`).
- The benchmark compose file sets no `OCTOFHIR__SERVER__COMPRESSION` for octofhir (`fhir-server-performance-benchmark/docker-compose.yaml:142-195`). The container has `cpus: '8'`, `memory: 20G` (`docker-compose.yaml:151-153`).
- tower-http's `CompressionLayer` "uses the `Accept-Encoding` header to pick an appropriate encoding". gzip, deflate, br, and zstd each sit behind a crate feature, and quality is set with `.quality(CompressionLevel)` ([tower-http docs](https://docs.rs/tower-http/latest/tower_http/compression/struct.CompressionLayer.html)). The docs page fetched did not state the default level.

### Inferences
- Keep compression off for the benchmark profile. If it is enabled for real deployments over WAN, prefer `zstd` at a low level, or gzip with `CompressionLevel::Fastest`, and keep the 1 KB threshold. Brotli at default quality is CPU-expensive for dynamic content. This is general knowledge, not measured here.
- Comparing servers that do compress against OctoFHIR, which doesn't, is a real asymmetry: those servers pay CPU, and k6 pays decompression CPU for them.
- Experiment: run the k6 read and search scripts twice, with `OCTOFHIR__SERVER__COMPRESSION=true` and `false`, and record RPS and p95 together with server CPU% (docker stats). Expect throughput loss on the docker network.

### Gaps
- No primary benchmark found giving per-KB CPU cost of gzip/zstd/br levels inside tower-http for JSON payloads of 2-50 KB.
- The default `CompressionLevel` was not confirmed from docs.

## Q3. Caching under authorization: ETag/304, per-version immutable cache, and why search caching is risky

### Takeaway
OctoFHIR already authorizes before evaluating `If-None-Match`/`If-Modified-Since`, and it caches only single resources by `(type,id)` with invalidation stamps. A per-version, immutable `(type,id,versionId)` cache, used for vread, is safe by construction. Caching search results is not safe without the authorization context and full write-invalidation in the key, so it should stay out of scope.

### Cited Findings
- **Read ordering.** Authorization (`handlers.rs:1575-1583`), then `snapshot_not_modified` (`handlers.rs:1587-1589`, defined at `handlers.rs:7200-7219`).
  - If `If-None-Match` is present it alone decides, via `octofhir_api::check_if_none_match`. Otherwise `If-Modified-Since` is compared at seconds granularity.
  - The ETag is `W/"{txid}"` (`handlers.rs:1593-1594`).
- **Resource cache** (`crates/octofhir-server/src/cache/resource.rs:4-5`): a bounded moka cache keyed `res:{resource_type}:{id}`, storing `Arc<RawStoredResource>`.
  - It uses a `FillStamp` (generation plus per-stripe revision) captured before the DB read, so "Invalidations during that read make its eventual cache insertion unusable" (`resource.rs:63-80`).
  - It is enabled only when `cfg.cache.resource_ttl_secs > 0` (`crates/octofhir-server/src/server.rs:1617`).
- The benchmark sends `Cache-Control: no-cache` (`fhir-server-performance-benchmark/k6/util.js:86`). That is an HTTP-cache directive; it does not by itself forbid an origin-internal cache. A strict reading is that the origin must produce a fresh response, which the invalidated moka cache does.

### Inferences
- **Vread entries never go stale.** A `(type,id,versionId)` entry is immutable: history rows never change. With authorization still evaluated per request, such a cache cannot serve stale data, so it is the safe cache to add for `_history/{vid}` and for `_include` targets referenced by version.
- **Safe 304 for a different user.** A 304 for user B on a resource user A fetched is safe only because authorization runs first (the current order). Keep that invariant, and add a test that a forbidden user gets 403, not 304, when sending a matching `If-None-Match`.
- **Why search-result caching is risky:**
  1. Results depend on the authorization context (compartment, patient-scope constraint pushed into SQL via `query_visibility`, `authorization.rs:207-219`). The cache key would need the full policy-relevant context.
  2. Any write to any resource of that type, or to referenced types for chained/`_include`/`_has`, invalidates it.
  3. `_total` and paging tokens shift.

  This violates the "never stale or unauthorized" constraint unless invalidation is perfect, so don't do it.

### Gaps
- No external source fetched specifically on FHIR + HTTP caching semantics. RFC 9110/9111 would be the primary reference.

## Q4. Allocators: mimalloc vs jemalloc vs snmalloc vs glibc/musl. Which does OctoFHIR use?

### Takeaway
OctoFHIR already uses mimalloc by default, on glibc (Debian bookworm, not musl), which avoids the known musl-malloc pathology. Further allocator gains are workload-dependent and must be measured. jemalloc with THP is the main candidate experiment, at a known memory cost.

### Cited Findings
- `#[global_allocator] static GLOBAL: mimalloc::MiMalloc` behind the default feature `mimalloc` (`crates/octofhir-server/src/main.rs:3-7`; `crates/octofhir-server/Cargo.toml:13-15,147-148`, `mimalloc = { version = "0.1", default-features = false }`).
- The source comment's "~10-20% improvement" (`main.rs:4`) is unsourced.
- Build image `rust:1.99-slim-bookworm`, runtime `debian:bookworm-slim` (`Dockerfile:35,75`). That is glibc, not musl.
- musl's default allocator is widely reported as slow, especially multithreaded. One report claims up to a 7x slowdown, and a write-up of a Brokk/Bifrost benchmark found musl about 25% slower than glibc even with mimalloc/jemalloc, partly due to memcpy/memcmp ([lobsters discussion](https://lobsters.jakechampion.name/item/78syo9), [feedbagel summary](https://feedbagel.com/post/musl-default-allocator-causes-7x-performance-slowdown-in-rust-applications), [zeli.app](https://zeli.app/de/story/49479826)). These are secondary sources, and the 7x figure could not be verified against the original benchmark.
- In a rust-lang PR thread, one user's hyper/tower server measured about 270k rps vs 260k+ rps between allocators, about a 4% difference (baseline unclear) ([rust-lang/rust#92249](https://github.com/rust-lang/rust/pull/92249)).
- The Rust perf book: alternative allocators give "large improvements in runtime speed and large reductions in memory usage ... in practice", varying by program and platform ([perf-book build configuration](https://nnethercote.github.io/perf-book/build-configuration.html)).
- In rustc, enabling jemalloc transparent huge pages gave "~5% wall-time reduction", "~60% reduction in page faults", and "~15%" average memory increase (up to ~35%). The same footnote says rustc's earlier jemalloc→mimalloc trial gave "~5%" but "up to 35% memory usage increase". THP can be tried without code changes via `MALLOC_CONF="thp:always,metadata_thp:always"` ([Kobzol blog](https://kobzol.github.io/rust/rustc/2023/10/21/make-rust-compiler-5percent-faster.html)).
- The TechEmpower xitca-web entry uses mimalloc (`perf-allocator` feature) ([xitca-web Cargo.toml](https://raw.githubusercontent.com/TechEmpower/FrameworkBenchmarks/master/frameworks/Rust/xitca-web/Cargo.toml)).

### Inferences
- **Experiment matrix** (same k6 read and search runs, 8-CPU container):
  1. mimalloc 0.1 default (current).
  2. mimalloc with `MIMALLOC_ARENA_EAGER_COMMIT=1` / large OS pages via env.
  3. `tikv-jemallocator` with and without `MALLOC_CONF=thp:always,metadata_thp:always`.
  4. glibc malloc with `--no-default-features`.

  Record RPS, p99, and RSS. All of these are safe-Rust swaps; allocator crates contain unsafe internally but are mature.
- Do not move to an Alpine/musl image without a non-musl allocator. Even then, the memcpy finding suggests staying on glibc.

### Gaps
- No primary, reproducible benchmark found comparing mimalloc vs jemalloc vs snmalloc specifically for Tokio/hyper JSON servers.

## Q5. JSON libraries: serde_json vs simd-json vs sonic-rs

### Takeaway
On the hot read and search path OctoFHIR barely parses JSON now (only RawValue validation and the authorization/`_elements` paths). Swapping libraries would help less than removing those parses (Q1). sonic-rs is 1.7-3x faster than serde_json on its own benchmarks, but it is SIMD- and unsafe-heavy. If adopted at all, limit it to the write/validation path, which is out of this scope.

### Cited Findings
- sonic-rs README (Xeon Platinum 8260, self-reported):
  - Deserialize to struct, twitter: sonic_rs `from_slice` 827.74 µs, simd_json 1.0872 ms, serde_json `from_str` 1.3842 ms.
  - Untyped DOM, twitter: sonic_rs 556.10 µs, simd_json borrowed 1.1951 ms, serde_json `from_str` 2.8960 ms.
  - Serialize untyped, twitter: sonic_rs 390.00 µs vs serde_json 797.34 µs. On canada, serde_json is faster (6.5747 ms vs 6.7082 ms untyped; 4.6281 ms vs 4.7481 ms struct).
  - The `_unchecked` APIs skip UTF-8 validation and are called inside `unsafe` blocks.
  - "Faster in x86_64 or aarch64, other architecture is fallback and maybe very slower", and it recommends `-C target-cpu=native`.
  - Source: [sonic-rs README](https://github.com/cloudwego/sonic-rs).
- RUSTSEC-2023-0087 ("unsound", INFO): `simd-json-derive` misused `MaybeUninit::uninit().assume_init()`, fixed in 0.12.0 ([RustSec](https://rustsec.org/advisories/RUSTSEC-2023-0087)). No advisory was found for core `simd-json` in the search results.
- TechEmpower xitca-web uses `sonic-rs 0.5.6` under the `perf-json` feature, with `serde_json` as the default `json` feature ([xitca-web Cargo.toml](https://raw.githubusercontent.com/TechEmpower/FrameworkBenchmarks/master/frameworks/Rust/xitca-web/Cargo.toml)).

### Inferences
- **sonic-rs gains depend on SIMD.** For the read path, the only parse left in the benchmark config is possibly the `AuthorizedRead` Value parse. Removing it (Q1) beats any library swap. sonic-rs's advantage depends on SIMD target features, which conflicts with a portable `x86-64` image (see Q7).
- **Unsafe exposure.** Under the "no unsafe without proven need" rule, sonic-rs and simd-json add large unsafe surfaces (SIMD intrinsics, unchecked UTF-8) and a different `Value` type. serde_json is the conservative default. If evaluated, use only the checked APIs (`from_slice`, `get_from_str`) and run the fuzz/miri history check first.

### Gaps
- The json-benchmark (serde-rs) repo numbers were not fetched. No independent (non-vendor) confirmation of the sonic-rs figures.
- The soundness history of `sonic-rs` was not checked against RustSec.

## Q6. Tokio, hyper/axum, tower middleware, tracing, and lock contention

### Takeaway
- The runtime is `#[tokio::main]` with defaults. Worker count comes from `available_parallelism`, which honors cgroup quotas, so `cpus: '8'` should give 8 workers.
- `axum::serve` is used without setting TCP_NODELAY.
- Each non-noisy request creates an info span that formats the full URI, clones the method and URI, allocates the method and path as strings, and does `metrics` label lookups with owned Strings.
- These are cheap individually but are fixed per-request overhead worth measuring with a flamegraph.

### Cited Findings
- **Runtime.** `#[tokio::main] async fn main()` (`crates/octofhir-server/src/main.rs:49`). No custom `Builder`, no `worker_threads`. Tokio's `worker_threads` default "is determined by calling `std::thread::available_parallelism`" and can be overridden by env `TOKIO_WORKER_THREADS`. `event_interval` defaults to `61`, `max_blocking_threads` to 512, `thread_keep_alive` to 10 s, and `disable_lifo_slot` requires `tokio_unstable` ([tokio Builder docs](https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html)).
- **`available_parallelism` on Linux.** It "may overcount the amount of parallelism available when limited by a process-wide affinity mask or cgroup quotas and `sched_getaffinity()` or cgroup fs can't be queried, e.g. due to sandboxing". This implies it does read cgroup quotas when it can. It "should not be called from hot code" because it is recomputed each call ([std docs](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html)).
- **Server.** `axum::serve(listener, self.app).with_graceful_shutdown(...)` (`crates/octofhir-server/src/server.rs:2445-2453`). Axum's `ListenerExt::tap_io` doc example explicitly calls `tcp_stream.set_nodelay(true)` per accepted connection ([axum ListenerExt](https://docs.rs/axum/latest/axum/serve/trait.ListenerExt.html)). The docs did not state the default.
- **Middleware stack** (outer to inner): request_timeout → DefaultBodyLimit → trace_metrics → dynamic_cors → [compression] → auth → audit → handler (`server.rs:2311-2357`). The comment notes it was reduced "6 layers total (down from 10)" (`server.rs:2312-2315`).
- **`trace_metrics_middleware`** (`crates/octofhir-server/src/middleware.rs:550-629`):
  - `method.clone()`, `uri.clone()`, and an `x-request-id` `format!` (`563-572`).
  - `tracing::info_span!("http.request", http.method=%method, http.target=%uri, ...)` (`575-581`).
  - `method.to_string()` and `uri.path().to_string()` (`583-584`).
  - A debug event on success (`609-614`).
  - `metrics::record_http_request` (`crates/octofhir-server/src/metrics.rs:80-105`), which normalizes the path and creates `counter!`/`histogram!` with owned `String` labels (`method_owned.clone()`, `normalized_path.clone()`, `status_class.to_string()`) per request.
- **Tracing.** The workspace sets `tracing = { features = ["release_max_level_info"] }` (`Cargo.toml:90`), so debug and trace events compile out of release builds. An OpenTelemetry layer exists (`crates/octofhir-server/src/observability.rs:10,146`). The benchmark sets `OCTOFHIR__LOGGING__LEVEL: 'info'` and `RUST_LOG: 'info'` (`fhir-server-performance-benchmark/docker-compose.yaml:192` region).
- **Read-mostly state.** `arc_swap` is already used in `crates/octofhir-search/src/registry.rs`, `crates/octofhir-search/src/reloadable.rs`, `crates/octofhir-server/src/canonical.rs`, and `crates/octofhir-server/src/server.rs` (grep). `DashMap` appears 61 times in octofhir-server; RwLock or parking_lot appear 8 times.
- The resource cache uses `moka::future::Cache` (`cache/resource.rs:15`) plus `SeqCst` atomics on every `begin_fill`/`is_current` (`resource.rs:65-80`).

### Inferences
- **TCP_NODELAY experiment.** Add `listener.tap_io(|s| { let _ = s.set_nodelay(true); })` and compare p50 latency for small responses. This is safe; if the default is already on, the change is a no-op.
- **Leaner per-request middleware:**
  - Use `http.target = %uri.path()`, or record the target only at debug.
  - Reuse `&'static str` for the method (`Method::as_str()` gives `&str`; the label could use `SharedString::const_str` for the standard methods).
  - Cache `metrics::Counter`/`Histogram` handles per (method, route template, status class) in a small `DashMap` or a static array, instead of rebuilding label sets.

  The `metrics` crate's label construction plus registry hash lookup on every call is a known cost (inference; not measured here). Measure with `samply` against the `profiling` profile (`Cargo.toml` `[profile.profiling]`).
- **Span or log line, not both.** If no OTel exporter is configured in the benchmark, the `info_span!` plus fmt layer still allocates for `%uri` and `%req_id` formatting. Gating the span to only when OTel or debug is enabled could save a few µs per request. That needs a flamegraph to confirm.
- **Tokio knobs.** `event_interval`, `global_queue_interval`, and LIFO tweaks are second-order for a DB-bound server. Only consider them after the profile shows scheduler time. Pinning `worker_threads=8` explicitly (or setting `TOKIO_WORKER_THREADS=8` in compose) removes any dependence on cgroup detection. Verify the actual count at startup by logging `tokio::runtime::Handle::current().metrics().num_workers()`.
- **SeqCst atomics.** The `SeqCst` loads in the moka resource cache could be `Acquire`/`Release`, but only after careful reasoning about the fill/invalidate race. The gain is tiny on x86 (loads are plain moves). Low priority.

### Gaps
- hyper 1.x HTTP/1 defaults (writev auto, `max_buf_size`, `pipeline_flush`, keep-alive) and whether `axum::serve` sets TCP_NODELAY by default were not confirmed from source in this session.
- No primary measurement found of tower/axum `from_fn` layer overhead per request.

## Q7. Build: LTO, codegen-units, panic=abort, PGO/BOLT, target-cpu

### Takeaway
`Cargo.toml` already has fat LTO, `codegen-units=1`, and `panic=abort`. The Dockerfile overrides this to thin LTO with 16 codegen units, so the published image (which the benchmark pulls) is not built with the tuned profile. Restoring fat LTO with CGU=1 in a dedicated benchmark image, then adding PGO, is the cheapest build-level win.

### Cited Findings
- Workspace `[profile.release]` sets `lto = true`, `codegen-units = 1`, `panic = "abort"`, `debug = 1` (`Cargo.toml` `[profile.release]`). A `profiling` profile with thin LTO and 16 CGUs also exists.
- The Dockerfile builds with `CARGO_PROFILE_RELEASE_LTO=thin`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, `CARGO_PROFILE_RELEASE_DEBUG=0` and `cargo build --release --locked --bin octofhir-server` (`Dockerfile:66-69`). There are no `RUSTFLAGS` or `target-cpu`, and no `.cargo/config.toml` in the repo.
- The benchmark compose file uses `image: ghcr.io/octofhir/octofhir-server:latest` (`fhir-server-performance-benchmark/docker-compose.yaml:144`).
- The Rust perf book says:
  - LTO "can improve runtime speed by 10-20% or more". Fat LTO "may improve performance ... further (but not always)" ([rust-lang/rust#103453](https://github.com/rust-lang/rust/pull/103453)).
  - `codegen-units=1` may improve runtime speed.
  - `panic="abort"` "might ... increase runtime speed slightly".
  - PGO "can improve runtime speed by 10% or more".
  - `target-cpu=native` helps "especially if the compiler finds vectorization opportunities".
  - Source: [perf-book build configuration](https://nnethercote.github.io/perf-book/build-configuration.html).
- cargo-pgo workflow: `cargo pgo build` → run a representative workload → `cargo pgo optimize`; optionally `cargo pgo bolt build --with-pgo` / `bolt optimize --with-pgo`. It warns that a profile from a "substantially different workload" may "even make your binary slower" ([cargo-pgo](https://github.com/Kobzol/cargo-pgo)). The README gives no numbers.
- The awesome-pgo list links many Rust projects (Vector, Databend, Qdrant, pingora, pydantic-core, serde_json, rustc) but does not summarize percentages in-page ([awesome-pgo](https://github.com/zamazan4ik/awesome-pgo)).
- A PGO evaluation on rathole (Rust tunnel/proxy) found the PGO server binary consistently faster, with client-mode CPU savings of about 0.5-1.5% ([rathole discussion #287](https://github.com/rapiz1/rathole/discussions/287)). This came from search snippets; the numbers were not read in full.
- TechEmpower xitca-web uses `lto = true`, `opt-level = 3`, `codegen-units = 1`, `panic = "abort"` ([xitca-web Cargo.toml](https://raw.githubusercontent.com/TechEmpower/FrameworkBenchmarks/master/frameworks/Rust/xitca-web/Cargo.toml)).

### Inferences
- **Experiment A (fat LTO in the image).** Build the image with the repo's own `[profile.release]` (drop the three `CARGO_PROFILE_RELEASE_*` overrides, or add a `--build-arg PROFILE=bench-release`) and compare against the current image with k6 read and search. Expected: a few percent to low tens of percent CPU per request, per the perf book's 10-20% LTO range. Thin and fat may differ by less. The cost is a much longer docker build.
- **Experiment B (PGO).** Use `cargo pgo build`, run k6 `crud.js` + `search.js` + `bench.js` against the instrumented binary for several minutes, then run `cargo pgo optimize`. The k6 benchmark is a representative workload by definition, so PGO risk is low. Then try `bolt optimize --with-pgo` on x86_64 Linux only. PGO requires `llvm-profdata` matching rustc's LLVM (llvm-tools-preview).
- **Experiment C (target-cpu).** Build a separate benchmark image with `RUSTFLAGS="-C target-cpu=x86-64-v3"` (AVX2, Haswell+). Keep the default image at baseline x86-64 for portability. Gains are likely small for this workload (memcpy/serde/hashing, not vectorizable loops) unless sonic-rs is adopted.
- **Symbols.** `debug = 1` in release does not affect codegen speed, only binary size and link time.

### Gaps
- No primary, numeric PGO result for an axum/hyper/sqlx web server was found in this session. The rustc PGO blog post (blog.rust-lang.org/inside-rust/2020/11/11/exploring-pgo-for-the-rust-compiler.html) returned only a redirect notice.
- No measured comparison of thin vs fat LTO for a Tokio web server.

## Q8. What TechEmpower top Rust entries do for DB-backed tests

### Takeaway
The top Rust entries combine fat LTO, CGU=1, panic=abort, mimalloc, an optional SIMD JSON library, and a custom pipelining Postgres driver rather than sqlx. OctoFHIR matches on allocator and profile (except in Docker); the driver choice is the structural difference.

### Cited Findings
- xitca-web (TechEmpower): `xitca-postgres 0.5.0` driver (no sqlx or tokio-postgres), a `zero-copy` row-parsing feature, mimalloc, `sonic-rs` behind `perf-json`, `io-uring`/`compio` runtime options, and fat LTO / CGU=1 / panic=abort ([xitca-web Cargo.toml](https://raw.githubusercontent.com/TechEmpower/FrameworkBenchmarks/master/frameworks/Rust/xitca-web/Cargo.toml)).

### Inferences
- **Most transferable without unsafe or a driver swap:**
  - build-profile parity (Q7);
  - avoiding per-row allocations (`Bytes` bodies, pre-sized Bundle buffers, Q1);
  - fewer per-request allocations in middleware (Q6).
- **Statement pipelining** is the main technique in pipelining drivers. sqlx has no general pipelining. That is a driver-level change, out of scope for safe incremental work.

### Gaps
- may-minihttp, actix, and axum TechEmpower entries were not inspected. The claims about pipelining and prepared statements in TechEmpower DB tests are from general knowledge, not fetched sources. Read `frameworks/Rust/*/src` in TechEmpower/FrameworkBenchmarks to confirm.

## Ranked local experiment list (inference; for the report writer)
Each item is safe Rust, no caching of searches, no `unsafe`.
1. Build the image with fat LTO + CGU=1 (`Dockerfile:66-69`), then PGO via cargo-pgo on the k6 workload. Measure RPS, p95, and CPU%.
2. Make the read body `Bytes` (no `resource_json.clone()`, `handlers.rs:1609`). Write search Bundles into a pre-sized buffer by byte concatenation of `resource::text`, dropping `RawValue::from_string` validation (`handlers.rs:3014,3022`, `octofhir-api/src/lib.rs:25-28`).
3. Confirm whether the benchmark config hits the `AuthorizedRead` full-parse at `handlers.rs:1577`. If so, skip the parse when the policy does not need resource content.
4. Slim `trace_metrics_middleware` (`middleware.rs:550-629`, `metrics.rs:80-105`): fewer clones, cached metric handles, a cheaper span. Set TCP_NODELAY via `tap_io` (`server.rs:2449`).
5. Allocator A/B: mimalloc (current) vs jemalloc+THP vs glibc, tracking RSS.
6. Keep compression off for the benchmark. Confirm with an on/off A/B.
7. Optional: a `(type,id,versionId)` immutable cache for vread/versioned includes, with per-request authorization kept.
