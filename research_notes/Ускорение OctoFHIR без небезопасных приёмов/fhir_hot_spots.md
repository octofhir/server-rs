# FHIR-specific hot spots in OctoFHIR and how to speed them up without weakening conformance

Path abbreviations (all local, read-only, state as of 2026-10-30, `main` @ 536cfc2):
- SRV = `/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-server/src`
- DB = `/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-db-postgres`
- SEARCH = `/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-search/src`
- AUTH = `/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-auth/src`
- FS = `/Users/alexanderstreltsov/work/octofhir/fhirschema/octofhir-fhirschema/src`
- FP = `/Users/alexanderstreltsov/work/octofhir/fhirpath-rs/crates/octofhir-fhirpath/src`
- FM = `/Users/alexanderstreltsov/work/octofhir/fhir-model-rs/src`

Local file:line citations are written as `path:line` (local source = primary evidence). The codegraph MCP server was down, so everything was mapped with grep and Read. Two read-only sub-explorations, one for the validation path and one for search, were spot-checked by hand. I re-verified these lines myself: `FS/validation/mod.rs:1188`, `FS/validation/compiler.rs:56`, `FS/validation/prepared.rs:85`, `FS/validation/mod.rs:1721-1740`, `FP/evaluator/prepared.rs:14-27`, `SRV/config.rs:735`, `SRV/handlers.rs:27-39`, `DB/src/queries/crud.rs:34-53` and `DB/sql/resource_write_gate.sql`.

## 1. Validation per write (fhirschema + FHIRPath invariants + bindings + references)

### Takeaway
Schemas, compiled schemas and FHIRPath plans are already cached, and `ele-1` is checked structurally rather than through FHIRPath. The remaining per-write overhead is mostly data-movement: 2 deep JSON clones, a full JSON→`FhirNode` conversion that is inserted into a pointer-keyed cache that can never hit for writes, 3–4 extra full-tree walks, `format!` path strings on every node, and **reference targets read 2–3 times per write**. Every one of these can be removed without dropping a single check.

### Cited findings (OctoFHIR, current state)

**Call chain**
- create → `should_skip_validation` → `validate_for_checked_write` (`SRV/handlers.rs:1155`, `:895`) → `schemas_for` (base type + `meta.profile`, `SRV/validation.rs:303`) → `validate_write_content` (`SRV/handlers.rs:913`) → `run_validation` (`SRV/validation.rs:356`) → `FhirValidator::validate_with_known_references` / `validate_impl` (`FS/validation/mod.rs:508`, `:523`).
- `validate_impl` runs these phases:
  - structural (`FS/validation/mod.rs:575`)
  - constraints + bindings, once per schema (`:593`)
  - extension walk (`:645`)
  - QuestionnaireResponse (`:654`)
  - reference existence (`:672`)
  - targetProfile (`:730`)
- Everything is sequential on the request task. There is no `spawn_blocking` or parallelism in `FS/validation`; the only `tokio::spawn` is in tests (`FS/validation/prepared.rs:352`).

**Schema caches**
- Raw schemas sit in moka caches keyed by name and by URL, capacity 2048, negative results cached too; a miss loads from Postgres `fcm.fhirschemas` (`SRV/model_provider.rs:72-120`, `:196-270`; capacity at `SRV/server.rs:676-680`).
- Compiled schemas sit in `moka::future::Cache::new(500)` with generation-based invalidation (`FS/validation/compiler.rs:46-56`, `:68-97`).
- **500 entries is below the R4 working set.** The server's own comment counts about 655 R4 core schemas (`SRV/server.rs:672-675`), and `validate_one_extension` calls `compile(url)` for every extension instance (`FS/validation/mod.rs:2505`), so evictions and recompiles are possible under a mixed workload.

**Deep clones per write**
- `ValidationReferenceResolver::for_write` does `Arc::new(candidate.clone())` (`SRV/operations/validation_reference.rs:133`).
- `prepare_constraint_variables` does `Arc::new(root_resource.clone())` for `%rootResource` (`FS/validation/mod.rs:1185-1189`).

**JSON→FHIRPath conversion**
- `PreparedRoots::value` converts `serde_json::Value` into an Arc tree `FhirNode` and caches it in a 32 MiB `moka::sync` cache **keyed by `Arc::as_ptr` address** (`FP/evaluator/prepared.rs:14-27`).
- Because every write produces a fresh Arc (the clone at `FS/validation/mod.rs:1188`), writes never hit this cache. Each insert only churns the cache.
- Two further full walks follow: a node vector (`FP/evaluator/model_prepared.rs:30-47`) and a pointer→index HashMap (`FM/prepared.rs:106-116`).

**Invariant evaluation**
- In compiled schemas, type-level invariants (`dom-2..6`, `ele-1`, `ext-1`) sit on the schema root (`FS/converter.rs:49-70`).
- When nested datatypes are expanded inline, their root constraints are dropped (`FS/validation/compiler.rs:442-445`).
- **`ele-1` is enforced structurally**: an object with no keys besides `id` is reported as an `ele-1` violation (`FS/validation/mod.rs:1721-1740`). It is not run through FHIRPath per element.
- `dom-*` and resource invariants are evaluated once at the root. Element invariants such as `pat-1` run per instance (`FS/validation/mod.rs:2309-2441`).
- Warning-severity constraints are not evaluated at all (`FS/validation/prepared.rs:85-88`).
- No `ext-1` implementation was found anywhere in `FS` (grep for `ext-1` returns nothing). **This is a possible conformance gap and needs a test**, unless extension-profile schemas carry `ext-1` at their compiled root.
- `format!` builds a path string at every visited node, even nodes with no constraints (`FS/validation/mod.rs:2346-2350`, `:2379`, `:2428`).

**FHIRPath plan caching**
- Expressions are parsed once:
  - `FhirValidator.constraint_plans` is a moka cache of 4096 entries (`FS/validation/mod.rs:330`, `:364`, `:382`).
  - The engine has its own AST cache of 256 entries (`FP/evaluator/engine.rs:32`, `:52`, `:83`).
  - Plans are retained handles (`FP/evaluator/plan.rs:12-17`).
- Results are memoized per `(NodeId, plan, type)` across all schemas of one write (`FS/validation/prepared.rs:11`, `:106-111`).

**Bindings**
- Only `required` bindings are checked, per element instance and per schema, with no memoization across schemas (`FS/validation/mod.rs:2555-2616`).
- A terminology error is advisory and does not fail the write (`FS/validation/mod.rs:2636-2640`).
- Wired through `HybridTerminologyProvider`:
  - result cache: `moka::sync`, 100k entries, TTL default 3600 s
  - negative cache: 60 s
  - single-flight for concurrent misses
  - on a miss: local expansion **rebuilt from `compose` every time**, then the remote fallback, `https://tx.fhir.org/r4` by default
  - (`SEARCH/terminology.rs:23-27`, `:147-184`, `:249-274`, `:439-472`, `:689-776`)

