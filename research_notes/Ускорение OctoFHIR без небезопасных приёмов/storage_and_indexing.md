# FHIR storage model and PostgreSQL indexing choices for OctoFHIR (PG 16–18, with PG19 RC notes)

Local code is cited as `path:line` relative to `/Users/alexanderstreltsov/work/octofhir/server-rs`. Numbers labelled "OctoFHIR brief" come from the task brief (in-house measurements), not from external sources.

## 1. Storage model: per-type JSONB + functional indexes vs extracted search-param tables vs columns vs hybrid

### Takeaway
OctoFHIR's per-type JSONB table with optional functional indexes is the only model among the surveyed servers in which adding a SearchParameter needs neither DDL nor a data reindex to return correct results; the extracted-table (HAPI, Microsoft) and column/lookup-table (Medplum) designs buy cheaper, better-estimated lookups by writing one extra row or column per value on every write and by requiring a reindex before a new parameter matches existing data. No server avoids non-HOT updates once any index covers the document, so the cheapest wins for OctoFHIR are fewer and narrower indexes per type, not a change of model.

### Cited Findings
**OctoFHIR today (local code)**
- Resource table: `id TEXT PRIMARY KEY, txid BIGINT, created_at, updated_at, resource JSONB, status, ucum JSONB` plus an optional `profile TEXT[] GENERATED ... STORED` column — `crates/octofhir-db-postgres/src/schema.rs:223-237`.
- Reloptions `toast_tuple_target = 8160, fillfactor = 90, autovacuum_vacuum_scale_factor = 0.02, autovacuum_analyze_scale_factor = 0.01, autovacuum_vacuum_cost_limit = 2000` — `schema.rs:245-251`. The comment says no UPDATE can be HOT because `resource` carries index entries — `schema.rs:211-215`.
- History uses statement-level `AFTER UPDATE/DELETE ... REFERENCING OLD TABLE AS old_rows FOR EACH STATEMENT` triggers — `schema.rs:283-326`. The history table has PK `(id, txid)` plus a BRIN on `(txid, updated_at)` with `pages_per_range = 32` — `schema.rs:288-307`.
- `updated_at`, `created_at`, `txid` and `status` are deliberately unindexed "since the timestamp trigger touches the column on every write" — `schema.rs:328-330`.
- A whole-document `GIN (resource jsonb_path_ops) WITH (fastupdate=on)` is on every table unless `document_gin_resource_types` narrows it — `schema.rs:333-347`, `octofhir.toml:38-46`.
- Functional indexes are created only for `search.indexed_params`, at bootstrap, non-concurrently — `crates/octofhir-db-postgres/src/functional_indexes.rs:1-11`. Per parameter type:
  - date → `GiST(tstzrange(min, max))` — `functional_indexes.rs:277-290`
  - string → `GIN(fhir_text_blob(...) gin_trgm_ops)` — `functional_indexes.rs:229-233, 292-297`
  - CodeableConcept/Coding token → `GIN((subtree) jsonb_path_ops)` — `functional_indexes.rs:299-315`
  - reference → `GIN(fhir_extract_ref(resource, jsonpath[]))` — `functional_indexes.rs:316-331`
  - quantity → btree or GiST hulls — `functional_indexes.rs:332-380`
- The default `indexed_params` list has 31 entries: 15 date (GiST), 5 string (trigram GIN) and 11 reference (GIN) — `crates/octofhir-server/src/config.rs:537-571`.
- OctoFHIR brief: Patient writes run at ~1.6k/s with the default `indexed_params` vs ~7.5k/s with 4 params. Index insert costs ~1.4 µs/row for btree vs ~10–14 µs/row for GiST.
- Resource ids are generated as UUIDv7 in Rust (`uuid::Uuid::now_v7()`) — `crates/octofhir-core/src/id.rs:26`, `crates/octofhir-db-postgres/src/queries/crud.rs:1169`.

