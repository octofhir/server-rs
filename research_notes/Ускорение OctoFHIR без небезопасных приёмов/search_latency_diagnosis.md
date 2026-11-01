# Why OctoFHIR search p99 is ~65 ms for every parameter type: diagnosis

Conventions. A `bench:` citation means `git show octofhir:<path>` in /Users/alexanderstreltsov/work/octofhir/fhir-server-performance-benchmark (branch `octofhir`, last commit f17bfaf). Nothing in that repo was checked out or modified. A `srv:` citation means /Users/alexanderstreltsov/work/octofhir/server-rs at HEAD 536cfc2.

Downloaded data is in /private/tmp/claude-501/-Users-alexanderstreltsov-work-octofhir-server-rs/06e92889-60ab-47e6-873e-0ee75b19ed75/scratchpad/bench_raw/ (treated as untrusted). Extracts:
- /private/tmp/claude-501/-Users-alexanderstreltsov-work-octofhir-server-rs/06e92889-60ab-47e6-873e-0ee75b19ed75/scratchpad/runs_table.txt: every octofhir-branch run with RPS, p99, and Postgres/app CPU.
- /private/tmp/claude-501/-Users-alexanderstreltsov-work-octofhir-server-rs/06e92889-60ab-47e6-873e-0ee75b19ed75/scratchpad/bench_raw/search_timeseries_0910.txt: per-group p99, RPS and CPU time series for run 2026-09-10.

## Q1. Benchmark mechanics: what search sends, what "latency" means, and the 30 VU / 5803 req/s arithmetic

### Takeaway
The published per-parameter "latency" is k6's p99, not the mean. The p99 is computed cumulatively over the whole run, and the published figure is in practice one 5-minute-aligned sample of that gauge. At 30 VUs and 5803 req/s, the mean request time is at most about 5.2 ms. A p99 of about 65 ms on a mean of 5 ms or less means the latency distribution is bimodal: about 95% of requests are fast and a few percent hit a roughly 50–75 ms stall that does not depend on the query.

### Cited Findings
- Search scenario settings:
  - `constant-vus` executor, `vus: 30`, `duration: '2m'`, `discardResponseBodies: true`, no sleep or think time anywhere. — bench:k6/search.js:18,31-32
  - One k6 instance runs all 30 VUs (one `perf-k6` container). — bench:docker-compose.yaml:486-488
- What one VU iteration does:
  - It loops over every searchType, resourceType and parameter in order, issuing one GET per parameter. That is 24 groups: string 3, date 3, token 4, quantity 3, composite 3, reference 8.
  - URL shape: `${baseUrl}/${resourceType}?${name}${modifier}=${prefix}${value}&_count=20`.
  - No `_total`, `_elements`, `_sort` or `_include`. — bench:k6/search.js:43,130-138; bench:k6/searchConfig.js
- Values per parameter:
  - Values are random picks, including a "NON-EXISTS" value for each parameter.
  - Reference values are real ids sampled in `setup()` (`?_count=20&_elements=id`). — bench:k6/search.js:47-110
- Request headers:
  - Every request sends `Accept-Encoding: gzip`, `Accept: application/json` and `Cache-Control: no-cache`.
  - OctoFHIR runs anonymously, with no Authorization header. — bench:k6/util.js:75-89; bench:runner.sh (octofhir case sets only BASE_URL)
- Metrics pipeline:
  - k6 writes Prometheus remote-write with `K6_PROMETHEUS_RW_TREND_STATS='p(95),p(99),min,max'`. — bench:runner.sh:145,148
  - Headline search RPS: `avg_over_time((sum by (fhirimpl)(rate(k6_http_reqs_total{scenario="search"}[1m])))[24h:15s])`. — bench:scripts/generate-snapshot.js:193
  - Per-parameter latency: `sum by (fhirimpl, scenario, group)(avg_over_time(k6_http_req_duration_p99{scenario="search"}[24h:])) * 1000`. The UI labels it "P99 in milliseconds". — bench:scripts/generate-snapshot.js:198-202; bench:scripts/benchmark-converter.js:205-213