**Reference existence (ON by default, `skip_reference_validation: false`, `SRV/config.rs:710`, `:751`)**
- Phase 4 batches and deduplicates references, but the write resolver's `prefetch` → `fetch_resources_batch` runs `storage.read_many` **once per type, sequentially, reading full bodies** (`SRV/reference_resolver.rs:292-325`; `SRV/operations/validation_reference.rs:236-263`, `:325-344`).
- The cheap `exists_many_grouped` path exists (`SRV/reference_resolver.rs:230-290`): one `UNION ALL` of `id = ANY($n)` per type (`DB/src/queries/crud.rs:280-320`). It is only used by the server-wide resolver.
- The write step then checks again:
  - fast path: `check_references_exist` → `exists_many_grouped`, a second round-trip (`SRV/handlers.rs:1015-1035`, `:1183`)
  - prepared/authorized path: `lock_references` + `lock_reference_snapshots`, which re-reads each snapshotted body and deep-compares it with the copy captured during validation (`SRV/write.rs:423-466`; snapshots collected at `SRV/handlers.rs:952-972`)

**targetProfile (FS1017)**
- OFF by default (`SRV/config.rs:713-719`; documented in `octofhir.toml` `[validation]`).
- A base-SD target is settled by type identity. A real profile is meant to be settled from `meta.profile` via `fetch_profiles_batch`, but the write resolver does not implement that (`SRV/operations/validation_reference.rs:346-348`), so profile targets always fall through to fetch + recursive `validate_impl` (`FS/validation/mod.rs:757-998`).
- Bounded by `max_reference_depth` = 5.

**Other**
- `resource_constraints` re-parses `SearchParameter.expression` without caching (SearchParameter writes only; `SRV/validation.rs:514`).
- `$meta-add` / `$meta-delete` build a brand-new `ValidationService` per call, so they validate with cold caches (`SRV/operations/meta.rs:60`).

**Skip switch**
- `X-Skip-Validation` works only when `validation.allow_skip_validation` is set; the code default is `false` (`SRV/config.rs:735-737`, `SRV/handlers.rs:27-39`).
- **The checked-in `octofhir.toml` sets `allow_skip_validation = true`** (`/Users/alexanderstreltsov/work/octofhir/server-rs/octofhir.toml`, `[validation]`).
- Once enabled, the header is honoured for any caller who passes write authorization. It is not role-gated (`SRV/handlers.rs:27-39`).
- `$import` `skipValidation` ignores `allow_skip_validation` entirely (`SRV/operations/bulk/import.rs:220-225`).

### Cited findings (measured numbers, local)

**FHIRPath constraint microbenchmark** — Divan `benches/prepared_constraints.rs`, 3 invariants per node, fresh resource each iteration (`/Users/alexanderstreltsov/work/octofhir/fhirpath-rs/docs/EVALUATOR_PERFORMANCE.md:40-67`):

| Nodes | Legacy | Prepared |
| --- | --- | --- |
| 10 | ≈84 µs | ≈49 µs |
| 100 | ≈785 µs | ≈440 µs |
| 1000 | ≈13.0 ms | ≈4.3 ms |

- That is about 1.4 µs per invariant evaluation including preparation.
- Allocations at 1000 nodes fell from 117 MB to 26.75 MB.

**HTTP `$validate`** — `POST /fhir/Patient/$validate` with 100 `Patient.contact` nodes, 8 VUs, read-only (`/Users/alexanderstreltsov/work/octofhir/server-rs/k6/benchmarks/VALIDATION_RESULTS.md`):
- First pass: 1584–1595 → 4768–4829 RPS; p99 17–18 → 8.7–9.2 ms.
- VM follow-up: 4774/4771 → 4882/4943 RPS. The doc says it does "not establish a whole-server performance gain".

**HTTP `$fhirpath`** — regex case 1032 → 5815 RPS; where 6839 → 7447; distinct 6692 → 7326 (`/Users/alexanderstreltsov/work/octofhir/server-rs/k6/benchmarks/FHIRPATH_RESULTS.md`).

**CRUD** — 3080 RPS overall, create p95 ≈ 50 ms (`/Users/alexanderstreltsov/work/octofhir/server-rs/benchmark-results/crud.json`, 2026-02-20). This does not isolate validation.

### Cited findings (other servers / literature)