**PostgreSQL HOT rules**
- HOT requires that "the update does not modify any columns referenced by the table's indexes, not including summarizing indexes. The only summarizing index method in the core PostgreSQL distribution is BRIN", and that the old row's page has enough free space — [PG18 docs: HOT](https://www.postgresql.org/docs/current/storage-hot.html).
- HOT benefits: "New index entries are not needed to represent updated rows, however, summary indexes may still need to be updated"; intermediate versions can be pruned during normal operation, including SELECTs — [PG18 docs: HOT](https://www.postgresql.org/docs/current/storage-hot.html).
- The BRIN exception arrived in PG16: "Allow HOT updates if only BRIN-indexed columns are updated" — [PG16 release notes](https://www.postgresql.org/docs/16/release-16.html).
- HOT ratio is monitored via `pg_stat_all_tables.n_tup_upd` vs `n_tup_hot_upd`, and lower fillfactor raises HOT likelihood — [PG18 docs: HOT](https://www.postgresql.org/docs/current/storage-hot.html).

**HAPI FHIR (extracted index tables)**
- `HFJ_RESOURCE` holds one row per resource, and `HFJ_RES_VER` holds each version's body, GZIP-compressed (encoding `JSONC`). Search indexes live in `HFJ_SPIDX_{DATE,NUMBER,QUANTITY,STRING,TOKEN,URI}`, each row carrying `SP_ID, RES_ID, SP_NAME, RES_TYPE, HASH_IDENTITY, SP_UPDATED, SP_MISSING` plus typed values and hashes (token: `HASH_VALUE, HASH_SYS_AND_VALUE, HASH_SYS`; string: `SP_VALUE_NORMALIZED`, `HASH_EXACT`). References go to `HFJ_RES_LINK`. Rows are written "when a resource is created or updated" — [HAPI JPA schema](https://hapifhir.io/hapi-fhir/docs/server_jpa/schema.html).
- Hashes are MurmurHash3_x64_128 truncated to 64 bits. A default string search uses `HASH_IDENTITY` plus `LIKE` on `SP_VALUE_NORMALIZED` — [HAPI JPA schema](https://hapifhir.io/hapi-fhir/docs/server_jpa/schema.html).
- New search parameters "will cover resources added after the search parameter was added", and older resources only after reindex (secondhand, via an old HAPI post quoted in search results). Smile CDR (HAPI's commercial distribution) documents `$reindex` for this case — [Smile CDR: search parameter reindexing](https://smilecdr.com/docs/fhir_repository/search_parameter_reindexing.html); [Smile blog](https://www.smiledigitalhealth.com/our-blog/custom-search-parameters-in-hapi-fhir).

**Microsoft fhir-server (SQL Server, extracted tables)**
- Schema v80 defines `dbo.Resource`, `dbo.SearchParam (SearchParamId SMALLINT IDENTITY, Uri, Status, LastUpdated, IsPartiallySupported)` and per-type tables such as `TokenSearchParam (ResourceTypeId, ResourceSurrogateId, SearchParamId, SystemId, Code VARCHAR(256), CodeOverflow)`. Each token value carries a clustered index `(ResourceTypeId, ResourceSurrogateId, SearchParamId)` plus `IX_SearchParamId_Code_INCLUDE_SystemId`, and everything is partitioned by `ResourceTypeId`. `Resource` has a `SearchParamHash` column and an `IsHistory` flag — [microsoft/fhir-server Migrations/80.sql](https://github.com/microsoft/fhir-server/blob/main/src/Microsoft.Health.Fhir.SqlServer/Features/Schema/Migrations/80.sql) (lines ~24, 133-138, 698-707, 890-919, read locally via `gh api`).
- Azure's documentation states that custom search parameters require a `$reindex` (system-wide or per-instance) before existing data is searchable — [Microsoft Learn: custom search](https://learn.microsoft.com/en-ca/azure/healthcare-apis/fhir/how-to-do-custom-search).
- Firely Server also requires a reindex after adding a custom parameter: resources already stored "will not be indexed for that parameter" — [Firely docs](https://docs.simplifier.net/projects/Firely-Server/en/latest/features_core_fhir_platform/customsearchparameters.html).

**Medplum (columns on resource tables + lookup tables)**
- Each resource-type table has `id`, `content`, and "one column per search parameter" typed per parameter type, many as arrays (`text[]`). `HumanName` uses a lookup table with `name, given, family` and tsvector columns. Tokens are stored as (searchParam, system, code) tuples in per-type tables such as `DiagnosticReport_Token`. Values are extracted with FHIRPath and "indexed into the database transactionally". A subquery on an array column is "nearly (±5%) as performant as &&" — [Medplum search architecture](https://www.medplum.com/docs/contributing/search-architecture).
- A Medplum maintainer described filtering on address and sorting on name as "a pathological case due to the way our database stores these types of search parameters" (v3.2.11) — [Medplum community thread](https://linen.medplum.com/t/28489420/sorting-patients-by-name).

**Aidbox (same family as OctoFHIR)**
- Aidbox stores resources in a jsonb column and uses a GIN index for `@>`. For complex params it uses functional GIN `gin_trgm_ops` indexes over `knife_extract_text(resource, ...)`, and offers index-suggestion RPCs (`aidbox.index/suggest-index`, `suggest-index-query`) — [Aidbox indexes](https://www.health-samurai.io/docs/aidbox/storage-1/indexes); [Aidbox index management](https://www.health-samurai.io/docs/aidbox/storage-1/index-management).

**Generated columns as an alternative**
- PG18: "Allow generated columns to be virtual, and make them the default" — [PG18 release notes](https://www.postgresql.org/docs/18/release-18.html).
- A virtual generated column's expression "must not reference user-defined functions or types". A generated column "cannot be part of a partition key". A stored column "is computed when it is written ... and occupies storage" — [PG18 docs: generated columns](https://www.postgresql.org/docs/18/ddl-generated-columns.html).
- In PG18, virtual generated columns cannot be indexed, nor carry unique constraints, extended statistics or FKs. EDB says work to lift the index restriction is in progress for PG19 — [EDB: journey to virtual generated columns](https://enterprisedb.com/index%2ephp/blog/journey-virtual-generated-columns); [depesz](https://www.depesz.com/?p=4342).
- A March 2026 commit fixed a spurious "indexes on virtual generated columns are not supported" error on ordinary expression indexes, backpatched to 18. This matters for OctoFHIR's functional indexes on 18.x minors that predate the fix — [pgsql-committers](https://www.postgresql.org/message-id/E1w4zA1-001DwY-2l%40gemulon.postgresql.org).
- PG19 RC1 adds only "Allow extended statistics on virtual generated columns" and has no index support for virtual generated columns — [PG19 release notes (RC1)](https://www.postgresql.org/docs/19/release-19.html).

### Inferences
- **Write amplification per resource write.**
  - *OctoFHIR:* 1 heap tuple, 1 PK entry, and per indexed param one btree/GiST entry or k GIN keys. The document GIN adds one key per path/value, mostly absorbed by the pending list. History adds +1 heap tuple and 1 PK entry on update/delete.
  - *HAPI/Microsoft:* 1 heap row in a side table *per value* (~24-byte tuple header + payload) plus 1–2 btree entries on that row. A resource with 30 indexed values (common for Observation/Patient across the full SearchParameter set) adds ~30 heap tuples + ~60 index entries.
  - *Medplum:* the column approach widens the main tuple and adds one btree/GIN entry per column. Its HOT behaviour matches OctoFHIR's, since every write rewrites indexed columns.
  - Conclusion: extracted tables are cheaper per query but costlier per write than OctoFHIR's selective functional indexes, unless OctoFHIR indexes everything.
- **HOT.** With any index whose expression references `resource` (the document GIN or any functional index), every FHIR update is non-HOT, because every update changes `resource` (`meta.versionId`/`lastUpdated`), as `schema.rs:211-215` already notes. A table with only the PK (document GIN off, no indexed params) *can* get HOT updates, because `id` never changes and `updated_at` is unindexed. That makes `document_gin_resource_types = []` plus a short `indexed_params` list a real HOT lever for update-heavy types. Extracted-table designs can keep the main table HOT-eligible but pay delete+insert of side rows instead (unless they diff, which I could not verify for HAPI).
- **What a migration to extracted tables would lose:**
  - the "no reindex on new SearchParameter" property (all three competitors need `$reindex`);
  - read-after-write simplicity: one table holds the truth, with no per-value consistency to maintain;
  - the cheap path for rarely searched params.
- **What a migration would gain:** btree lookups on hashed `(param, system, code)`, with good statistics per value. These are better density-dependent estimates than functional GIN/GiST on JSONB give.
- **Recommended hybrid that keeps OctoFHIR's model:** keep JSONB as source of truth and recheck, keep functional indexes as optional accelerators, and add *stored* side data only where it is built from the resource at write time and validated by an exact recheck. The existing `ucum` column already does this (`schema.rs:221-237`). Never make a side structure the sole truth for matching without a recheck against `resource`.

### Gaps
- I did not verify whether HAPI updates `HFJ_SPIDX_*` rows by diff or by delete/insert on resource update.
- No public write-throughput numbers were found for HAPI, Microsoft or Medplum that compare like-for-like with OctoFHIR's 1.6k/s vs 7.5k/s.
- Medplum's procedure when a SearchParameter is added (migration vs reindex) is not documented on the page read. Since columns are per-param, DDL is implied (inference).
- The IBM/LinuxForHealth FHIR schema was not fetched within the tool budget. Its parameter tables are reportedly of the extracted type, which is unverified.

## 2. Index types, planner behaviour and pagination

### Takeaway
For write cost the hierarchy is btree < GIN with fastupdate < GiST, and OctoFHIR's GiST date hulls are the most expensive default; 15 of the 31 default `indexed_params` are dates. Correctness is safe for every index type considered, because GIN pending lists are always scanned and trigram/GiST hull indexes are rechecked. Plan quality matters most where density varies, under `ORDER BY ... LIMIT`, and for OFFSET paging; expression statistics and keyset pagination improve these without adding write cost or a reindex.

### Cited Findings
**GIN fastupdate**
- GIN fastupdate buffers new entries in a pending list that is merged "when the table is vacuumed or autoanalyzed, or when `gin_clean_pending_list` function is called, or if the pending list becomes larger than gin_pending_list_limit".
- Its downside: "searches must scan the list of pending entries in addition to searching the regular index, and so a large list of pending entries will slow searches significantly", and the insert that overflows the list pays "an immediate cleanup cycle". Results stay correct; only latency is affected.
- "If consistent response time is more important than update speed", set `fastupdate = off`. `gin_pending_list_limit` can be set per index.
- Source for all three points: [PG18 docs: GIN](https://www.postgresql.org/docs/18/gin.html).
- `jsonb_path_ops` supports only `@>`, `@?`, `@@` (not `?`, `?|`, `?&`) "but offers better performance for those operators" — [PG18 docs: GIN](https://www.postgresql.org/docs/18/gin.html).
- For bulk loads the docs advise dropping the GIN index and recreating it after bulk insertion. GIN build time "is very sensitive to the `maintenance_work_mem` setting" — [PG18 docs: GIN](https://www.postgresql.org/docs/18/gin.html).
- OctoFHIR already flushes GIN pending lists every 30 s in the background, because autovacuum cadence "can lag behind a write-heavy workload" — `crates/octofhir-db-postgres/src/gin_maintenance.rs:1-21`. The query analyzer's suggested GIN uses `gin_pending_list_limit=131072` (128 MB, in kB) — `crates/octofhir-db-postgres/src/query_analyzer.rs:413`.

**pg_trgm**
- `gin_trgm_ops` and `gist_trgm_ops` support `LIKE`, `ILIKE`, `~`, `~*`, similarity and `=`; equality is "not as efficient as regular B-tree".
- "A pattern with no extractable trigrams will degenerate to a full-index scan".
- GiST supports KNN `<->` ordering; GIN does not.
- GiST trigram signatures are lossy, with `siglen` default 12 bytes and range 1–2024.
- Source for all four points: [PG18 docs: pg_trgm](https://www.postgresql.org/docs/18/pgtrgm.html).

**Extended statistics**
- `CREATE STATISTICS s ON (expr) FROM t` builds univariate statistics on an expression, "provid[ing] benefits similar to an expression index without the overhead of index maintenance".
- The multivariate kinds are `ndistinct`, `dependencies`, `mcv`.
- Source for both points: [PG18 docs: CREATE STATISTICS](https://www.postgresql.org/docs/18/sql-createstatistics.html).
- PG19 RC1 adds `pg_restore_extended_stats()`, `pg_clear_extended_stats()` and "Speed up join selectivity computations for large optimizer statistics targets" — [PG19 release notes (RC1)](https://www.postgresql.org/docs/19/release-19.html).
- PG18 `pg_upgrade` preserves optimizer statistics but "Extended statistics are not preserved" — [PG18 release notes](https://www.postgresql.org/docs/18/release-18.html).

**Custom vs generic plans**
- For prepared statements under `plan_cache_mode = auto`, "the first five executions are done with custom plans", after which a generic plan is used "if its cost is not so much higher than the average custom-plan cost". `force_custom_plan` and `force_generic_plan` override this — [PG18 docs: PREPARE](https://www.postgresql.org/docs/18/sql-prepare.html).
- OctoFHIR binds search queries with `.persistent(false)` (unnamed statements, so a plan is built per execution with the actual values) — `crates/octofhir-db-postgres/src/queries/search.rs:2273, 2298, 2323, 2348, 2402`.
- PG19 RC1 adds generic/custom plan counts to `pg_stat_statements` — [PG19 release notes (RC1)](https://www.postgresql.org/docs/19/release-19.html).

**ORDER BY + LIMIT pathology**
- With a selective filter plus `ORDER BY <indexed col> LIMIT n`, the planner can walk the ordering index and discard many rows (minutes vs sub-millisecond in pganalyze's example). The cause is estimates that assume matches are uniformly spread, or correlation in the wrong direction — [pganalyze: ORDER BY + LIMIT](https://pganalyze.com/blog/5mins-postgres-planner-order-by-limit).
- The same issue appears with a JSONB filter and a UUID PK in [BUG #18834](https://www.postgresql.org/message-id/18834-ec88fdda949458d0@postgresql.org), and with a rare `org_id` in [BUG #18423](https://www.postgresql.org/message-id/18423-f063cbf58ca6a294%40postgresql.org).

**OctoFHIR ordering and paging**
- `build_order_clause` returns nothing when no `_sort` is given; with a sort it appends `id ASC` as a tiebreaker — `crates/octofhir-search/src/sql_builder.rs:1243-1270`.
- Pagination is `LIMIT n OFFSET m` — `sql_builder.rs:1272-1285`.
- System search orders a UNION and then applies `LIMIT ... OFFSET` — `sql_builder.rs:856`.

**Skip scan and IN-lists**
- PG18 btree skip scan lets multicolumn indexes be used "when there are no restrictions on the first or early indexed columns ... and there are useful restrictions on later indexed columns" — [PG18 release notes](https://www.postgresql.org/docs/18/release-18.html).
- A secondary summary reports up to 5x for low-cardinality leading columns and no gain for high-cardinality ones (unverified) — [pulse.bot summary of Robins Tharakan](https://www.pulse.bot/big-data/news/robins-tharakan-the-skip-scan-you-already-had-before-v18-f08301fc-ad88-40d8-a719-fb349ecbf9ad).
- PG17: "Allow btree indexes to more efficiently find a set of values, such as those supplied by IN clauses using constants" — [PG17 release notes](https://www.postgresql.org/docs/17/release-17.html).
- PG18: "Allow transforming OR-clauses to arrays for faster index processing" — [PG18 release notes](https://www.postgresql.org/docs/18/release-18.html).

### Inferences
**Index type by parameter type (all preserve "no reindex on new SP"; they are accelerators only):**
- *Token on scalar code/identifier:* a btree over a normalized text-array expression is not possible, because btree cannot index arrays for `&&`/`@>`. The current subtree GIN `jsonb_path_ops` is the right call (`functional_indexes.rs:299-315`). For hot single-valued tokens (e.g. `Observation.status`), a partial index or a btree on `(resource->>'status')` is cheaper; the predicate must match the expression textually.
- *Reference:* the GIN over `text[]` is right. Do not switch to `fastupdate=off` on high-write tables unless p99 search latency shows pending-list scans. The 30 s cleaner already bounds the pending list.
- *Date:* GiST hull costs ~10–14 µs/row (OctoFHIR brief) vs ~1.4 µs for btree. For single-valued dates (`Patient.birthdate`, `Claim.created`, `MedicationRequest.authoredon`, `Encounter.period` start), two btrees on `fhir_extract_date_min` and `fhir_extract_date_max` would let `ge/le/gt/lt` use an index scan. Combined with BitmapAnd or a recheck, they are likely cheaper to write than one GiST. `eq` (overlap semantics) needs both bounds, and a single-sided btree probe is selective enough when the query range is narrow. The commit message `35fa56b` mentions a "date ge/le" fix; verify that predicates still match.
- *String:* a trigram GIN is required for `:contains`. For the default FHIR string semantics (case- and accent-insensitive *prefix*), a btree with `text_pattern_ops` (or `COLLATE "C"`) over a normalized `lower(unaccent)` first-token array cannot index arrays either. HAPI handles this with a side table, which is the HAPI model. Keep the trigram GIN, but consider making it optional per param with a cheaper btree on the normalized first value for single-valued strings (e.g. `Organization.name`).
- *Composite params:* multicolumn btree on two extracted scalars. With PG18 skip scan, an index `(code, value)` also serves value-only filters when `code` is low-cardinality.
- *Expression statistics (`CREATE STATISTICS ON (expr)`):* lets OctoFHIR get density-aware estimates for *non-indexed* params with zero write cost; only ANALYZE pays. This targets exactly the rare-vs-common token problem behind seq scans under LIMIT. It works only if the query predicate contains the same expression, so it suits the deterministic extraction functions already generated (`fhir_extract_ref`, the date min/max functions). For indexed expressions, ANALYZE already collects stats on the index expression. Reapply after `pg_upgrade` to 18, because extended stats are not preserved.
- *Custom plans:* keep unnamed/custom plans. FHIR search density varies by literal (common vs rare code), which is the case where a generic plan is wrong. Measure planning cost with `pg_stat_statements` (`track_planning = on`). Do not set `force_generic_plan`.
- *ORDER BY `_lastUpdated` + LIMIT:* without an index on `updated_at`, the planner must sort all matches (top-N heapsort). That is cheap when the filter is selective and expensive on broad filters. Adding a btree on `updated_at` makes broad sorted searches fast but invites the pganalyze pathology on rare filters. Because tables are already non-HOT when indexed, it only costs about +1 btree insert per write (~1.4 µs). It *does* cost HOT on tables that would otherwise be HOT-eligible (see section 1). Make it opt-in per type.
- *Keyset vs OFFSET:* OFFSET reads and discards m rows, so deep pages are O(m). Keyset (`WHERE (sortkey, id) > ($last_sort, $last_id)`) is O(page) and also safe against concurrent inserts shifting pages. With no `_sort`, OctoFHIR emits no ORDER BY. If no caller adds one, OFFSET paging over an unordered result can duplicate or skip rows between pages. That is a correctness issue worth checking: add `ORDER BY id` (UUIDv7 gives recency order and uses the PK) or keyset on `id`.

### Gaps
- No primary-source numbers were found for PG18 skip scan, the OR-to-ANY transformation, or self-join elimination; only qualitative release-note text exists.
- No external benchmark compares GIN-fastupdate pending-list size with search latency; OctoFHIR should measure this itself.
- The `gin_pending_list_limit` default (4 MB) comes from the PG runtime-config docs, which I did not re-fetch in this session: https://www.postgresql.org/docs/18/runtime-config-client.html#GUC-GIN-PENDING-LIST-LIMIT
- The keyset/OFFSET complexity claim is standard (e.g. https://use-the-index-luke.com/no-offset) but was not fetched here.

## 3. Partitioning (by type, by hash, by time for Observation)

### Takeaway
OctoFHIR already partitions by resource type (one table per type), which is what Microsoft does with `PartitionScheme_ResourceTypeId`. Hash or time partitioning of *current* resource tables gives no pruning for typical FHIR searches, and time partitioning breaks enforcement of `id` uniqueness, so reject it for the current table. Time or `txid` range partitioning is viable only for append-only `_history`.

### Cited Findings
- Microsoft partitions resource and search-param tables by `ResourceTypeId` (`CREATE PARTITION SCHEME PartitionScheme_ResourceTypeId`) — [microsoft/fhir-server 80.sql](https://github.com/microsoft/fhir-server/blob/main/src/Microsoft.Health.Fhir.SqlServer/Features/Schema/Migrations/80.sql).
- "A generated column cannot be part of a partition key" — [PG18 docs: generated columns](https://www.postgresql.org/docs/18/ddl-generated-columns.html).
- PG17: "Improve optimization of the LIMIT clause on partitioned tables, inheritance parents, and UNION ALL queries"; exclusion constraints and identity columns are allowed on partitioned tables — [PG17 release notes](https://www.postgresql.org/docs/17/release-17.html).
- PG18 improves planning efficiency for many partitions and partition cost estimates, and adds `VACUUM/ANALYZE ONLY` for partitioned parents. It also notes that "autovacuum does not process partitioned tables, just its children" — [PG18 release notes](https://www.postgresql.org/docs/18/release-18.html).
- PG19 RC1 adds `MERGE PARTITIONS` / `SPLIT PARTITIONS` (per the beta announcement) and `COPY TO` on partitioned tables — [PG19 beta 2 news](https://postgresql.org/about/news/postgresql-19-beta-2-released-3350); [PG19 release notes (RC1)](https://www.postgresql.org/docs/19/release-19.html).

### Inferences
- **Hash on `id`:** no pruning for search (predicates are on `resource`), so every search probes N partitions' indexes. It mainly gains parallel autovacuum and smaller per-partition GIN pending lists. PG19's parallel autovacuum workers reduce even that benefit. Not recommended for the benchmark.
- **Time partitioning of Observation by effective date:** the date lives in `resource`. It would need a real (non-generated) column set on write, and updates that change the date become cross-partition row moves (delete+insert). The PK must include the partition key (documented PG rule, not fetched here: https://www.postgresql.org/docs/18/ddl-partitioning.html), so `id` uniqueness across partitions becomes unenforceable by the database. **Rejected as unsafe** for current-resource tables: a duplicate `id` would produce wrong reads.
- **History tables:** PK `(id, txid)` already includes `txid`, so `RANGE (txid)` partitioning keeps uniqueness and makes retention a cheap DETACH/DROP. It is worth it only for very large history; it does not help the write/search benchmark.

### Gaps
- No FHIR-specific benchmark of partitioned vs unpartitioned per-type tables on PG17/18 was found.

## 4. PostgreSQL 17/18 (and 19 RC) features relevant to search and writes

### Takeaway
PG18's biggest measurable win is asynchronous I/O for cold-cache sequential and bitmap-heap scans: 2–3x in pganalyze's test, "up to 3x" in the PG18 announcement. Both apply to OctoFHIR's GIN/GiST bitmap scans and unindexed seq scans, but not to writes. Virtual generated columns cannot be indexed in 18 or 19. PG17 gives more efficient vacuum memory and up to 2x WAL throughput under high concurrency. PG19 (RC1 as of 2026-10-15) makes lz4 the default TOAST compression and adds parallel autovacuum.

### Cited Findings
**PG18 (released 2025-09-25)**
- **Asynchronous I/O:** "allows backends to queue multiple read requests, which allows for more efficient sequential scans, bitmap heap scans, vacuums". It is controlled by `io_method`, `io_combine_limit` and `io_max_combine_limit`, and is observable via `pg_aios`. `effective_io_concurrency` and `maintenance_io_concurrency` defaults rose to 16 — [PG18 release notes](https://www.postgresql.org/docs/18/release-18.html).
- `io_method` values are `worker`, `io_uring` and `sync`; the announcement says "up to 3× performance improvements when reading from storage" — [PG18 announcement](https://www.postgresql.org/about/news/postgresql-18-released-3142/).
- pganalyze reports that `worker` is the default, that AIO covers reads only (writes stay synchronous), and that `io_workers` defaults to 3.
- pganalyze's cold-cache `COUNT(*)` on a 3.5 GB table: PG17 15,831 ms; PG18 sync 15,071 ms; worker 10,052 ms; io_uring 5,723 ms. Caveat: "`EXPLAIN ANALYZE` may underreport I/O" because it shows only wait time — [pganalyze: PG18 AIO](https://pganalyze.com/blog/postgres-18-async-io).
- **Other PG18 items** — [PG18 release notes](https://www.postgresql.org/docs/18/release-18.html):
  - btree skip scan;
  - OR-clauses transformed to arrays;
  - self-join elimination (`enable_self_join_elimination`);
  - GIN parallel build;
  - sorted builds for range-type GiST and btree indexes;
  - `EXPLAIN ANALYZE` includes BUFFERS by default;
  - `pg_stat_io` gains `read_bytes`/`write_bytes`/`extend_bytes` and WAL rows;
  - faster, lower-memory hash joins and GROUP BY;
  - NOT NULL constraints stored in `pg_constraint`, with `NOT VALID` support;
  - `initdb` enables data checksums by default (`--no-data-checksums` to opt out; `pg_upgrade` needs matching settings);
  - `pg_upgrade` keeps planner stats except extended stats;
  - eager freezing of all-visible pages (`vacuum_max_eager_freeze_failure_rate`);
  - `autovacuum_vacuum_max_threshold`;
  - `autovacuum_worker_slots`;
  - `OLD`/`NEW` in `RETURNING`;
  - `uuidv7()`;
  - SIMD processing of long JSON strings;
  - `amcheck gin_index_check()`;
  - after `pg_upgrade`, "it is recommended to reindex all indexes related to full-text search and pg_trgm".

**PG17**
- Up to 20x less vacuum memory, no silent 1 GB cap, and "up to 2x better write throughput" for high-concurrency WAL — [PG17 announcement](https://www.postgresql.org/about/news/postgresql-17-released-2936/); [PG17 release notes](https://www.postgresql.org/docs/17/release-17.html).
- Streaming I/O for sequential scans and ANALYZE (`io_combine_limit`) — [PG17 release notes](https://www.postgresql.org/docs/17/release-17.html).
- btree IN-list improvement — [PG17 release notes](https://www.postgresql.org/docs/17/release-17.html).
- `JSON_TABLE`, `JSON_EXISTS/QUERY/VALUE`, and jsonpath `.date()`/`.timestamp_tz()`/`.decimal()` methods — [PG17 release notes](https://www.postgresql.org/docs/17/release-17.html).
- IS [NOT] NULL elimination on NOT NULL columns, and correlated IN subqueries turned into joins — [PG17 release notes](https://www.postgresql.org/docs/17/release-17.html).
- BRIN parallel build; up to 2x faster `COPY` export of large rows — [PG17 announcement](https://www.postgresql.org/about/news/postgresql-17-released-2936/).

**PG19 RC1 (2026-10-15, not final)** — [PG19 release notes (RC1)](https://www.postgresql.org/docs/19/release-19.html)
- Default TOAST compression changes from pglz to lz4, and `wal_compression = on` uses the best available method.
- Parallel autovacuum workers (`autovacuum_max_parallel_workers`) and an autovacuum scoring system.
- Query table scans can set all-visible bits.
- `REPACK (CONCURRENTLY)` replaces `VACUUM FULL`/`CLUSTER`.
- `io_method = worker` auto-scales workers; `EXPLAIN (ANALYZE, IO)`.
- GIN vacuum uses streaming reads; SIMD `COPY FROM`.
- JIT is off by default; `max_locks_per_transaction` default rises to 128.
- No index support for virtual generated columns.

**Local setup**
- The dev compose file pins `postgres:16-alpine` — `docker-compose.yml:3`.

### Inferences
- **AIO helps** GIN/GiST bitmap heap scans (most OctoFHIR indexed searches), seq-scan fallbacks for non-indexed params, vacuum and ANALYZE. **It does not help** index-only/plain index scans in 18 (not listed), WAL, inserts, or GIN pending-list inserts. With `io_uring` on Linux, cold-cache bitmap scans should improve most. On a warm-cache benchmark the gain is small.
- **Self-join elimination and OR-to-ANY:** OctoFHIR's token `:in`/ValueSet expansion emits OR branches (`octofhir.toml`, `max_valueset_expansion = 500`). PG18's OR-to-ANY transformation may turn indexable OR-chains into a single `= ANY` index probe. Check `EXPLAIN` on PG18.
- **Data checksums on by default:** a safety improvement. Its write-path CPU cost is not quantified in the sources found; measure it rather than disabling it.
- **`uuidv7()`:** irrelevant, since ids are generated in Rust as UUIDv7 already. The PK is TEXT because FHIR ids are client-assignable strings, so a `uuid` column type is not an option without a separate surrogate key.
- **Upgrading the benchmark DB from 16 to 18 is the cheapest broad win:** AIO, GIN parallel build, skip scan, OR-to-ANY, PG17 WAL and vacuum memory. After `pg_upgrade`, reindex the trigram GINs per the release note, re-run `CREATE STATISTICS`/ANALYZE, and update to a minor release that includes the March 2026 virtual-column false-error fix.

### Gaps
- I found no primary benchmark numbers for PG18 hash-join, self-join-elimination or OR-to-ANY gains.
- I found no evidence on whether PG18 AIO covers plain index scans; the release notes list seq, bitmap heap and vacuum only.
- I could not confirm the PG19 final release date; as of the RC1 page it was "2026-??-??".

## 5. TOAST behaviour for FHIR resource sizes

### Takeaway
With `toast_tuple_target = 8160`, OctoFHIR stores typical 1–3 KB Observations uncompressed and inline, so reads pay no detoast cost but heap pages hold fewer rows. Large resources such as ExplanationOfBenefit get lz4 compression, which beats pglz on speed, loses ~7% on ratio, and stays faster than uncompressed under parallel load. Any `resource->'x'` access on a compressed or out-of-line value detoasts the whole value.

### Cited Findings
- TOAST triggers only when a row exceeds `TOAST_TUPLE_THRESHOLD` (~2 kB), then compresses or moves values "until the row value is shorter than `TOAST_TUPLE_TARGET`" (default ~2 kB, adjustable per table).
- An UPDATE that does not change out-of-line values incurs no TOAST cost.
- `EXTENDED` compresses first, then moves out of line. `MAIN` moves out of line only as a last resort. Only `EXTERNAL` (uncompressed) allows slice fetches, and only for text/bytea substring.
- Chunks are ~2000 bytes each, behind an 18-byte TOAST pointer.
- Source for all four points: [PG18 docs: TOAST](https://www.postgresql.org/docs/18/storage-toast.html).
- OctoFHIR sets `toast_tuple_target = 8160` and switches `resource` to lz4 only if the server was built with lz4 and the column is not already lz4. Comment: "only rows over ~8KB are compressed at all" — `crates/octofhir-db-postgres/src/schema.rs:51-67, 245-253, 298-300`.
- **credativ JSONB test** (GitHub Archive, one week): lz4 38 GB, pglz 41 GB, uncompressed (EXTERNAL) 98 GB. "LZ4 consistently outperformed uncompressed data" when extracting a single key; pglz was slower than uncompressed and degraded under parallel load beyond the core count — [credativ](https://www.credativ.de/en/blog/postgresql-en/toasted-jsonb-data-in-postgresql-performance-tests-of-different-compression-algorithms/).
- **Fujitsu (PG14):** average compression ratio pglz ~2.23 vs lz4 ~2.07; lz4 inserts ~80% faster (via search summary, not fetched) — [Fastware: lz4 TOAST in PG14](https://www.postgresql.fastware.com/blog/what-is-the-new-lz4-toast-compression-in-postgresql-14). Tiger Data measured a 37% faster total runtime for lz4 on wide text — [Tiger Data](https://www.tigerdata.com/blog/optimizing-postgresql-performance-compression-pglz-vs-lz4).
- PG19 makes lz4 the default TOAST compression — [PG19 release notes (RC1)](https://www.postgresql.org/docs/19/release-19.html).

### Inferences
- **For 1–3 KB resources at target 8160:** no compression, so no CPU on read or write, and a heap of roughly 3–7 tuples per 8 KB page.
  - Lowering the target to the default (~2 KB) would compress 2–8 KB rows inline with lz4. That shrinks the heap (better cache density and seq/bitmap scans) at some CPU cost per read.
  - The trade-off depends on whether the workload is cache-resident. A benchmark that fits in RAM favours the current setting; a larger-than-RAM one may favour 2 KB + lz4, given credativ's finding that lz4 beats uncompressed when I/O-bound.
- **Detoast cost multiplies with accessors:** the search predicates (`fhir_extract_*`, `@>`) and the SELECT of `resource` each detoast a compressed or out-of-line value. Large resources (EOB, Bundle-like DocumentReference) pay this several times per row unless the executor caches the detoasted datum. Not verified; see Gaps.
- **Functional index maintenance:** recomputing an index expression on UPDATE requires the detoasted `resource`, so large resources pay detoast+extract once per indexed param per write.

### Gaps
- I found no source quantifying per-row detoast repetition when one query references a toasted jsonb column in several expressions. The pgsql-hackers "detoast cache" discussions were not fetched.
- I found no FHIR-specific TOAST benchmark (Observation vs EOB size distributions); the Synthea size distribution should be measured locally.

## 6. Which options preserve "no reindex on new SearchParameter", plus recommendations with local experiments

### Takeaway
Everything that keeps `resource` as the matching truth and treats indexes, statistics or side columns as optional accelerators with an exact recheck preserves the property. Extracted search tables, per-param columns, and asynchronous or deferred indexing do not, and asynchronous indexing is rejected outright because it returns wrong results. The recommendations below are ordered by expected write/search payoff, and each comes with a local experiment.

### Cited Findings
- HAPI/Smile, Microsoft/Azure and Firely all require `$reindex` before a new parameter matches existing data — [Smile CDR](https://smilecdr.com/docs/fhir_repository/search_parameter_reindexing.html); [Microsoft Learn](https://learn.microsoft.com/en-ca/azure/healthcare-apis/fhir/how-to-do-custom-search); [Firely](https://docs.simplifier.net/projects/Firely-Server/en/latest/features_core_fhir_platform/customsearchparameters.html).
- OctoFHIR evaluates predicates over JSONB; functional indexes match the predicate expression exactly, so a missing index means a slower plan, not a missed match — `crates/octofhir-db-postgres/src/functional_indexes.rs:1-6`.
- The whole-document GIN is what "keeps a search on a parameter outside `search.indexed_params` from turning into a sequential scan" — `schema.rs:333-337`.
- Expression statistics need no index maintenance — [PG18 docs: CREATE STATISTICS](https://www.postgresql.org/docs/18/sql-createstatistics.html).
- GIN pending lists are always scanned, so they never cause wrong results — [PG18 docs: GIN](https://www.postgresql.org/docs/18/gin.html).
- Virtual generated columns cannot be indexed in PG18 — [EDB](https://enterprisedb.com/index%2ephp/blog/journey-virtual-generated-columns).

### Inferences
**Preserves the property:**
- functional expression indexes (btree/GIN/GiST/trigram);
- partial indexes;
- the document GIN;
- expression `CREATE STATISTICS`;
- BRIN;
- keyset pagination;
- PG18 upgrade features (AIO, skip scan, OR-to-ANY);
- TOAST tuning;
- STORED generated columns or write-time computed columns, such as `ucum` and `profile`, *as long as* matching rechecks `resource`. Adding a new derived column for a new SP still needs DDL plus a table rewrite, so treat it as an opt-in accelerator, never as the sole path.

**Breaks the property:**
- HAPI/Microsoft-style extracted tables and Medplum-style per-param columns, because existing rows have no entries until reindexed;
- any "index lazily in background" scheme.

**Rejected as unsafe:**
- asynchronous or deferred search-index population, because searches miss recently written data;
- `synchronous_commit = off` or `fsync = off`, which can lose acknowledged writes (this belongs to the adjacent durability topic);
- UNLOGGED resource tables, which are truncated on crash;
- time-partitioning current-resource tables, which loses `id` uniqueness enforcement (section 3);
- disabling data checksums to gain write speed;
- removing the exact recheck behind lossy hull, trigram or prefilter indexes.

**Recommendations (each with a local experiment):**
1. **Trim default `indexed_params`, starting with date GiST (15 of 31 defaults).** Replace single-valued date params with two btrees on min/max extraction, or a single btree on the min bound plus the existing exact recheck. Keep GiST only for multi-valued or period dates (`Observation.date` with `effective[x]` Period).
   - *Experiment:* for each of the ~31 defaults, time 100k Synthea Patient/Observation inserts with that one index enabled (pgbench or the k6 CRUD script), and record µs per row and `pg_relation_size` per index. Then run `EXPLAIN (ANALYZE, BUFFERS)` on `birthdate=ge1980`, `date=2020-01` and `date=lt2015` with the btree pair vs GiST, checking result-set equality.
2. **Make the document GIN opt-in per type, and measure HOT on the updated workload.** Without any resource-referencing index, updates become HOT-eligible: `id` is unchanged, `updated_at` is unindexed, and fillfactor 90 is already set.
   - *Experiment:* run the k6 update load on Patient with `document_gin_resource_types = []` and four indexed params, comparing `n_tup_hot_upd / n_tup_upd`, WAL bytes (`pg_stat_wal` / PG18 `pg_stat_io` WAL rows) and throughput against the defaults.
3. **Add expression statistics for frequently searched non-indexed params**, and also for indexed ones where estimates are poor. Example: `CREATE STATISTICS ... ON (fhir_extract_ref(resource, '{...}'))`.
   - *Experiment:* pick a rare token value and a common one (e.g. `Observation.code`) and compare estimated vs actual rows and plan choice under `_count=10`, before and after `CREATE STATISTICS` + ANALYZE. Measure ANALYZE time added.
4. **Fix pagination stability and depth.** Emit `ORDER BY id` when no `_sort` is given (cheap on the PK with UUIDv7 recency), and move `next` links to keyset cursors (`(sort_key, id) > (...)`), keeping OFFSET only as a fallback.
   - *Experiment:* insert concurrently while paging through `Observation?_count=50` to detect duplicates or skips, and measure page-100 latency for OFFSET vs keyset.
5. **Upgrade the dev/bench DB to PostgreSQL 18.** Set `io_method = io_uring` on Linux (worker on macOS/Docker Desktop) and `effective_io_concurrency` 16–64. Upgrade to the latest 18.x for the virtual-column false-error fix. After `pg_upgrade`, reindex the `pg_trgm` GINs and recreate extended statistics.
   - *Experiment:* run the same search mix on PG16 vs PG18 with `io_method = sync`/`worker`/`io_uring`, cold cache (restart and drop OS cache) and warm. Record p50/p99 and `EXPLAIN (ANALYZE, BUFFERS)`, and remember AIO wait-time under-reporting.
6. **Optionally index `_lastUpdated` per type** (btree on `updated_at`, opt-in) for broad `_sort=-_lastUpdated` searches. Do not add it to tables that are HOT-eligible after recommendation 2.
   - *Experiment:* run `Observation?_sort=-_lastUpdated&_count=10` with no other filter, and with a rare filter, with and without the index. Watch for the ORDER BY+LIMIT pathology and confirm the custom plan picks the filter index on rare values.
7. **TOAST target A/B.** Compare `toast_tuple_target` 8160 (current) with 2032 (default) plus lz4 on Observation and ExplanationOfBenefit.
   - *Experiment:* load the Synthea bundle import, then record heap+toast size (`pg_total_relation_size`), cold and warm search latency, and import throughput.
8. **GIN pending-list policy.** Keep `fastupdate = on` with the 30 s cleaner. Set a per-index `gin_pending_list_limit` (e.g. 1–4 MB) on the reference GINs of the highest-write types, so that no insert pays a huge flush.
   - *Experiment:* sample `pgstatginindex()` (pgstattuple) pending pages during the k6 write load alongside search p99 for `Observation?subject=...`, with fastupdate on/off and various limits.
9. **Do not** migrate to extracted search-param tables or per-param columns. They would cost the no-reindex property for a gain mainly in estimate quality, which items 3 and 4 address more cheaply.

### Gaps
- The concrete µs/row costs of trigram GIN and reference GIN inserts in OctoFHIR are not in the brief and need local measurement (experiment 1).
- The planning-time overhead of custom plans for OctoFHIR's large generated SQL (OR-expanded ValueSets) is unmeasured; use `pg_stat_statements` with `track_planning`.
- It is unverified whether any caller adds a default ORDER BY when `_sort` is absent; `sql_builder.rs:1248-1250` alone emits none. Confirm before acting on recommendation 4.