- The `sum by` does not add multiple series together:
  - The Grafana snapshot shows exactly one series per group.
  - Labels include `url: 'code-value-quantity'` (equal to the `name` tag), `status: '200'`, `expected_response: 'true'`. — Grafana snapshot API https://grafana.cdr.aidbox.dev/api/snapshots/NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3 (panel "P99")
- The trend sink is cumulative for the whole test:
  - xk6-output-prometheus-remote creates `tsdb: make(map[metrics.TimeSeries]*seriesWithMeasure)` once and adds every sample to it with `swm.Measure.Add(sample)`.
  - `flush()` never clears it, so each `_p99` gauge is the p99 of all samples since the test started. — [xk6-output-prometheus-remote remotewrite.go](https://raw.githubusercontent.com/grafana/xk6-output-prometheus-remote/main/pkg/remotewrite/remotewrite.go)
  - The k6 docs do not say whether stats are cumulative or per interval. The default push interval is 5 s. — [k6 docs: Prometheus remote write](https://grafana.com/docs/k6/latest/results-output/real-time/prometheus-remote-write/)
- In run 2026-09-10, every published p99 equals the gauge value at the 04:50:00 sample:
  - OctoFHIR code-value-quantity: series 65.6, 65.1, 64.9; published 64.9.
  - Aidbox code-value-quantity: series 84.4, 40.4, 39.7; published 84.4. Aidbox was still in its cold first minute at 04:50, so its published p99s in that run are inflated (36–123 ms) compared with its steady state (18–51 ms). — snapshot NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3 vs octofhir/SOURCE_DATA_2026-09-10T04:24:44Z.json
- The OctoFHIR p99 series is flat for the whole run. Per group it stays at 64.5–66.6 ms from the first sample to the last (for example Patient.name 64.6, 65.0, 64.9). Warm-up is not what drives the number. — snapshot NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3
- RPS in run 2026-09-10 (Grafana): OctoFHIR 5778–5832 req/s, Aidbox 3734–3774 req/s. The Aidbox search RPS in that run's SOURCE_DATA came back empty. — same snapshot, panel "RPS"
- Pipeline order:
  - prewarm (10 VUs, 30 s, CRUD), then crud (300 VUs, 4 min), then import, then `vacuumdb --analyze -j 50 --all`, then search.
  - The octofhir-branch CI runs only `SERVERS: octofhir aidbox`. — bench:.drone.yml:148,175-182; bench:k6/prewarm.js; bench:k6/crud.js:20-26

### Inferences
- Little's law gives the mean bound. With 30 VUs in a closed loop and no think time, 30 = X·(R + Z). At X = 5803/s, R + Z = 5.17 ms, so the mean `http_req_duration` R is at most 5.2 ms (Z is k6's own JavaScript and scheduling overhead per request).
- p99 / mean ≥ 12.5 for OctoFHIR. For Aidbox in the main run: mean ≤ 30/3392 = 8.8 ms and p99 21–60 ms, a ratio of about 2.5–7.
- A ratio of about 12 or more with a flat p99 across every query shape fits a bimodal model: about 95% of requests take ~2–3 ms and about 3–5% wait in a ~50–75 ms stall. Check: mean ≈ 0.95·2.5 + 0.05·55 ≈ 5.1 ms, which matches.
- Each group gets about 5803/24 ≈ 240 req/s, so about 29k requests per group per run, and the 1% tail is about 290 requests. Any global stall that hits every in-flight request lifts every group's p99 by the same amount. That is the uniformity seen.
- Aidbox and Medplum are compared on different days and in different runs. OctoFHIR has never appeared in a main-branch run (2026-10-30 main has no octofhir). OctoFHIR's 5803 req/s is the highest search RPS observed. Its problem is p99, not throughput.

### Gaps
- VictoriaMetrics' default subquery step for an instant query without `step` was not checked against VM docs. The claim that the published value equals one 5-minute-aligned sample is inferred from the data matching exactly.
- The raw k6 `http_req_duration` mean and p95 are not in SOURCE_DATA. p95 is pushed (TREND_STATS includes it) but the snapshot does not query it. It needs VM access (vm.cdr.aidbox.dev, auth required).

## Q2. Raw results: search p99 per parameter for OctoFHIR, Aidbox and Medplum, plus container CPU

### Takeaway
In every one of 25 octofhir-branch runs, Postgres sat at its 8-CPU cgroup limit (799–800% max) during OctoFHIR's search phase. OctoFHIR's own container used only 0.1–2.2 cores. OctoFHIR's p99 floor stays at 65–79 ms whether search throughput is 470 or 5803 req/s. The only other servers whose bottleneck container sits at its CPU cap show the same flat ~70–80 ms floor: WSO2 (Postgres pegged) and Medplum (1-CPU replicas). The shared bottleneck is the Postgres container's CPU quota, not OctoFHIR code.

### Cited Findings
- Run 2026-09-10 (octofhir branch), OctoFHIR search p99 by parameter:
  - string 64.9 / 65.0 / 65.3; date 65.6 / 66.2 / 65.9; reference 64.8–65.5
  - composite 64.5–64.9; quantity: value 65.2, combo 85.4, component 98.7
  - token: category 70.9, class 73.4, code 78.8, status 80.8
  - Search RPS 5803. — [octofhir SOURCE_DATA 2026-09-10](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/octofhir/SOURCE_DATA_2026-09-10T04:24:44Z.json)
- Run 2026-09-10, container CPU% (1-minute rate, cAdvisor):

  | Phase | Postgres | OctoFHIR app | Aidbox app |
  |---|---|---|---|
  | OctoFHIR search | 800 (max), ~740–800 | 202–214 | – |
  | Aidbox search | 734–776 | – | 734–748 |
  | OctoFHIR CRUD (11.8k req/s) | 793 max | 444 avg | – |

  — Grafana snapshot NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3, panel "CPU Usage" (`sum(rate(container_cpu_usage_seconds_total{...}[1m])) by (name)*100`)
- All 25 octofhir-branch runs: OctoFHIR median search p99 by run is 65–79 ms (one broken early run excepted). Postgres max CPU is 799–800% in every run. Examples:

  | Run | Search RPS | Median p99 |
  |---|---|---|
  | 2026-06-20T09:52 | 470 | 77 ms |
  | 2026-06-10T14:25 | 2498 | 71 ms |
  | 2026-07-14T17:32 | 4956 | 69 ms |
  | 2026-07-25T19:23 | 5765 | 65 ms |
  | 2026-09-10 | 5803 | 65 ms |

  — /private/tmp/claude-501/-Users-alexanderstreltsov-work-octofhir-server-rs/06e92889-60ab-47e6-873e-0ee75b19ed75/scratchpad/runs_table.txt, built from https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/octofhir/SOURCE_DATA_*.json and the linked Grafana snapshots
- Main run 2026-10-30 (no OctoFHIR):

  | Server | Search RPS | p99 range | Container CPU during search |
  |---|---|---|---|
  | Aidbox | 3392 | 21.2–59.4 ms (most 21–35) | Postgres 788 avg; app 710 |
  | Medplum | 3583 (Grafana 3599) | 72.0–90.5 ms, flat | 8 replicas at ~84–100% of their 1-CPU caps; Postgres 142 |
  | WSO2 | 1878 | mostly 68–80 ms, flat (outliers 178, 506, 1283) | Postgres 800 max |
  | HAPI | 1013 | 69–682 ms | app 790–800 (JVM) |

  — [SOURCE_DATA 2026-10-30](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/SOURCE_DATA_2026-10-30T00:03:50Z.json); Grafana https://grafana.cdr.aidbox.dev/api/snapshots/kX4FlUTLki0a7iSoog4zzvFC20WQRqnV
- Run 2026-07-25 CRUD, a natural control:
  - The OctoFHIR app was at 771% max (near its own 8-CPU cap) with Postgres at 696%.
  - CRUD read p99 was only 18–24 ms.
  - In 2026-09-10 CRUD, Postgres was at 793% and every CRUD operation's p99 jumped to a flat 76–90 ms (reads 76–79 ms).
  — octofhir/SOURCE_DATA_2026-07-25T15:12:45Z.json and 2026-09-10; snapshots jFvY2yVtRyi84FTblc6QLUNpjWGlKVoZ and NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3
- Resource limits:
  - postgres, aidbox and octofhir each get `deploy.resources.limits.cpus: '8'`, a CFS quota, not a cpuset.
  - Medplum: 8 replicas × `cpus: '1'`. — bench:docker-compose.yaml:23,55,152,304-308
- Connection pools:
  - OctoFHIR `POOL_SIZE: '32'`, Aidbox `BOX_DB_POOL_MAXIMUM__POOL__SIZE: 32`, WSO2 `pool_max_conns=32`.
  - Postgres has `max_parallel_workers_per_gather = 4`, `max_parallel_workers = 6`. JIT is not set, so the PG default applies.
  — bench:docker-compose.yaml:164,69,224; bench:infra/postgres/postgres.conf:5-7
- CFS bandwidth behaviour:
  - The default `cpu.cfs_period_us` is 100 ms, and quota is handed out in 5 ms slices.
  - Once a period's quota is used up, the group's threads are throttled until the next period.
  - `cpu.stat` exposes `nr_throttled` and `throttled_time`. — [Linux kernel docs: CFS Bandwidth Control](https://docs.kernel.org/scheduler/sched-bwc.html)
- A container with more parallelism than its quota "will use more cores than its quota for a short period of time and then get throttled". In the worst case that leaves "95ms of throttled time before any thread could be scheduled again", which is "disastrous for tail latency". — [Dan Luu, The container throttling problem](https://danluu.com/cgroup-throttling/)

### Inferences
- Postgres CPU per OctoFHIR search ≈ 8 cores / 5803 = 1.38 ms. Aidbox ≈ 7.7 / 3734 = 2.1 ms. Medplum ≈ 1.42 / 3599 = 0.39 ms (Medplum's cost sits in its app tier instead).
- OctoFHIR's app spends about 0.35 ms CPU per search (2.0 cores / 5800). So OctoFHIR is the cheapest per request in total, and the system is entirely bound by the Postgres quota.
- Because the app is so fast, almost all of each VU's in-flight time is spent in Postgres. Up to 30 backends are runnable at once (pool 32 ≥ 30 VUs) on a host with more than 8 cores.
- Demand is about 2–2.6× the quota. A ~2 ms service time for 30 VUs means about 15k req/s × 1.38 ms ≈ 20 cores wanted. Each 100 ms period therefore looks like this: a burst of roughly 25–40 ms on many cores, then a freeze until the period boundary.
- In this model each VU completes about 18–19 requests per period: most of them fast, plus about one that sits through the freeze. That gives a ~5% stall rate, mean ≈ 5.2 ms and p99 ≈ 60–75 ms, which matches what is observed.
- Aidbox drives Postgres to about 770–790% as well. But its JVM tier takes most of each request's time, so fewer than about 8 backends are runnable at once and bursts above the quota are short. That explains its lower, query-dependent tail.
- tokio sizes its worker pool from `available_parallelism`, which respects the cgroup quota. So the OctoFHIR app cannot exceed its own quota, and it showed no floor at 771% in the 07-25 CRUD run. Postgres runs one process per connection and can exceed its quota.

### Gaps
- No direct throttle counters (`container_cpu_cfs_throttled_periods_total`) are available. cAdvisor is scraped into VM, but the snapshot dashboard does not plot them. This is the decisive confirmation (see Q6).
- The CI host's (hz-cdr) core count is unknown. If the host had exactly 8 cores, quota throttling could not happen and the explanation would shift to plain CPU saturation plus queueing.

## Q3. OctoFHIR search request path end to end, and what is ruled out

### Takeaway
The OctoFHIR request path is short:
- anonymous auth fast path
- no audit
- one SQL statement per search, with no COUNT, no transaction and no SET
- rows passed through as raw text
- a cheap ArcSwap registry snapshot

Nothing in the app explains a uniform 60 ms tail, and the app uses under 2.2 of its 8 CPUs. The levers that matter are database-side: concurrency offered to Postgres, Postgres CPU per query, and JIT/parallel bursts.

### Cited Findings
- Middleware order, outer to inner:
  - request_timeout → DefaultBodyLimit → trace_metrics → CORS → (optional) CompressionLayer → auth → audit → handler. — srv:crates/octofhir-server/src/server.rs:2311-2360
- Compression is off by default (`compression: false`), and the benchmark does not enable it. Responses are uncompressed even though k6 sends `Accept-Encoding: gzip`. — srv:crates/octofhir-server/src/config.rs:265; srv:server.rs:2329-2337; bench:docker-compose.yaml:154-193
- `trace_metrics_middleware`, per request:
  - creates an `info_span!("http.request", http.target=%uri, ...)` and a random request id
  - records metrics and logs at debug level for 2xx
  — srv:crates/octofhir-server/src/middleware.rs:550-628,575
- The handler adds a second span: `#[tracing::instrument(name = "fhir.search")]`. — srv:handlers.rs:2937. OTel is disabled in the image config (`[otel] enabled = false`). — srv:Dockerfile embedded octofhir.toml
- Auth for anonymous requests: with `anonymous_access` on and no credentials, the shared `Arc` anonymous context is inserted and the request proceeds. No token validation, QuickJS or policy evaluation runs on this path. — srv:middleware.rs:936-957; bench:docker-compose.yaml:182-183
- Audit is disabled via `OCTOFHIR__AUDIT__ENABLED: 'false'`. — bench:docker-compose.yaml:185
- Search handler: parse the query string, check visibility (anonymous → Ok), call `execute_authorized_type_search`, build the Bundle from `RawJson` (`RawValue`) entries, serialize with `Json(bundle)`. — srv:crates/octofhir-server/src/handlers.rs:2938-3054,3123-3135
- No COUNT by default:
  - A count query is built only when `total_mode == Accurate`.
  - `total` is otherwise derived from fetching `count+1` rows and `has_more`. — srv:crates/octofhir-db-postgres/src/queries/search.rs:1235-1243,1306-1324; srv:handlers.rs:3583-3603
- One DB round-trip set per search:
  - `execute_query_raw` makes a single `fetch_all(pool)`, decoding `(String, String, i64, DateTime, DateTime)`. The SQL selects `resource::text`, so there is no serde_json Value round-trip.
  - There is no transaction, `SET LOCAL` or statement_timeout on this path. — srv:search.rs:1466-1510
- Statements are unnamed:
  - Search statements are `persistent(false)` because renderers embed literals (quantity bounds, JSONPath) into the SQL text. Each execution is parsed and planned with the actual values.
  - This was introduced in 35fa56b, after the 2026-09-10 image. — srv:search.rs:1245-1249,2250-2259,2402; `git log -S"persistent(false)"`
- Registry snapshot per request is `ArcSwap::from(self.data.load_full())`, with no index copying. — srv:crates/octofhir-search/src/registry.rs:104-111
- Pool settings:
  - `max_connections(pool_size)` (32 in the benchmark), `min_connections = pool_size/4`, `test_before_acquire(false)`, so there is no ping per acquire. — srv:crates/octofhir-db-postgres/src/pool.rs:26-38
  - Searches use `state.read_db_pool`, the primary pool unless `read_replica` is configured. — srv:handlers.rs:3070-3078; srv:server.rs:592-614
- No TCP_NODELAY:
  - The server uses plain `axum::serve(listener, app)`. — srv:server.rs:2449
  - axum 0.8.9 only sets `TCP_NODELAY` in its doc example and tests, via `ListenerExt::tap_io`. — ~/.cargo/registry/.../axum-0.8.9/src/serve/listener.rs:66-88; serve/mod.rs:519-529
- No per-session JIT or parallel settings in production code. Only test fixtures run `SET jit=off; SET max_parallel_workers_per_gather=0`. — srv:crates/octofhir-db-postgres/src/queries/search/revinclude_tests.rs:111; srv:crates/octofhir-db-postgres/tests/reference_traversal_test.rs:1223

### Inferences
- Candidates the code rules out for a uniform ~60 ms tail:
  - auth/QuickJS: never reached
  - audit: disabled
  - `_total` COUNT: not issued
  - pool ping: disabled
  - registry clone: O(1)
  - compression CPU: off
  - k6 series-summing artifact: one series per group
  - warm-up skew: the series is flat
- Tracing spans at info level cost little CPU and cannot produce a 60 ms stall.
- Nagle plus delayed ACK (40 ms on Linux) can produce stalls of about this size. Without compression, though, hyper writes the headers and the full body together, so it is unlikely. It is still worth a cheap A/B test (Q6, H5).

### Gaps
- The exact SQL text and plans for each benchmark parameter were not captured. No OctoFHIR server was running locally, and the local `perf-postgres` (port 13020) has no loaded benchmark dataset: `octofhir2` tables are empty and pg_stat_statements is not installed there.

## Q4. Build, profile and image settings

### Takeaway
The image is a proper optimized build: release opt-level 3, thin LTO, 16 codegen units, debug info off, mimalloc. Build settings do not explain the tail, and since the app is not the bottleneck, they are irrelevant to the 65 ms.

### Cited Findings
- Workspace release profile: `lto = true`, `codegen-units = 1`, `panic = "abort"`, `debug = 1`. — srv:Cargo.toml `[profile.release]`
- The Dockerfile overrides to `CARGO_PROFILE_RELEASE_LTO=thin`, `CODEGEN_UNITS=16`, `DEBUG=0` and runs `cargo build --release --locked`. Base image is rust:1.99 on bookworm. — srv:Dockerfile:66-68
- mimalloc is the default feature and global allocator. — srv:crates/octofhir-server/Cargo.toml:13-15; srv:crates/octofhir-server/src/main.rs:5-7
- The runtime is `#[tokio::main]` with default worker threads, i.e. `available_parallelism`, which follows the cgroup quota. — srv:crates/octofhir-server/src/main.rs:49

### Inferences
- Moving to fat LTO with 1 CGU might save a few percent of app CPU. That is not on the critical path, because Postgres is.

### Gaps
- I did not verify that the `ghcr.io/octofhir/octofhir-server:latest` pulled on 2026-09-10 was built from this Dockerfile. The closest commit on main before the run is 7bd38be (2026-09-10 01:17 +0300).

## Q5. Ranked hypotheses for the uniform ~65 ms p99, with confirmation experiments, fixes and expected gains

### Takeaway
- **H1, near-certain:** Postgres CFS-quota throttling caused by OctoFHIR offering about 30 concurrent backends to an 8-CPU-quota Postgres.
- **H2, high, the throughput lever:** Postgres CPU per search (~1.4 ms) sets both the RPS ceiling and how fast the quota is burned.
- **H3, medium, explains the token/quantity outliers:** PG JIT and parallel query on higher-cost plans.
- **Low:** TCP_NODELAY, k6 artifacts, app-side overhead.

All fixes below are safe: no change to results, writes or auth.

### Cited Findings (evidence for each hypothesis, in rank order)

**H1. Postgres CFS throttling from too many concurrent backends. Likelihood: very high.**
- Evidence:
  - Postgres CPU max is 799–800% in every OctoFHIR search window across 25 runs. — runs_table.txt, from the Grafana snapshots above
  - OctoFHIR's p99 floor is 65–79 ms regardless of RPS (470–5803).
  - Postgres has an 8-CPU CFS quota (bench:docker-compose.yaml:23) and OctoFHIR's pool is 32 (bench:docker-compose.yaml:164), with 30 VUs.
  - The same flat ~70–80 ms floor appears for WSO2 (Postgres pegged) and Medplum (1-CPU replicas), and it is absent from the 07-25 OctoFHIR CRUD run, where the cgroup-sized tokio app reached 771% but Postgres stayed below its cap.
  - Mechanism as described in the [kernel docs](https://docs.kernel.org/scheduler/sched-bwc.html) and by [Dan Luu](https://danluu.com/cgroup-throttling/).
- Confirm:
  - (a) From the existing metrics (needs VM credentials) for the run window: `rate(container_cpu_cfs_throttled_periods_total{name="perf-postgres",runid="2026-09-10T04:24:44Z"}[1m]) / rate(container_cpu_cfs_periods_total{name="perf-postgres",...}[1m])` and `rate(container_cpu_cfs_throttled_seconds_total{name="perf-postgres",...}[1m])`. Expect a ratio well above 0.5 during search. Also plot `k6_http_req_duration_p95` next to p99.
  - (b) Locally on a host with more than 8 cores: bring up the benchmark compose (postgres `cpus: '8'`), load the search dataset, then:

    ```
    docker exec perf-postgres cat /sys/fs/cgroup/cpu.stat
    k6 run k6/search.js   # BASE_URL=http://octofhir:8888/fhir, 30 VUs
    docker exec perf-postgres cat /sys/fs/cgroup/cpu.stat
    ```

    Expect a large delta in `nr_throttled` and `throttled_usec`.
  - (c) A/B `OCTOFHIR__STORAGE__POSTGRES__POOL_SIZE` = 8, 10, 12, 16, 32, one 2-minute search run each, comparing p99, mean, RPS and `nr_throttled`.
  - (d) Control: rerun with postgres pinned via `cpuset: "0-7"` instead of `cpus: '8'`. The floor should disappear even at pool 32 and turn into ordinary queueing.
- Fix (safe, config only):
  - Cap concurrent Postgres work at about Postgres' vCPU count, by setting the octofhir pool to about 8–12 in the benchmark compose.
  - Alternatively, a code-level semaphore or separate search pool sized to DB cores, so CRUD write transactions are not starved. Measure CRUD at 300 VUs too, since the same pool serves writes. CRUD p99 in 09-10 shows the same 76–90 ms floor and should improve as well.
- Expected gain:
  - p99 from about 65 ms down to roughly 8–20 ms: queueing in sqlx's fair FIFO pool instead of 60–75 ms freezes.
  - RPS about the same (quota-bound, ~5.8k), possibly slightly higher from fewer context switches.

**H2. High Postgres CPU per search (~1.38 ms). Likelihood: high, as the cause of saturation and the throughput ceiling.**
- Evidence:
  - Postgres at 800% / 5803 rps, versus Medplum's Postgres at 142% / 3599 rps (≈0.39 ms). — Grafana snapshots above
  - OctoFHIR selects `resource::text` from JSONB, so `jsonb_out` runs for each of up to 21 rows (srv:search.rs:1466-1480).
  - Every statement is unnamed, so it is parsed and planned on every execution (srv:search.rs:2250-2259,2402).
- Confirm:
  - Enable `pg_stat_statements.track_planning = on` (pg_stat_statements is already preloaded: bench:infra/postgres/postgres.conf:10) and run a 2-minute search.
  - Rank `total_exec_time + total_plan_time` by query shape and get `mean_plan_time` vs `mean_exec_time`.
  - Run `perf top -p <backend pid>` to split time between jsonb_out, planner and jsonb path/GIN recheck.
  - A/B a hand-written variant selecting `resource` (binary jsonb) vs `resource::text`.
- Fix (safe): reduce plan cost and per-row work, for example
  - parameterize literals so prepared plans become possible where selectivity is stable
  - make sure every parameter hits a narrow index with no recheck
  - consider storing or serving a text projection only if measurements show jsonb_out dominates (a storage trade-off, not a correctness risk)
- Expected gain: RPS scales about 1/(PG CPU per query) at a fixed quota. 30% less PG CPU means about 30% more RPS. It also reduces how often the quota is hit.

**H3. JIT and parallel query on higher-cost plans. Likelihood: medium. Explains token 71–81 ms and quantity combo 85 / component 99 ms above the 65 ms floor.**
- Evidence:
  - PG 18 runs with the default `jit` (not set in bench:infra/postgres/postgres.conf).
  - `max_parallel_workers_per_gather = 4` (postgres.conf:7).
  - OctoFHIR sets neither per session; only tests disable them (srv:revinclude_tests.rs:111).
  - Parallel workers add runnable processes, which burn the quota faster. JIT compile is CPU-heavy, and the groups above the floor are the broad-match token and quantity ones.
- Confirm:
  - With the server up, run `GET /fhir/Encounter?status=finished,in-progress&_count=20&_debug=search-explain-analyze` and `GET /fhir/Observation?component-value-quantity=gt80&_count=20&_debug=search-explain-analyze`.
  - Look for a `JIT:` section and `Gather` / `Workers Launched`.
- Fix (safe, no result change): `ALTER DATABASE octofhir SET jit = off; ALTER DATABASE octofhir SET max_parallel_workers_per_gather = 0;` at bootstrap, or `SET` in an sqlx `after_connect`.
- Expected gain: removes the extra 6–35 ms on the token and quantity groups and lowers PG CPU bursts.

**H4. k6 or measurement artifact. Likelihood: low for OctoFHIR, real for Aidbox's 09-10 numbers.**
- Evidence:
  - One series per group, so nothing is summed.
  - The cumulative p99 is flat for OctoFHIR.
  - The published value is the 04:50:00 sample, which for Aidbox fell in its cold first minute (84 vs steady 40 ms).
- Confirm: pull the p95 and p99 time series from VM, and compare with k6's end-of-test summary (`k6 run --summary-trend-stats "avg,med,p(95),p(99),max"`).
- Fix: none on the OctoFHIR side. Optionally ask upstream to use an end-of-run value or `max_over_time` instead of a sample at a 5-minute step.

**H5. TCP_NODELAY off, so Nagle plus 40 ms delayed ACK. Likelihood: low.**
- Evidence:
  - `axum::serve` without `tap_io(set_nodelay)` (srv:server.rs:2449; axum-0.8.9/src/serve/listener.rs:66-88).
  - Responses are full, uncompressed bodies written together, so the trigger is unlikely.
- Confirm: A/B with `listener.tap_io(|s| { let _ = s.set_nodelay(true); })`, or capture `tcpdump -i any port 8888` during the run and look for ~40 ms gaps before the final segment.
- Fix (safe): set nodelay.
- Expected gain: probably none to a small amount.

**H6. App-side overhead (auth, policy, tracing, registry, serialization, pool acquire). Likelihood: very low.**
- Evidence: the app CPU is 1.7–2.2 cores of 8 during search, and each path was checked in Q3.
- Confirm if desired: `samply` or `perf` on the server during a local run, plus a tokio-console check for blocked workers.

### Inferences
- Fixing H1 alone should take OctoFHIR's search p99 from a uniform ~65 ms to well below Aidbox's 21–35 ms. Its mean (≤5 ms) is already lower than Aidbox's (≤8.8 ms).
- H2 is the only route to more RPS while the 8-CPU Postgres quota stays fixed.
- H1 and H3 are configuration or bootstrap changes with no correctness or conformance risk.
- Pool reduction must be validated against CRUD and import. Those share the pool, and long write transactions holding connections could lower throughput if the pool is too small. A separate, smaller read/search limiter avoids that.

### Gaps
- No local measurements were taken. No OctoFHIR server was running, and the local perf-postgres has no benchmark data. Every confirmation above is specified but not executed.
- The CFS throttle counters for past runs need VictoriaMetrics credentials (vm.cdr.aidbox.dev).
- The CI host's core count is unknown. H1's quota-throttling mechanism needs the host to have more than 8 cores; with exactly 8, the floor would be plain saturation queueing and H1's fix (cap concurrency) would still help, but less dramatically.