**HAPI**
- Caching moved into `ValidationSupportChain` as of HAPI 8.0.0, and `CachingValidationSupport` is deprecated.
- StructureDefinitions are held in a **non-expiring** cache, because the validator "makes assumptions that these objects will not change for the lifetime of the validator".
- `fetchAll…` results sit in an expiring cache refreshed asynchronously; other lookups use an expiring cache.
- Source: [HAPI validation support modules](https://hapifhir.io/hapi-fhir/docs/validation/validation_support_modules.html).

**HAPI performance reports**
- HAPI's maintainer said HAPI uses the same validator code as the Validator CLI ([FHIR chat: Performance as FHIR server](https://chat-archive.fhir.org/stream/179167-hapi/topic/Performance.20as.20FHIR.20server.html)).
- Users report the HAPI validator getting slower with each added package ([hapi-fhir group](https://groups.google.com/g/hapi-fhir/c/SMouCqKJuBY)).
- One validation took ~1m19s, triggered by custom code system URLs containing the word "CodeSystem" ([FHIR chat: Current validator very slow](https://chat-archive.fhir.org/stream/179166-implementers/topic/Current.20validator.20very.20slow.20for.20some.20resources.3F.html)).
- No current per-resource ms benchmark was found.

**Aidbox**
- Vendor claim: the FHIR Schema engine is "100 times faster than the official FHIR validator and 17 times faster than the zen validator". No methodology is given ([Aidbox news](https://health-samurai.io/news/aidbox-transitions-to-the-fhir-schema-engine)).

**Medplum** (`@medplum/core` validation)
- `skippedConstraintKeys` = `ele-1`, `dom-3`, `org-1`, `sdf-19`. `dom-3` is skipped because it needs `descendants()`.
- Only `severity === 'error'` constraints are evaluated, per element value, via `evalFhirPathTyped` with an **LRU cache of 1000 parsed expressions**.
- Required bindings are **not** checked against ValueSets during validation; they are collected as tokens.
- Source: [validation.ts](https://raw.githubusercontent.com/medplum/medplum/main/packages/core/src/typeschema/validation.ts).

**Firely**
- "Compilation is expensive". The default resolver `CreatedCached` caches compiled schemas, and a shared `ConcurrentDictionary` can be supplied ([Firely profile validation internals](https://docs.fire.ly/projects/Firely-NET-SDK/en/latest/_sources/validation/profile-validation-internals.md.txt)).
- The FHIRPath `FhirPathCompilerCache` defaults to 500 compiled expressions ([Firely: invoking the compiler](https://docs.fire.ly/projects/Firely-NET-SDK/en/latest/fhirpath/invoking-compiler.html)).

**Spec basis for a structural `ele-1`**
- R4 JSON: "Objects are never empty", "JSON objects and arrays are never empty", "String property values can never be empty", "properties never have null values" except the primitive-array alignment case ([FHIR R4 JSON](https://hl7.org/fhir/R4/json.html)).
- A structural check of these rules is therefore equivalent to `ele-1` for JSON input. OctoFHIR already does this (`FS/validation/mod.rs:1721-1740`).

### Inferences
- Unlike Medplum, OctoFHIR does not *drop* `ele-1`. It replaces it with a structural equivalent, so it keeps conformance where Medplum skips. `ext-1` (`extension.exists() != value.exists()`) is structural in the same way and should be a native Rust check rather than missing or FHIRPath. A native check adds conformance and costs about nothing.

**Cheapest safe wins**, all preserving every check:
1. **Reference reads.**
   - Use `exists_many_grouped` for Phase-4 existence and read bodies only when targetProfile or a snapshot is actually needed.
   - Run the per-type `read_many` calls concurrently, or as one `UNION ALL`.
   - Reuse the validation-time existence result in `check_references_exist` within the same request.
   - Today a write with N reference types costs up to N sequential full-body reads plus one more existence round-trip.
2. **Clones and conversion.**
   - Pass `Arc<Value>` from the handler into the validator and resolver, removing both deep clones.
   - Bypass `PreparedRoots` insertion for one-shot roots (a pointer key that never hits).
   - Fuse the weight, node-vector and pointer-index walks into one pass.
3. **Walk pruning.**
   - Precompute at compile time a per-`CompiledElement` flag: "subtree has error-severity constraints or required bindings".
   - Skip subtrees that have nothing to check, and build path strings lazily, only when an issue is emitted.
4. **Binding memo.** Memoize required-binding results per `(node, valueset)` across schemas, as is already done for constraints.
5. **Expansion cache.** Cache local ValueSet expansions as hash sets (keyed by VS URL + version + package generation) instead of rebuilding from `compose` on each cache miss.
6. **Compiled-schema cache.** Raise its capacity (≥2048, matching the model provider) or size it by weight. Cache "unknown extension URL" negatives with generation invalidation.
7. **`$meta`.** Reuse the shared `ValidationService` in `$meta`.

- Validation is CPU-bound and runs on a Tokio worker. For large resources or bundles, `spawn_blocking` or a rayon pool would protect tail latency of other requests. This is a latency/isolation win, not a throughput win.
- An explicitly opt-in "trusted ingest" mode already exists (`allow_skip_validation`). **Risk:** any authorized writer can then persist non-conformant data (cardinality, invariants, required bindings, dangling references), which later breaks search indexing, `$validate` and IG conformance.
  - It should be role- or scope-gated.
  - The checked-in config should default it to `false`.
  - `$import skipValidation` should respect the same switch.

### Gaps
- No measured split of per-write time into validation vs SQL vs auth. `benchmark-results/crud.json` does not isolate validation. **Experiment:**
  - Run `just bench-crud` twice: once with the same payload sent with `X-Skip-Validation: true` (`allow_skip_validation` on, bench only), once without.
  - Add a `tracing` span around `validate_for_checked_write`, plus the `PROFILE_EVAL=1` timer that already exists.
  - Also profile `SRV/handlers.rs:913` with `just profile` (macOS `sample`, see `justfile:137-148`).
- `ext-1` conformance status is unverified. **Experiment:**
  - POST a Patient with `extension: [{url: "http://x", valueString: "a", extension: [{url: "y", valueString: "b"}]}]`. It must fail `ext-1`.
  - POST one with neither value nor extensions. It must fail too.
- No independent, published, per-resource ms benchmark exists for HAPI, Firely or IBM validators. The Aidbox 100x figure is vendor-only. No LinuxForHealth/IBM FHIR numbers were found (not searched in depth).
- No saved results exist for `fhirschema/benches/validation_bench.rs`; there is no `target/criterion` in any repo. **Experiment:** `cargo bench -p octofhir-fhirschema --bench validation_bench` before/after each item in the safe-wins list above.

## 2. Auth / policy evaluation per request (JWT, revocation, QuickJS)

### Takeaway
JWT signatures are already cached by token hash (DashMap, 60 s TTL, revocation re-checked). Even on a cache hit, every authenticated request still makes **2–3 Postgres round-trips**: revocation, client, and user for password/auth-code tokens. Two of those queries filter on unindexed `resource->>'…'` expressions. The QuickJS engine is pooled, but it **re-parses and re-compiles the policy script, re-evaluates the helper prelude, rebuilds `console`, and JSON-serializes the whole context (including the resource) into JS source on every call**. In searches with resource-dependent policies this happens once per returned row. The k6 benchmark uses an `allow` policy, so it never exercises QuickJS.

### Cited findings (OctoFHIR)

**JWT verification and per-request lookups**
- JWT verification cache `JwtVerificationCache`: DashMap keyed by SHA-256 of the token, max 10,000 entries, short TTL (default 60 s per header doc), with a JTI index for invalidation (`SRV/cache/jwt.rs:1-80`).
- On a miss, the signature is verified in `spawn_blocking` (`SRV/middleware.rs:384-405`).
- After the cache, every request:
  1. runs `revoked_token_storage.is_revoked(jti)` (`SRV/middleware.rs:408`)
  2. calls `load_current_context` → `client_storage.find_by_client_id` and `load_user_context` → `user_storage.find_by_id` (`AUTH/middleware/auth.rs:132-160`, `:278-310`)
  3. inserts into `auth_cache` (`SRV/middleware.rs:426`)
- `auth_cache` is **written but never read** in `validate_token`. The comment says the context is cached "never as an authorization proof" (`SRV/middleware.rs:368-377`).

**Unindexed lookup queries**
- Revocation query: `SELECT EXISTS(SELECT 1 FROM revokedtoken WHERE resource->>'jti' = $1)` (`/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-auth-postgres/src/revoked_token.rs:76-89`).
- Client query: `… FROM client WHERE resource->>'clientId' = $1` (`/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-auth-postgres/src/client.rs:94-106`).
- No expression index on `resource->>'jti'` or `resource->>'clientId'` was found: grep of `crates/octofhir-auth-postgres/src` and `DB/src/schema.rs` finds only session-token indexes.
- The per-table GIN is `jsonb_path_ops` on the whole document (`DB/src/schema.rs:337-347`), which serves `@>`, not `->> =`.
- `revokedtoken` is a historyless high-churn table (`DB/src/schema.rs:448-453`). Its seq-scan cost grows with the number of revocations and logouts until cleanup.

**Benchmark auth setup**
- k6 gets tokens with `grant_type=password` (`/Users/alexanderstreltsov/work/octofhir/server-rs/k6/lib/auth.js:29-32`), so the user lookup also runs on every request.
- The k6 client's AccessPolicy is `engine: { type: "allow" }` (`/Users/alexanderstreltsov/work/octofhir/server-rs/scripts/k6-setup.ts:120-135`), so the benchmark never runs QuickJS.

**Anonymous access**
- With `auth.policy.anonymous_access` on and no credentials, the middleware inserts a shared anonymous context and calls `next.run(req)` immediately (`SRV/middleware.rs:951-958`).
- Policy evaluation for anonymous requests therefore happens, if at all, only in downstream handler checks. I did not trace those.

**Policy evaluation**
- `PolicyEvaluator::evaluate`:
  - optional SMART scope check first (`evaluate_scopes_first`), using pre-parsed scopes (`AUTH/policy/engine.rs:374-391`, `:600-631`)
  - cached applicable policies per resource type
  - first deny wins
  - (`AUTH/policy/engine.rs:413-493`)
- `Allow`/`Deny` engines are constant-time (`AUTH/policy/engine.rs:633-645`).

**QuickJS runtime**
- Pool of `num_cpus` runtimes, created once. Each runtime sits behind a `std::sync::Mutex` and is picked round-robin by an atomic counter, not first-free (`AUTH/policy/quickjs.rs:87-138`; defaults `timeout_ms: 100`, `pool_size: num_cpus` at `AUTH/config.rs:294-307`).
- A blocking mutex is taken inside async code, and contention is possible when a slow script holds an instance while the counter keeps routing calls to it.
- Per call (`AUTH/policy/quickjs.rs:142-256`):
  1. `serde_json::to_string(policy_context)` builds a `var __ctx = <json>; …` script, which is `ctx.eval`'d, i.e. JSON parsed as JS source.
  2. `setup_console` creates new `Function` objects.
  3. `HELPER_SETUP` (about 15 helper definitions) is `ctx.eval`'d again.
  4. The policy is `format!("(function() {{\n{script}\n}})()")` and `ctx.eval`'d, so it is **parsed and compiled on every call**.
- The doc comment "Scripts are compiled to AST once and cached" (`AUTH/policy/engine.rs:255`) does not match this code.

**Per-row evaluation in search**
- With resource-dependent policies, search rows are checked one by one in `PreparedSnapshotSearch::allows` (`SRV/authorization/snapshots.rs:136-170`).
- For each row it parses `row.resource_json` into `serde_json::Value`, clones the policy context per context, and calls `evaluate`.
- A 100-row page therefore means 100 QuickJS script compilations and 100 full context serializations.
- QuickJS policies are deferred at admission until the resource is known (`AUTH/policy/engine.rs:440-454`).

**Benches**
- A criterion bench exists covering pool creation, simple and complex scripts, and small and large resources (`/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-auth/benches/quickjs_bench.rs:228-424`). No saved results were found.

### Cited findings (external)
- SMART/OAuth itself puts no constraint on server-side caching. HAPI and Medplum caching details for auth were not researched; no source found.
- Correctness of caching verified tokens is supported by OctoFHIR's own design: the cache is keyed by token hash, revocation is re-checked after a hit, and entries expire before `exp` (`SRV/cache/jwt.rs:7-12`).

### Inferences

**JWT cache**
- Safe as implemented: hash key, TTL ≤ token `exp`, revocation still checked.
- The code comment "RS256/ES384 verification takes 1-3 ms" (`SRV/middleware.rs:388-389`) looks pessimistic. RSA-2048 verification is generally tens of microseconds, but that figure is unsourced here; see Gaps.

**Removing per-request DB lookups safely**
1. Add expression indexes, `CREATE UNIQUE INDEX … ON client ((resource->>'clientId'))` and `… ON revokedtoken ((resource->>'jti'))`, created by `SchemaManager` for these system tables. This is a pure win.
2. Hold an in-process revocation set: load the JTIs of unexpired revoked tokens at startup and update it on the revocation write path. This server is single-instance by design (see MEMORY `project_single_instance_focus`). The DB stays authoritative and the set is just a mirror, so revocation latency stays zero for in-process revocations.
3. Cache client and user rows in moka with **write-path invalidation** (a write to `client` or `user` invalidates the key) plus a short TTL backstop.
4. Combined effect: the steady state becomes 0 DB round-trips for auth, versus 2–3 today.
- Do not cache the *authorization decision* across requests unless it is keyed by everything the policy reads.

**QuickJS: compile once, call many**
1. At policy load, evaluate the helper prelude once per runtime.
2. Compile each policy to a function, e.g. `globalThis.__p_<id> = function(user, client, scopes, request, resource, environment){…}`. Cache the handle per runtime and recompile on policy-generation change.
3. Per call, pass the context as a QuickJS object built natively (or via `JSON.parse` of a string argument, which is much cheaper than parsing JS source) and call the stored function.
4. Build the `console` object once.
5. Pick an instance with `try_lock` across the pool before falling back to waiting.
6. In snapshot searches, serialize the static context part (user, client, scopes, environment) once per page and set only `resource` per row.

**Short-circuit for pure role checks.** Recognize trivial scripts at load time (`return hasRole("x")`, `return allow()`, `return true`) and compile them to native `PolicyEngineType` variants. This requires a tiny pattern matcher and keeps identical semantics; the fallback is always QuickJS.

### Gaps
- No measured RSA/ECDSA verify cost on the target host. **Experiment:** `openssl speed rsa2048 ecdsap384`, plus a criterion bench of `JwtService::decode` with the cache disabled.
- No measured auth-overhead share in k6. **Experiment:**
  - Run `just bench-crud` with the default config, then with an expression index on `client`/`revokedtoken` plus a client/user moka cache.
  - Compare p50/p99 and `pg_stat_statements` calls per request.
- QuickJS cost per call is unmeasured. **Experiment:**
  - Run `cargo bench -p octofhir-auth --bench quickjs_bench` before and after "precompile and call function".
  - Then run a k6 search with a resource-dependent QuickJS policy over a 100-row page.
- Whether anonymous requests are policy-checked downstream was not traced.

## 3. Search features: `_total`, paging, `_include`/`_revinclude`, chaining, `:contains`, `_sort`

### Takeaway
- **`_total`:** already cheap. No COUNT runs by default; one runs only for `_total=accurate` or `_summary=count`.
- **Paging:** offset-only, `_offset` uncapped, no cursor.
- **Includes:** batched (one query per spec × target type, 4 in flight).
- **Chains:** correlated `EXISTS`, and `_has` is an IN-semijoin, both via per-row `jsonb_array_elements`; the `fhir_extract_ref` GIN indexes are not used.
- **`_sort`:** has no B-tree support, so `_sort=-date` sorts all matches.
- **`_total=estimate`:** parsed but not implemented.

### Cited findings (OctoFHIR)

**`_total`**
- COUNT is built only when `total_mode == Accurate` (`DB/src/queries/search.rs:1235-1242`, run at `:1310-1326`). It is a separate `SELECT COUNT(*) … WHERE …` with no LIMIT (`SEARCH/sql_builder.rs:1025-1049`).
- Without a COUNT, the total is derived from a lookahead row: `offset + page_len` is returned only when the last page is reached, else `None` (`SRV/handlers.rs:3583-3603`).
- `accurate`, `estimate` and `none` are accepted (`SEARCH/params_converter.rs:1293-1297`). `estimate` behaves like `none`; no `reltuples` or EXPLAIN estimate exists.

**Benchmark requests**
- `k6/benchmarks/search.js` sends no `_total`. Its URLs (`/Users/alexanderstreltsov/work/octofhir/server-rs/k6/benchmarks/search.js`):
  - `:141` `Patient?family=…&_count=20`
  - `:164` `Observation?code=29463-7&_count=20`
  - `:189` `Observation?date=lt…`
  - `:201` `Observation?subject:Patient.family=…`
  - `:228` `Patient?organization.name=…`
  - `:243` `Observation?code=29463-7&_include=Observation:subject&_count=10`
  - `:266` `Patient?family=…&_revinclude=Observation:subject&_count=10`
  - `:291` `Patient?_count=10`
  - `:311` `Patient?_count=10&_offset=10`
  - `:325` `Observation?code=29463-7&_sort=-date&_count=20`

**Paging**
- `LIMIT n+1 OFFSET m` (`SEARCH/params_converter.rs:210`; `SEARCH/sql_builder.rs:1274-1288`).
- No default ORDER BY. With `_sort`, `id ASC` is added as a tiebreaker (`SEARCH/sql_builder.rs:1262-1268`).
- `_count` is clamped to `max_count=100` (`SEARCH/params_converter.rs:1271-1275`; `SRV/config.rs:575-580`).
- **`_offset` is an uncapped u32** (`SEARCH/params_converter.rs:1277-1281`).
- Links are built in `/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-api/src/lib.rs:1330-1390`.

**`_include` / `_revinclude`**
- One query per include spec per target type, `buffered(4)` (`DB/src/queries/search.rs:2064-2066`).
- Forward includes use an `IN (SELECT … jsonb_array_elements … regexp_match …)` semijoin on the target PK (`DB/src/queries/search.rs:1654-1689`).
- `_revinclude` uses a per-source-row `EXISTS` over `jsonb_array_elements` with `split_part` and does not use the `fhir_extract_ref` GIN (`DB/src/queries/search.rs:1691-1724`).
- `:iterate` runs a depth loop with a `seen` set (`DB/src/queries/search.rs:2029-2096`).
- Limits are 1000 resources, 16 MiB and depth 100, and exceeding them is an error (`/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-storage/src/types.rs:454-478`).

**Chaining and `_has`**
- Forward chains are nested correlated `EXISTS (… chainN.id = ANY(ARRAY(<ids from outer ref>)) …)` (`SEARCH/ir/chain.rs:156-229`).
- `_has` is `outer.id IN (SELECT linked.id FROM source CROSS JOIN LATERAL (<ids>) …)` (`SEARCH/ir/chain.rs:326-402`).
- IDs are extracted per row with `split_part(ref->>'reference','/',-1)` (`SEARCH/ir/chain.rs:407-416`).
- Reference GIN indexes on `fhir_extract_ref(...)` exist only for `search.indexed_params` (`DB/src/functional_indexes.rs:324-331`; `SRV/config.rs:537-570`).

**Strings and `:contains`**
- `LIKE` on `f_unaccent_lower` (`SEARCH/types/string.rs:1-15`).
- Indexed params use a pg_trgm GIN prefilter on `fhir_text_blob(...)` followed by an exact recheck (`SEARCH/ir/render.rs:3316-3357`; `DB/src/functional_indexes.rs:230-232`, `:294-296`).
- pg_trgm is enabled in the migration `20241213000001_consolidated_schema.sql:10`.
- Trigram indexes exist only for Patient.name/family/given, Practitioner.name and Organization.name (`SRV/config.rs:554-558`).

**`_sort`**
- Dates sort on `fhir_extract_date_min/max(...)`. Strings and numbers sort via a correlated `min/max(unnest(...))` (`SEARCH/sql_builder.rs:365-419`).
- Only a GiST `tstzrange` index exists for dates (`DB/src/functional_indexes.rs:284-289`), which cannot serve ORDER BY.
- `updated_at` is deliberately unindexed (`DB/src/schema.rs:327-329`, consistent with MEMORY "only updated_at index" being reverted).

**Statement caching**
- Search statements use `.persistent(false)` deliberately (`DB/src/queries/search.rs:2252-2273`).
- `QueryCache` is constructed but unused (`DB/src/queries/search.rs:1136`, `:1244-1248`).

### Cited findings (spec and other servers)

**FHIR R4 search spec**
- `_total` values: "none", "estimate", "accurate". "Providing a precise number of matching resources may be onerous for the server"; "servers can ignore the _total parameter"; it is "just an optimization hint"; "The Bundle.total element is still optional".
- "Servers SHALL NOT return more resources than requested".
- For `_include`: "Servers are expected to limit the number of iterations done to an appropriate level" and "are not obliged to honor requests to include additional resources"; `:iterate` "is not the default behavior"; "each page of search results should include the matching includes".
- Source: [FHIR R4 search](https://hl7.org/fhir/R4/search.html).

**Medplum**
- `_total` defaults to none: "By default, the search responses do not include totals".
- Estimates are used above a threshold (1M on hosted; `accurateCountThreshold` when self-hosted).
- `_offset` is capped at 10,000.
- Cursor pagination works only with `_lastUpdated` ascending, needs `_count` ≥ 20, "uses database indexes, making it much faster than offset-based pagination".
- Source: [Medplum paginated search](https://www.medplum.com/docs/search/paginated-search).

**Smile CDR / HAPI**
- The total "may or may not be present"; counting "is a potentially slow operation" ([Smile CDR search basics](https://smilecdr.com/docs/fhir_standard/fhir_search_queries.html)).
- HAPI JPA caches search results for one minute by default (`reuseCachedSearchResultsForMillis`), bypassed with `Cache-Control: no-cache`. That trades away freshness ([HAPI JPA configuration](https://hapifhir.io/hapi-fhir/docs/server_jpa/configuration.html)).
- HAPI history counts are cached in RAM for up to one minute (COUNT_CACHED); COUNT_DISABLED avoids the count query ([HAPI JPA performance](https://hapifhir.io/hapi-fhir/docs/server_jpa/performance.html)).

**PostgreSQL**
- `count(*)` must "check visibility for all rows" under MVCC.
- `reltuples` and an `EXPLAIN (FORMAT JSON)`-based `count_estimate(query)` give fast estimates, but they are only planner estimates refreshed by VACUUM/ANALYZE.
- Source: [PostgreSQL wiki: Count estimate](https://wiki.postgresql.org/wiki/Count_estimate).

**Keyset pagination**
- With OFFSET, "the database must still fetch these rows … before it can send the following ones".
- The keyset/seek method "solves the problem of drifting results" and is "even faster", but you "cannot directly navigate to arbitrary pages" and it needs a definite sort order.
- Source: [use-the-index-luke: no-offset](https://use-the-index-luke.com/no-offset).

### Inferences

**`_total=estimate`**
- Implement it as planner `EXPLAIN (FORMAT JSON)` rows using the *already-built* SQL with bound parameters, never string-interpolated (SQL-injection caveat per the wiki).
- Optionally implement `accurate` capped at N (Medplum-style threshold) when the client asks for `estimate`.
- Spec-conformant: `_total` is a hint and `Bundle.total` is optional.
- Never silently downgrade an explicit `_total=accurate`.

**Deep paging**
- Add an opaque cursor link: `next` carries a signed or opaque `_cursor` encoding the last `(sort keys…, id)`.
- Today with no `_sort` there is no ORDER BY at all. Paging without ORDER BY is not guaranteed stable in Postgres. Adding `ORDER BY id` (PK) for default paging plus keyset `WHERE id > $last` gives stable, O(page) paging.
- Keep `_offset` for compatibility but cap it (Medplum: 10k) with an OperationOutcome that points to the `next` link.
- Conformance impact is nil: the spec leaves paging mechanics to the server, and the links are opaque.

**`_revinclude`, `_has`, chaining**
- Rewrite them to use the indexed `fhir_extract_ref(resource, paths) && ARRAY[...]` (GIN) when the param is in `indexed_params`, falling back to `jsonb_array_elements` otherwise.
- Same results, index-backed. Verify with the existing `_debug=search-explain-analyze` hook (MEMORY `project_transaction_conditional_refs`).

**`_sort=-date`**
- Add an optional B-tree on `fhir_extract_date_max(resource, paths)` for configured sort params, mirroring `indexed_params`. Gate it with config because each index costs writes.
- Postgres can then do index-ordered LIMIT scans.

**`:contains`**
- Already pg_trgm-backed for indexed params. For non-indexed string params, document that a seq scan is the expected cost, or add them to config.

### Gaps
- No measured latency per k6 search query in these notes. `benchmark-results/search.json` exists but was not parsed here. **Experiment:**
  - For each URL listed above, run `?_debug=search-explain-analyze` on a seeded DB (`just k6-*` seed).
  - Record the plan nodes: Sort vs Index Scan, `jsonb_array_elements` loops.
- No Medplum or HAPI source on chaining SQL shape was researched.

## 4. Conditional operations (If-None-Exist, conditional update/delete)

### Takeaway
Conditional operations are safe: search and write happen in one transaction under an **exclusive database-wide advisory lock**. That same lock is taken in *shared* mode by a statement trigger on every resource write, so **each conditional op briefly stops all writes server-wide**, and any long-running write blocks every conditional op. For conditional-heavy workloads (US-Core ingest, transaction bundles with `ifNoneExist`) this is the dominant serialization point.

### Cited findings (OctoFHIR)

**Lock mechanism**
- `lock_resource_writes(exclusive)` → `pg_advisory_xact_lock(1330006098, 1)` (exclusive) or `pg_advisory_xact_lock_shared(1330006098, 1)` (`DB/src/queries/crud.rs:34-53`).
- Every table has a statement trigger, `octofhir_guard_resource_write()` → `PERFORM pg_advisory_xact_lock_shared(1330006098, 1)` (`DB/sql/resource_write_gate.sql`; installed at `DB/src/schema.rs:255-261`).

**Flow**
- `select_rows` takes the exclusive lock, runs the search in the transaction, then `FOR UPDATE` on the matches. More than one match returns 412 (`SRV/write.rs:1002-1058`; `DB/src/queries/crud.rs:1623`).
- The condition search is run with `count=2`, total, sort, include and summary stripped (`SRV/handlers.rs:7766-7779`).
- **If-None-Exist:** a preview transaction, then validation outside the transaction, then search + insert again in the commit transaction (`SRV/handlers.rs:1108-1154`; `SRV/write.rs:1215-1248`).
- **Conditional PUT:** snapshot preview, then re-select + apply. A change in between returns `None` and the caller retries (`SRV/handlers.rs:2360-2440`; `SRV/write.rs:1089-1133`).
- **Conditional DELETE:** `conditionalDelete: "single"` (`SRV/handlers.rs:430`; `SRV/write.rs:1177-1210`).
- Unconditional create without an authorization context takes a no-transaction fast path (`SRV/handlers.rs:1122-1126`, `:1182-1187`).

### Cited findings (spec / others)
- The FHIR spec's conditional semantics (0, 1 or multiple matches → create / no-op / 412) are on the [R4 http page](https://hl7.org/fhir/R4/http.html). I did not fetch it in this session, so the exact quotes need checking; listed under Gaps.
- No HAPI or Medplum source on their conditional-create locking was retrieved here (gap).

### Inferences
- **Narrow the lock key.**
  - Make the advisory key per resource type, e.g. `pg_advisory_xact_lock(hashtext('Patient'))`. Ordinary writes then take shared locks only on the types they write, and a conditional op takes an exclusive lock only on the type it searches.
  - Correct as long as the condition's search reads only that type.
  - Chained or `_has` conditions read other types. For those, take shared or exclusive locks on every type touched by the compiled search IR, in sorted order to avoid deadlocks, or fall back to the global key.
  - Conformance is unchanged; only contention drops.
- **Identifier fast path.**
  - When the condition is exactly `identifier=system|value` (the dominant real-world If-None-Exist form) and the param is in `indexed_params`, an exclusive lock keyed on `hash(type, system, value)` serializes only competing writers of that identifier.
  - Ordinary writers of the same type must still take the shared form of that key, or a per-type shared key. That needs care, so prototype it behind a flag and prove it with a concurrency test (N parallel identical If-None-Exist creates → exactly 1 resource).
- **Validate once.** The preview + validate + re-search pattern validates once outside the transaction, which is good. Make sure the preview does not repeat the full search when the commit transaction will redo it anyway; for pure If-None-Exist the preview can be dropped when the search is cheap.

### Gaps
- No measurement of conditional-op throughput or of write stalls. **Experiment:**
  - Run `k6/benchmarks/transaction-upsert.js` alongside `crud.js` (concurrent writers).
  - Watch `pg_locks WHERE locktype='advisory'` and `pg_stat_activity.wait_event='advisory'`.
  - Compare against a per-type lock-key prototype.
- R4 http.html conditional-create quotes were not fetched in this session.

## 5. Reference integrity on write

### Takeaway
Existence checking is ON by default and already batched per type, but the same references are resolved two or three times per write: a full-body read in the validator, an existence check in the write step, and, on authorized paths, a re-read with deep-equality comparison. Collapsing these to one indexed existence probe per type (bodies only when targetProfile needs them) is a pure win.

### Cited findings
- **OctoFHIR:** see §1. Paths: `SRV/reference_resolver.rs:230-325`; `SRV/handlers.rs:809-886` (collection), `:1015-1035`; `SRV/write.rs:423-466`; `DB/src/queries/crud.rs:280-381`.
  - `lock_references_with_tx` deliberately avoids `FOR SHARE` to prevent MultiXact contention (`DB/src/queries/crud.rs:353-381`).
  - The fast path accepts the race with a concurrent delete (comment at `SRV/handlers.rs:1015-1017`).
- **HAPI** `setEnforceReferentialIntegrityOnWrite(true)`: "If the referenced entity does not exist, the server will return an error"; "referential integrity is not enforced on database-level"; "The referential integrity check _only_ validates references that are indexed by a `SearchParameter`". No performance numbers are given ([HAPI JPA configuration](https://hapifhir.io/hapi-fhir/docs/server_jpa/configuration.html)).
  - With deletes disabled, HAPI "is able to skip some resource reference deletion checks during resource creation" ([HAPI JPA performance](https://hapifhir.io/hapi-fhir/docs/server_jpa/performance.html)).
- **Medplum:** the Project field `checkReferencesOnWrite`, "Whether this project uses referential integrity on write operations such as 'create' and 'update'" ([@medplum/fhirtypes Project.d.ts](https://app.unpkg.com/@medplum/fhirtypes@4.3.10/files/dist/Project.d.ts)). It is opt-in per project; the design discussion is [medplum#1221](https://github.com/medplum/medplum/discussions/1221).

### Inferences
- OctoFHIR is stricter by default than HAPI and Medplum (both opt-in), and it checks every literal reference, not only SearchParameter-indexed ones. Keep the default. Only make it cheaper.

**Concrete plan**
1. The validator Phase 4 should call `exists_many_grouped` (PK `id = ANY`, `status != 'deleted'`, one `UNION ALL` round-trip).
2. Pass the resulting set as `known_refs` into the write step so it does not re-query on the fast path.
3. On the prepared path, lock only by id, and compare snapshots only for references whose bodies were actually used (targetProfile or policy).
4. A cross-request "exists" cache is **not** safe without delete invalidation, and it adds little because the PK probe is already sub-ms. Do not add one.

### Gaps
- No measurement of the reference-check share of write latency. **Experiment:**
  - Post Observations with 1, 3 and 6 distinct reference types and compare.
  - Count statements per request with `log_statement=all` (MEMORY `project_bundle_batching` recipe).

## 6. Startup (package loading, schema/search registries)

### Takeaway
Startup is already optimized:
- installed packages are skipped
- the schema LRU is primed in one bulk query
- the 7 heavy init groups run concurrently
- the SearchParameter registry is built from one bulk `SELECT` instead of 1 + 1400 round-trips
- GraphQL schema builds in the background

There is no recorded end-to-end startup time to compare against.

### Cited findings (OctoFHIR)
- Package install:
  - `SRV/canonical.rs:291-348`: already-installed packages skip the network preflight.
  - `SRV/canonical.rs:375-395`: already-installed packages skip install.
  - Registry packages install in parallel and URL packages sequentially (`SRV/canonical.rs:413-480`).
- Model-provider LRU of 2048 is primed by `bulk_load_fhirschemas_for_graphql` before parallel init. The comment says this saves "~2-3s off cold-boot" versus about 1400 per-SearchParameter SELECTs (`SRV/server.rs:768-780`; `SRV/model_provider.rs:151-163`).
- `tokio::join!` runs 7 groups concurrently: policy refresh, search registry (comment: "was the slowest future at 2.7s cold"), auth state, operations, compartments, gateway routes and resource types (`SRV/server.rs:783-829`).
- GraphQL schema builds early in the background (`SRV/server.rs:918-1010`).
- The conformance bootstrap (`bootstrap_conformance_if_postgres`) runs under a bootstrap lock before anything else (`SRV/server.rs:341`, `:760-767`).

### Cited findings (external)
- HAPI's maintainer on validator startup: "start up time; the validator has to load, and you can do this in advance if you're using HAPI" ([FHIR chat: Performance as FHIR server](https://chat-archive.fhir.org/stream/179167-hapi/topic/Performance.20as.20FHIR.20server.html)).
- HAPI keeps StructureDefinitions in a non-expiring cache for the validator's lifetime ([HAPI validation support modules](https://hapifhir.io/hapi-fhir/docs/validation/validation_support_modules.html)).

### Inferences
- Remaining safe levers:
  1. Persist the compiled SearchParameter registry and compiled schemas keyed by package-set hash, so warm restarts deserialize instead of re-deriving. The cache is invalidated when the package set changes.
  2. Prime `SchemaCompiler` for the top-N resource types in the background after `/healthz` turns ready, so first writes don't pay compile cost.
  3. Keep readiness honest: `/healthz` should report ready only after the registries that writes depend on are built.

### Gaps
- No measured cold or warm startup time was found in the repo. **Experiment:**
  - Run `RUST_LOG=info cargo run --profile bench` against a fresh DB (cold, with package install) and against an existing DB (warm).
  - Collect the `elapsed_ms` fields already logged ("Model provider LRU primed", "Parallel initialization completed", the GraphQL steps), plus the time to the first 200 from `/healthz`.

## 7. FHIRPath engine performance

### Takeaway
fhirpath-rs already compiles expressions to plans executed on a stack-machine VM, with AST and plan caches, and it passes 1176/1176 spec tests. For server workloads, the remaining overhead is converting `serde_json::Value` → `FhirNode` on every call, not evaluation itself. Other engines (fhirpath.js, Firely) also rely on compile-once-reuse, but published cross-engine numbers do not exist.

### Cited findings
- **fhirpath-rs internals:**
  - `CompiledPlan` lowers to an instruction array, with a `vm::Program` stack machine for context-dependent parts (`FP/evaluator/plan.rs:12-40`; `FP/evaluator/vm.rs:1-60`).
  - The AST cache is `moka::sync` with 256 entries and coalesced misses (`FP/evaluator/engine.rs:32`, `:83-85`).
  - There is an element-type cache (`FP/evaluator/engine.rs:58-65`).
  - Input is converted once into an Arc `FhirNode` tree (`FP/core/node.rs:1-36`), cached by Arc pointer (`FP/evaluator/prepared.rs:14-27`).
- **Measured** (`/Users/alexanderstreltsov/work/octofhir/fhirpath-rs/docs/EVALUATOR_PERFORMANCE.md`):
  - `1+2` ≈ 276 ns; `where(active)` ≈ 53 µs.
  - Warm regex-predicate median 90.77 → 71.26 µs.
  - The 8-thread lambda benchmark went 554 → 484 µs.
  - (The regex and lambda figures are as summarized in `k6/benchmarks/VALIDATION_RESULTS.md`.)
- **Measured** (`/Users/alexanderstreltsov/work/octofhir/fhirpath-rs/benchmark.md`, R5): simple ≈ 135.9K ops/s, medium ≈ 87.0K ops/s, complex Bundle with `resolve()` ≈ 17–23 ops/s.
- **fhirpath.js** recommends `compile()` once, then calling the returned function on many resources ([fhirpath.js README](https://github.com/HL7/fhirpath.js)).
- **Firely** reuses compiled expressions through a `FhirPathCompilerCache` (default 500) ([Firely docs](https://docs.fire.ly/projects/Firely-NET-SDK/en/latest/fhirpath/invoking-compiler.html)).
- **Go FHIRPath (another library)** says parsing is 10–50x more expensive than evaluation ([gofhir performance guide](https://gofhir.github.io/fhirpath/docs/advanced/performance)). This is library-specific and not evidence about fhirpath-rs.
- **Medplum** caches up to 1000 parsed expressions in an LRU ([Medplum validation.ts](https://raw.githubusercontent.com/medplum/medplum/main/packages/core/src/typeschema/validation.ts)).

### Inferences
- In-repo cross-language runs (`/Users/alexanderstreltsov/work/octofhir/benchmarks/results/*.json`, July 2025) use different suites per language, so they are not comparable. A fair comparison would run the official FHIRPath test cases in fhirpath.js `compile()`-once mode vs fhirpath-rs `compile_plan`-once mode on identical resources.
- **Biggest server-side lever:** make one `FhirNode` (or a borrowed view) per request and share it across validation, `$fhirpath`, search-parameter extraction and policies. Alternatively, have the engine navigate `serde_json::Value` directly through a borrowed adapter, avoiding the full mirror tree for one-shot use.
- For validation, the `PreparedRoots` pointer cache helps only repeated evaluation of the same Arc (e.g. `$fhirpath` loops). For writes it is pure overhead (§1).

### Gaps
- No published, apples-to-apples benchmark of fhirpath-rs vs fhirpath.js vs HAPI or Firely FHIRPath was found. **Experiment:**
  - Use the HL7 FHIRPath test-suite expressions over `patient-example.json` and `observation-example.json`.
  - Run 10k iterations each in compiled mode, on one machine.
  - Measure (a) parse+compile and (b) evaluate separately.

## 8. Terminology (binding validation, `:in`/`:below` expansion)

### Takeaway
Required-binding checks hit a large positive cache (100k entries, 1 h TTL) and a negative cache (60 s), with single-flight on misses. A local miss rebuilds the ValueSet expansion from `compose` every time, and the remote default is the public `tx.fhir.org`. A terminology outage is advisory: it does not block writes.

### Cited findings
- `SEARCH/terminology.rs:23-27`, `:147-184`, `:249-274`, `:439-472`, `:689-776`; `FS/validation/mod.rs:2555-2640`.
- `ServerTerminologyService` (`SRV/terminology_service.rs:41-60`) has no cache, but it is not on the write path.
- `search.max_valueset_expansion = 500` caps `:in`/`:below` expansions (`octofhir.toml [search]`).
- **HAPI's experience:** terminology lookups dominated some slow validations, and bypassing external terminology calls was suggested as a tuning step ([FHIR chat: Current validator very slow](https://chat-archive.fhir.org/stream/179166-implementers/topic/Current.20validator.20very.20slow.20for.20some.20resources.3F.html); [FHIR chat: Performance as FHIR server](https://chat-archive.fhir.org/stream/179167-hapi/topic/Performance.20as.20FHIR.20server.html)).
- **Medplum** does not check required bindings during validation at all ([Medplum validation.ts](https://raw.githubusercontent.com/medplum/medplum/main/packages/core/src/typeschema/validation.ts)). OctoFHIR is stricter.

### Inferences
- **Safe speedup:**
  - Cache *expanded* local ValueSets as `HashSet<(system, code)>` keyed by `(url, version, package-generation)` and invalidated on package install.
  - Pre-expand the required-binding ValueSets of the loaded core and IG packages at startup or in the background. Base R4 has a bounded set of them.
  - This removes `compose` re-evaluation on every cache miss and avoids remote calls for codes that are fully local.
- **Remote tx calls** for codes not resolvable locally should stay, so conformance is unchanged. Consider a configurable local tx server instead of `tx.fhir.org` for production latency.
- **Advisory-on-error behaviour** (`FS/validation/mod.rs:2636-2640`) is a conformance trade-off that already exists. Document it; do not widen it.

### Gaps
- No measurement of the binding-check share. **Experiment:**
  - Validate an Observation with a required-bound `status` plus a CodeableConcept from a large local VS, cold vs warm cache.
  - Count `validate_code_vs_local` misses with a tracing counter.
