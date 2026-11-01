# Write path of a durable, high-throughput FHIR server (OctoFHIR on PostgreSQL)

Scope: single-resource CRUD (300 VUs) and transaction-bundle import (20 VUs × 1000 bundles × ~1.6k entries) on PG 18, sqlx 0.9, pool 32, DB 8 CPU. Durability (fsync on, synchronous_commit on, logged tables, full_page_writes on) is a hard constraint.
Code citations are `path:line` against the repo at commit 536cfc2 (2026-10-30). The codegraph MCP server failed to connect, so the code was located with grep.

---

## Q0. What the current write path actually does (code grounding)

### Takeaway
The single-resource fast path is already one autocommit statement per write. The main structural costs are elsewhere: (a) a database-wide advisory "write gate" that every write statement takes shared and every conditional write takes **exclusive**; (b) a plpgsql row trigger on every UPDATE plus a dynamic-SQL statement trigger for history; (c) SQL text that inlines per-request literals (txid, If-Match version), which defeats sqlx's statement cache and costs extra round trips; (d) the fast path applies only when no policy/authorization context is attached.

### Cited Findings
- **Create (fast path)** is a single statement: `WITH new_tx AS (SELECT nextval(...)), stamp AS (...) INSERT ... RETURNING`. It runs autocommit on the pool, so there is no BEGIN/COMMIT round trip — `crates/octofhir-db-postgres/src/queries/crud.rs:92-131`. `create_raw` returns `resource::text` and skips JSONB→serde decode — `crud.rs:162-195`.
- **Update** is a single statement: a `previous AS MATERIALIZED (... FOR UPDATE)` CTE plus `UPDATE ... FROM previous ... RETURNING` new and old — `crud.rs:823-848`. **If-Match inlines the version as a SQL literal** (`format!("AND p.txid = {version}::bigint")`), so every distinct version produces distinct SQL text — `crud.rs:836-841`.
- **Fast-path gate.** The handler takes the fast path only when `authorization.is_none() && condition.is_none()`. The code comment says the locking protocol otherwise "pins a connection for 5+ round trips per write, which under load turns the pool into the bottleneck" — `crates/octofhir-server/src/handlers.rs:1122-1126`. On the fast path, reference checking is a separate pool statement (`exists_many_grouped`) issued before `create_raw` — `handlers.rs:1182-1184`, `handlers.rs:1018-1035`.
- **Reference existence checks** are batched: one `UNION ALL` of `id = ANY($n)` branches, one per referenced type, in one round trip — `crud.rs:280-318`.
- **Database-wide write gate.** `lock_resource_writes` takes `pg_advisory_xact_lock_shared(1330006098, 1)` for ordinary writes and the **exclusive** `pg_advisory_xact_lock(1330006098, 1)` for conditional writes — `crud.rs:35-53`. Every INSERT/UPDATE/DELETE/TRUNCATE statement on every resource table fires a `BEFORE ... FOR EACH STATEMENT` trigger that calls plpgsql `PERFORM pg_advisory_xact_lock_shared(1330006098, 1)` — `schema.rs:256-261`, `crates/octofhir-db-postgres/sql/resource_write_gate.sql:3-9`.
- **Who takes the exclusive gate:**
  - single conditional create/update/delete: `crates/octofhir-server/src/write.rs:1007`
  - the bundle "preview" transaction when the bundle has conditional entries (`ifNoneExist`, conditional PUT/DELETE): `handlers.rs:4973-4997`
  - the final bundle transaction, held for the whole bundle write, when `!conditional_matches.is_empty() || conditional_ref_dependent`: `handlers.rs:5604-5612`

  The code comment says plain conditional references do *not* take it, so that "Synthea-style" imports are not queued — `handlers.rs:5604-5608`.
- **Row-level BEFORE UPDATE trigger** (`octofhir_set_resource_metadata`, plpgsql) fires on every UPDATE row. A BEFORE INSERT row trigger fires only when the WHEN clause detects un-stamped id/meta — `schema.rs:262-279`.
- **History** uses AFTER UPDATE and AFTER DELETE statement triggers `REFERENCING OLD TABLE AS old_rows` — `schema.rs:309-325`. The function uses `EXECUTE format('INSERT INTO %I ... SELECT ... FROM old_rows ON CONFLICT (id, txid) DO NOTHING')`, i.e. dynamic SQL that is re-parsed and re-planned on each firing. DELETE also inserts a tombstone row — `sql/resource_history.sql:3-34`.
- **History tables:** PK `(id, txid)` plus a BRIN index on `(txid, updated_at)` — `schema.rs:283-307`. Main tables use `fillfactor = 90`, aggressive autovacuum, `toast_tuple_target = 8160` and lz4 for `resource`. The code notes that UPDATEs cannot be HOT because `resource` carries index entries — `schema.rs:210-250`.
- **Bundle insert:** one `INSERT ... SELECT FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[])` per type. **The txid is inlined as a literal** (`{txid}::bigint`, `'{txid}'`), and there is a per-row `NOT EXISTS (SELECT 1 FROM {table}_history h WHERE h.id = t.id AND h.txid >= {txid})` probe — `crud.rs:1248-1300`.
- **Bundle update:** shared gate, then `... WHERE id = ANY($1) ORDER BY id FOR UPDATE`, then `UPDATE ... FROM UNNEST` — `crud.rs:1391-1426`.
- **Bundle txid:** allocated with `nextval` + `set_config('octofhir.write_version', ..., true)` in one statement — `crates/octofhir-db-postgres/src/transaction.rs:61-73`.
- **IDs** are UUIDv7 strings stored as `TEXT` (`uuid::Uuid::now_v7().to_string()`) — `crates/octofhir-core/src/id.rs:25-27`. Batch inserts also use `Uuid::now_v7()` — `crud.rs:1259-1266`.
- **Pool:** `max_connections(pool_size)`, `min_connections = pool_size/4`, `test_before_acquire(false)`, no explicit `statement_cache_capacity`, so the sqlx default of 100 applies — `crates/octofhir-db-postgres/src/pool.rs:33-38`. The repo's `octofhir.toml:34` has `pool_size = 50`; the benchmark is stated to use 32.
- **Local docker PG settings** already include `wal_compression=zstd`, `wal_buffers=16MB`, `max_wal_size=8GB`, `checkpoint_completion_target=0.9` and `log_lock_waits=on` — `docker-compose.yml:13-60`. It is unknown whether the benchmark PG 18 instance (shared_buffers 10GB, wal_buffers 256MB) sets `wal_compression`.

### Inferences
- With the exclusive gate, a **single bundle that contains any `ifNoneExist` entry blocks every other writer in the database** (all CRUD writes and all other imports) for the length of that bundle's write. This happens because every write statement needs the shared lock via the statement trigger. Synthea hospital/practitioner bundles use `ifNoneExist`. If the Import benchmark bundles contain such entries, Import becomes a serialized queue. This is the first thing to measure.
- The shared advisory lock itself is cheap. Still, each write statement pays one plpgsql trigger call plus one lock-manager acquisition on a single hot lock tag (fast-path locking does not apply to advisory locks; see Gaps).

### Gaps
- Whether the CRUD benchmark requests carry an `AuthorizedWrite` extension (policy context), which would force the 5+ round-trip slow path, could not be determined statically. It needs a runtime log or counter.
- Whether the Import bundles contain `ifNoneExist` or conditional PUTs is not known from the code.

---

## Q1. COPY vs multi-row INSERT vs INSERT … SELECT FROM UNNEST

### Takeaway
At OctoFHIR's per-statement batch sizes (hundreds to ~1.6k rows per type per bundle), UNNEST is already in the efficient band. Binary COPY's 3.6× win shows up only above ~1,000-row batches, and COPY "performs poorly" at 1,000 rows. COPY via a staging table is possible with sqlx (`copy_in_raw`) but adds a statement and catalog/temp-table costs. Expected gain for this workload is small, so it should be tested, not assumed.

### Cited Findings
- **Tiger Data/Timescale batch-ingest benchmark** (8 CPU / 32 GB, single connection, 1M rows):
  - Binary COPY is "at least 3.6x faster" than other methods above batch size 1,000, and ~19× faster than parameterized INSERT…VALUES.
  - Text COPY is beaten by prepared statements at batches ≤10,000.
  - Both COPY variants "perform poorly" at 1,000 rows.
  - UNNEST at 100k rows is only ~3 ms slower than text COPY.
  - WAL: COPY ≈62 MB vs INSERT ≈109 MB (1.7× more for INSERT).
  - There are no concurrent measurements.

  — [Tiger Data: Benchmarking PostgreSQL batch ingest](https://www.tigerdata.com/blog/benchmarking-postgresql-batch-ingest)
- COPY writes MULTI_INSERT WAL records rather than per-row records. COPY cannot do `ON CONFLICT`; PG17+ text COPY `ON_ERROR IGNORE` only mimics DO NOTHING in a narrow case. COPY cannot be part of a larger query. — [Tiger Data: Benchmarking PostgreSQL batch ingest](https://www.tigerdata.com/blog/benchmarking-postgresql-batch-ingest); [search summary](https://www.tigerdata.com/blog/benchmarking-postgresql-batch-ingest?hl=ja-JP)
- **INSERT…UNNEST vs INSERT…VALUES:** UNNEST was reported 52.97% faster at batch 1,000, attributed to planning cost (VALUES plans every value). A commenter noted that 2.19 s → 1.03 s is really ~113%. UNNEST also avoids the 32,767-parameter limit. — [Timescale: Boosting Postgres INSERT performance by 50% with UNNEST](https://timescale.com/blog/boosting-postgres-insert-performance); [r/PostgreSQL discussion](https://reddit.birdcat.cafe/r/PostgreSQL/comments/1gsynek/boosting_postgres_insert_performance_by_50_with)
- **PostgreSQL docs:** COPY "incurs significantly less overhead for large data loads" and is almost always faster than INSERT even with PREPARE and batching. The WAL-skipping optimisation (COPY into a table created or truncated in the same transaction) applies **only with `wal_level = minimal`**, which "makes any base backups taken before unavailable for archive recovery and standby server, which may lead to data loss." — [PG docs §14.4 Populating a Database](https://www.postgresql.org/docs/current/populate.html)
- **sqlx 0.9 supports COPY IN** via `PgConnection::copy_in_raw(statement) -> PgCopyIn` — `~/.cargo/registry/.../sqlx-postgres-0.9.0/src/copy.rs:34,143`. COPY is **not allowed in libpq pipeline mode** — [PG docs: libpq pipeline mode](https://www.postgresql.org/docs/current/libpq-pipeline-mode.html).
- **Aidbox's 1-billion-resource load** used COPY streaming per thread, with indexes disabled during load (its `disable-idx?` flag is "not recommended for use on live production"). It reached an average of 57k resources/s on 16 vCPU app + Aurora db.r5.4xlarge. Throughput was flat beyond 12 threads, and Java was the bottleneck. The article does not discuss history, validation or durability. — [Health Samurai: How to load 1 billion FHIR resources into Aidbox in 5 hours](https://health-samurai.io/articles/how-to-load-1-billion-fhir-resources-into-aidbox-in-5-hours)

### Inferences
- **COPY into a staging table then `INSERT … SELECT … RETURNING`** inside the bundle transaction is semantically safe: same transaction, same durability, and the final INSERT is fully WAL-logged.
  - Costs: one extra statement per type; the final INSERT still pays full per-row WAL, index and trigger cost; temp-table DDL per transaction churns `pg_class`/`pg_attribute`. A session-lifetime `CREATE TEMP TABLE IF NOT EXISTS ... ON COMMIT DELETE ROWS` per pooled connection avoids repeated DDL.
  - The binary-COPY win in the Tiger benchmark comes from *the COPY into the target table* (MULTI_INSERT WAL, ring buffer). Staging forfeits most of that, because the final write is a normal INSERT. Expect a negligible gain at ~1.6k rows per bundle. The better candidate is binary COPY **directly** into the target table for create-only entries: no RETURNING, server-assigned v7 ids known client-side, and meta stamped client-side.
- Direct COPY into the live table forgoes the per-row history-resurrection guard (`crud.rs:1270-1276`). That guard would need to move to a post-COPY set-based check in the same transaction, which is still safe.

### Gaps
- No public benchmark measures COPY vs UNNEST for JSONB-heavy rows (1–10 KB TOASTed) under 20 concurrent writers. The Tiger benchmark used narrow rows on one connection.

---

## Q2. Pipelining, statement caching and round trips (sqlx 0.9 vs tokio-postgres)

### Takeaway
sqlx 0.9 does not pipeline. Each `execute` ends with a Sync and waits for ReadyForQuery, and a cache miss costs an **extra full round trip** (Parse/Describe/Sync) plus, on LRU eviction, **another** round trip (Close/Sync). The biggest cheap win is making SQL text stable (bind txid and If-Match version instead of inlining them), so that every statement hits the per-connection cache. Pipelining (tokio-postgres) mainly helps multi-statement transactions; the CRUD fast path is already one statement.

### Cited Findings
- **sqlx prepare path:** on a cache miss it sends `Parse` (+`Describe`) then `Sync`, flushes, and awaits `ParseComplete`/`ParameterDescription`/`RowDescription`/`ReadyForQuery` before sending `Bind`/`Execute`. That is a separate round trip — `sqlx-postgres-0.9.0/src/connection/executor.rs:23-80`.
- **sqlx cache eviction:** when inserting into a full statement cache, sqlx writes `Close::Statement` + `Sync`, flushes, and waits for CloseComplete and ReadyForQuery — another round trip — `executor.rs:184-195`.
- **sqlx execution:** each query sends `Bind`, `Execute`, `Close(portal)`, `Sync`. The maintainers' own comment says "Theoretically, we could send dozens of queries before a [Sync] … Some kind of builder pattern that is termed batching might suit this" — `executor.rs:240-276`. Transactions use separate `BEGIN`/`COMMIT` executions — `sqlx-postgres-0.9.0/src/transaction.rs:17-55`.
- **sqlx docs:** `statement_cache_capacity` default is 100 distinct statements per connection, with LRU eviction — [docs.rs PgConnectOptions](https://docs.rs/sqlx/latest/sqlx/postgres/struct.PgConnectOptions.html).
- **tokio-postgres** is "an asynchronous, pipelined" client: "Pipelining happens automatically when futures are polled concurrently" (e.g. `join`). The server still executes the queries serially; pipelining "can improve performance in use cases in which multiple, independent queries need to be executed." — [tokio-postgres docs](https://docs.rs/tokio-postgres/latest/tokio_postgres/)
- **libpq pipeline mode** is most useful when latency is high and with "many small operations … in rapid succession." The docs' example is a 100-statement operation at 300 ms RTT: 30 s unpipelined vs ~0.3 s pipelined. It is "not useful when information from one operation is required by the client to produce the next operation." Each Sync delimits an implicit transaction, and an error aborts everything queued until the next Sync. "The client must not assume that work is committed when it sends a COMMIT." COPY is disallowed. — [PG docs: libpq pipeline mode](https://www.postgresql.org/docs/current/libpq-pipeline-mode.html)
- **sqlx vs tokio-postgres raw throughput:** on 12M-row fetches, sqlx 0.6 took 29.8 s vs tokio-postgres 9.4 s, and sqlx 0.7 26.2 s vs 12.2 s. The maintainer attributed it to ~100× more `read` syscalls from a buffered-stream routine, plus per-column `FromRow` hashmap lookups, *not* pipelining. The issue is still open. — [launchbadge/sqlx#2007](https://github.com/launchbadge/sqlx/issues/2007)
- No sqlx issue or PR adding pipeline mode was found — [sqlx issues search](https://www.github.com/launchbadge/sqlx/issues).

### Inferences
- **Inlined literals cause cache misses:**
  - The **bundle INSERT** inlines `{txid}` (`crud.rs:1275-1296`), so every bundle × resource type is a brand-new statement. For each: one Parse/Describe/Sync round trip, a fresh server-side parse and plan, and an LRU eviction (Close/Sync round trip) once 100 entries have accumulated on that connection. At ~1.6k entries per bundle the planning cost is small relative to execution, but it is pure waste and pollutes `pg_stat_statements`.
  - **If-Match updates** (`crud.rs:836-841`) do the same per request on the CRUD path.
  - Fix: bind `$4::bigint`.
- **Round trips per CRUD op today (fast path, cache hit):**
  - create = 1 (+1 if references present)
  - update = 1
  - delete = see `delete_returning_previous`, `crud.rs:965`
  - read = 1

  Slow path (policy context): ≥5 (BEGIN, lock/select, reference lock, write, COMMIT; the code comment states "5+"). Collapsing the slow path into a single statement (CTE) or pipelining it would matter more than switching drivers.
- **Should the driver switch to tokio-postgres?** It would let the reference-existence check and an independent statement share one network flight. However, the create depends on the reference result (`handlers.rs:1182-1184`), and pipelined dependent statements would need to be expressed in SQL anyway (e.g. `INSERT ... SELECT ... WHERE (SELECT count(*) ... ) = n`). The gain for single CRUD is mostly from fewer *dependent* round trips, which SQL CTEs give without a driver change.
- With DB and app on the same host or LAN, RTT is ~50–200 µs, so round trips matter mainly through **pool occupancy** (connection held during RTT), not latency.

### Gaps
- No TechEmpower or diesel `metrics` numbers specifically comparing sqlx vs tokio-postgres for small-write workloads were retrieved. TechEmpower Rust entries mostly use tokio-postgres or custom drivers, but this is not verified here.

---

## Q3. Trigger cost: row vs statement/transition tables vs writing history in the same statement

### Takeaway
Statement-level transition-table triggers are far cheaper than row triggers for multi-row statements (≈2× vs ≈13× overhead in Cybertec's test). For **single-row CRUD**, however, a statement trigger with a transition table plus dynamic `EXECUTE` is close to the worst case: tuplestore setup plus re-planning on every update. Writing history in the same statement via a data-modifying CTE is equally durable and avoids the trigger machinery entirely. This should be benchmarked as an A/B.

### Cited Findings
- **Cybertec (PG10) bulk-update logging:**
  - no trigger: 295 ms
  - row-level trigger: 3,816 ms (~1,193% overhead)
  - statement-level trigger with transition tables: 642 ms (~117%)
  - rules: faster still

  — [Cybertec: Rules or triggers to log bulk updates](https://www.cybertec-postgresql.com/en/rules-or-triggers-to-log-bulk-updates/)
- **Tiger Data:** a 10M-row single-thread import with a row trigger took 370 s (27k rows/s, ~9× slower than baseline). With a statement trigger and transition tables it took 53 s vs 42 s with no trigger ("7× faster" than row-by-row). — [Tiger Data: Speed up triggers by 7× with transition tables](https://www.tigerdata.com/blog/speed-up-triggers-by-7x-with-transition-tables); [dev.to mirror](https://dev.to/tigerdata/speed-up-triggers-by-7x-with-transition-tables-347l)
- **Small-transaction audit triggers** (row-level) under pgbench added ~0.4%–11.8% latency — [Cybertec: More on Postgres trigger performance](https://www.cybertec-postgresql.com/en/more-on-postgres-trigger-performance/)
- **Statement-level INSERT triggers** measured faster than row-level across row counts — [Pablotron: Postgres trigger tests](https://pablotron.org/2022/01/23/pg-trigger-tests)
- **OctoFHIR today:**
  - every UPDATE runs a plpgsql row trigger (`schema.rs:270-273`)
  - the history statement trigger uses `EXECUTE format(...)` dynamic SQL with `ON CONFLICT DO NOTHING` (`sql/resource_history.sql:9-12`)
  - every write statement also runs the write-gate statement trigger (`schema.rs:256-261`)

  A single-row update therefore runs three trigger invocations: gate, row stamp, and history.
- **Comparable products:**
  - Aidbox stores history in `{type}_history` tables (per the task brief; not independently verified here).
  - HAPI stores every version in `HFJ_RES_VER` (per the task brief; HAPI docs not retrieved).

### Inferences
- **A/B candidate for single update** (same durability, one statement, no triggers):
  ```sql
  WITH previous AS MATERIALIZED (SELECT ... FOR UPDATE),
       hist AS (INSERT INTO "{t}_history" SELECT ... FROM previous),
       ... UPDATE ... RETURNING
  ```
  Meta stamping moves into the UPDATE's SET (as create already does at `crud.rs:110-122`).
  - Keep the triggers only as a safety net for non-storage writers, e.g. a `WHEN (pg_trigger_depth() = 0 AND current_setting('octofhir.history_written', true) IS NULL)` guard, or drop them if all writes go through storage.
  - Data-modifying CTEs all see the same snapshot and execute exactly once.
- **Replacing dynamic `EXECUTE format` with per-table static trigger functions** (generated in `schema.rs`) lets plpgsql cache plans. This is a smaller, lower-risk change.
- **Drop the write-gate statement trigger** if the conditional-write protocol is redesigned (see Q7). It is one plpgsql call plus one heavyweight-lock acquisition on a single global lock tag per write statement.

### Gaps
- There is no published measurement of transition-table overhead for *single-row* statements. It needs a local microbenchmark: pgbench custom script, 1-row UPDATE, trigger vs CTE vs none.

---

## Q4. WAL volume and commit/fsync behaviour

### Takeaway
Under durable settings, WAL bytes (dominated by full-page images after each checkpoint and by JSONB row images written twice, to the main table and to history) and the commit flush rate are the levers. Safe knobs: `wal_compression` (lz4/zstd), long checkpoints (already set), `commit_delay`/`commit_siblings` (still synchronous), sequential keys (already UUIDv7), and fewer indexes. `wal_buffers` beyond the default helps mainly right after checkpoints.

### Cited Findings
- "The first modification of a data page after each checkpoint results in logging the entire page content … a smaller checkpoint interval increases the volume of output to the WAL." — [PG docs: WAL configuration](https://www.postgresql.org/docs/current/wal-configuration.html)
- On busy systems with `full_page_writes`, "setting `wal_buffers` higher will help smooth response times during the period immediately following each checkpoint" — [PG docs: WAL configuration](https://www.postgresql.org/docs/current/wal-configuration.html)
- **`commit_delay`:** "a group commit leader process will sleep after acquiring a lock within XLogFlush, while group commit followers queue up behind the leader." There is no sleep if fewer than `commit_siblings` other sessions have active transactions. The recommended starting point is half the `pg_test_fsync` single-8kB-write flush time. Too high a value can reduce throughput. — [PG docs: WAL configuration](https://www.postgresql.org/docs/current/wal-configuration.html)
- `commit_delay` "is actually a synchronous commit method" — durability is kept — [PG docs: Asynchronous commit](https://www.postgresql.org/docs/current/wal-async-commit.html)
- **Geoghegan's 2012 patch** moved the commit_delay sleep into XLogFlush. He found half the `pg_test_fsync` time best for tpc-b and insert workloads on Linux/fdatasync, but noted the benefit "mostly disappeared" relative to the new 9.2 group-commit code in some runs. — [pgsql-hackers: Uh, I change my mind about commit_delay](https://www.postgresql.org/message-id/CAEYLb_Ww_C6bZTmpJJtOC-EsR+NHFGP4hzQq-q4GVbX5-8iSxA@mail.gmail.com); [pgsql-hackers: Help me develop new commit_delay advice](https://www.postgresql.org/message-id/CAEYLb_WtD+Ew3k6RZuL6ydsowoYbMKJWa6w+cNH-kzngAzN16g@mail.gmail.com)
- **`wal_compression`:** lz4 and zstd for full-page images are available since PG15. In EDB's pgbench test any compression beat none on TPS, with zstd slightly ahead on TPS and compression ratio. Cybertec calls lz4 "usually the sweet spot" and says zstd gives smaller WAL for more CPU. Exact byte counts were not retrievable, and the sources partly conflict on pglz vs lz4 ratio. — [EDB: pick your favorite compression algorithm for your WALs](https://enterprisedb.com/blog/you-can-now-pick-your-favorite-compression-algorithm-your-wals); [Cybertec pgconfigurator: wal_compression](https://pgconfigurator.cybertec-postgresql.com/docs/parameters/wal_compression); [Cybrosys benchmark](https://www.cybrosys.com/research-and-development/postgres/how-to-benchmark-postgresql-walcompression-comparing-off-pglz-lz4-and-zstd)
- **UUIDv4 vs v7 vs bigserial**, from a blog benchmark with unpublished methodology, 100M rows:
  - WAL during load: bigserial 38 GB, UUIDv4 142 GB, UUIDv7 51 GB
  - index size: v4 5.8 GB vs v7 2.6 GB
  - insert throughput: v4 ~45k rows/s vs v7 ~82k rows/s

  Random keys cause page splits and leave pages ~half full. Treat the magnitude as indicative only. — [Techplained: UUIDv7 vs UUIDv4 vs Serial](https://www.techplained.com/uuidv7-vs-uuidv4-vs-serial); [DEV: UUID v4 vs v7 benchmarking](https://dev.to/umangsinha12/postgresql-uuid-performance-benchmarking-random-v4-and-time-based-v7-uuids-n9b); [Scaling Postgres 368](https://scaling-postgres-1.castos.com/episodes/4-times-faster-uuid-primary-keys-scaling-postgres-368)
- **OctoFHIR already generates UUIDv7** (`crates/octofhir-core/src/id.rs:25-27`), stored as 36-char `TEXT` (`schema.rs:236`). INSERT is set-based with MULTI-row statements, but each row is still WAL-logged individually (COPY's MULTI_INSERT path does not apply; see Q1). History rows duplicate the full previous JSONB on every update and delete (`sql/resource_history.sql:9-12`).
- **Import fsync count:** 20 VUs × 1000 bundles = 20,000 commits for the whole run, so commit flushes are negligible for Import; WAL bytes and FPIs dominate. CRUD is ~one commit per write request (autocommit), so commit flushes scale with write req/s and group commit matters there.

### Inferences
- **v7 as TEXT vs native `uuid`:**
  - v7 as TEXT keeps time-ordered btree inserts (lowercase fixed-width hex sorts by time prefix), but 37-byte keys vs 16-byte `uuid` roughly double PK index size and WAL for index entries.
  - Changing the column type is a schema change. FHIR ids are arbitrary strings for client-supplied PUTs, so a native uuid type is not generally possible; a secondary hash or `uuid` column is not worth it.
  - Client-supplied v4 ids (PUT-create in bundles) will still scatter.
- **History PK `(id, txid)`:** inserts follow the id distribution of *updated* resources. For updates of old resources that is effectively random, so it generates FPIs after each checkpoint. Inherent unless history is keyed `(txid)` first. A `(txid, id)` PK plus a separate id index trades one random index for another, so no gain unless id lookups use BRIN or a hash.
- **Checkpoint timing:** with `checkpoint_timeout = 15min` and `max_wal_size = 8GB`, a write-heavy import will likely hit `max_wal_size` before the timeout (watch `checkpoints_req` vs `checkpoints_timed` in `pg_stat_checkpointer`, PG17+). Raising `max_wal_size` (e.g. 32–64 GB) is durable-safe and costs only recovery time and disk.
- **Group commit in CRUD:** with 300 VUs funnelled through a 32-connection pool, ≤32 concurrent committers already batch naturally on WALWriteLock. `commit_delay` may help only if `pg_stat_io`/`pg_stat_wal` show many syncs per second relative to commits.

### Gaps
- No authoritative measurement of `wal_compression` effect on JSONB-heavy FPIs was found; it needs a local measurement.
- Whether the benchmark PG instance has `wal_compression` on is unknown (local `docker-compose.yml:38` has zstd).
- The exact location of WAL sync counters in PG18 (`pg_stat_wal` vs `pg_stat_io` with `object = 'wal'`) was not verified in sources. Check `\d pg_stat_wal` on the instance.

---

## Q5. Connection pool sizing (32 vs 8 DB CPUs)

### Takeaway
The canonical formula gives (2 × 8 cores) + 0 spindles ≈ 16 active connections for a fully cached SSD box. A pool of 32 is 2× that and 50 (repo default) is ~3×. With 300 VUs, a pool sweep (12/16/24/32) is cheap and is backed by large measured gains from smaller pools in other systems. Pool size only matters if connections are busy on the DB, not waiting on round trips or app CPU.

### Cited Findings
- connections ≈ ((core_count × 2) + effective_spindle_count); spindles = 0 when the active set is cached; don't count hyperthreads; SSDs "not analyzed". Excess connections add contention: lock and spinlock, cache-line, context switches, and `work_mem` × N. Example: "10,000 transactions in batches of 5, 10, or 20 is usually faster than running 500 at once." — [PostgreSQL wiki: Number of database connections](https://wiki.postgresql.org/wiki/Number_Of_Database_Connections)
- HikariCP: "You want a small pool, saturated with threads waiting for connections." In the cited Oracle demo, cutting the pool from 2048 to 96 cut response time from ~100 ms to ~2 ms (>50×). A pool is "rarely much more than CPU cores × 2." — [HikariCP: About pool sizing](https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing)
- Aidbox's bulk-load test saw throughput flat beyond 12 threads, with near-linear scaling up to 4 — [Health Samurai 1B resources](https://health-samurai.io/articles/how-to-load-1-billion-fhir-resources-into-aidbox-in-5-hours)
- Aidbox vs HAPI 1,000-patient Synthea import with 20 concurrent threads: Aidbox ≈2,678 resources/s, HAPI ≈2,214 resources/s, on 8 vCPU / 24 GB. This is a vendor (Health Samurai) benchmark. — [Health Samurai: Performance at scale baseline](https://www.health-samurai.io/blog/performance-at-scale-baseline)
- OctoFHIR's slow path deliberately avoids holding connections across round trips, because "under load turns the pool into the bottleneck" — `handlers.rs:1122-1125`

### Inferences
- **Mixed workload:** for CRUD, connection hold time is about RTT + execution, so 32 may be right when RTT is a significant fraction of hold time. For Import (long JSONB-heavy statements, CPU- and WAL-bound), 20 bundles × 1 connection is already above 16 on an 8-CPU DB.
- **Separate pools:** consider a dedicated, smaller pool or semaphore for bundle imports (e.g. 8) so they cannot starve CRUD. This is safe and simple.
- **Per-connection memory:** each backend's baseline is a few MB plus `work_mem` per sort or hash node, plus catalog and relcache for many per-type tables. With ~150+ resource tables plus history tables, relcache per backend grows. Fewer connections means less memory.

### Gaps
- No HAPI or Aidbox documentation on recommended DB pool sizes was retrieved.
- No source quantifies per-backend memory for PG18 with hundreds of tables; measure with `pg_backend_memory_contexts` (PG14+).

---

## Q6. Referential integrity checks

### Takeaway
OctoFHIR already batches existence checks into a single `UNION ALL … id = ANY($n)` round trip (`crud.rs:280-318`). The fast path checks in one pool statement and inserts in another, so a TOCTOU window exists (a referenced resource deleted between check and insert). The safe and also faster option is to fold the check into the INSERT statement itself, or to take `FOR KEY SHARE`-style locks inside one statement.

### Cited Findings
- Batched existence check: one query per write with one branch per referenced type — `crud.rs:280-318`. It is called before `create_raw` as a separate pool statement on the fast path — `handlers.rs:1182-1184`, `handlers.rs:1018-1035`.
- The transactional path locks references (`lock_references_with_tx`, `lock_reference_snapshots_with_tx`, ordered by table and row) — `crud.rs:353-455`.
- PG docs on FKs during bulk load: each row inserted into a table with FKs queues a pending trigger event (memory for millions of rows). Dropping FKs loses error checking while missing — [PG docs §14.4.4](https://www.postgresql.org/docs/current/populate.html). This is relevant if real FKs were ever considered; OctoFHIR is FK-free since references live inside JSONB.

### Inferences
- **Single-statement create-with-check:**
  ```sql
  WITH refs AS (SELECT count(*) AS n FROM (... UNION ALL ...) r)
  INSERT ... SELECT ... WHERE (SELECT n FROM refs) = $expected
  RETURNING ...
  ```
  Then distinguish "0 rows" (a missing reference) with a follow-up only on failure. This cuts one round trip from every create that has references, which is most clinical resources in the 9-type CRUD mix. Race safety vs concurrent delete is the same as today, because there is still no lock; adding `FOR KEY SHARE` on referenced rows closes it at the cost of row-lock WAL.
- **Bundles:** a single `= ANY(array)` per type is already the optimum shape. Ensure that intra-bundle references (urn:uuid) are excluded before querying.

### Gaps
- No external benchmark compares per-reference vs `ANY(array)` existence checks. The improvement is self-evident in round trips but unmeasured here.

---

## Q7. Conditional create/update (If-None-Exist) cost and race safety

### Takeaway
OctoFHIR guarantees race safety for conditional writes with a **single global exclusive advisory lock**. That is correct but serializes the entire database's writes while any conditional write runs. A scoped lock keyed by (resource type, normalized search criteria hash) via `pg_advisory_xact_lock(hashtext(...))` gives the same correctness for identical criteria. A unique "search URL" index table (HAPI-style idea) gives DB-enforced dedupe. Overlapping-but-different predicates are the hard case and justify keeping a slow global path only for them.

### Cited Findings
- **The exclusive global gate:**
  - single conditional writes: `write.rs:1007`
  - bundle preview: `handlers.rs:4992-4997`
  - bundle final write, whenever conditional matches or conditional-ref dependencies exist: `handlers.rs:5604-5612`
  - every ordinary write statement takes the shared side via trigger: `sql/resource_write_gate.sql:3-9`
- **The race is real in other servers:** HAPI users reported duplicate resources from concurrent conditional creates (2017); a FHIR maintainer called duplicate prevention "a business rule." — [FHIR chat: Concurrent conditional create](https://chat-archive.fhir.org/stream/179167-hapi/topic/Concurrent.20.22conditional.20create.22.20requests.20for.20same.20resource.html); [FHIR chat: hapi fhir protect against race conditions](https://chat-archive.fhir.org/stream/179166-implementers/topic/hapi.20fhir.20protect.20against.20race.20conditions.html)

### Inferences
- **Scoped advisory locks:** for the common case (`identifier=system|value`), lock on `hashtextextended(type || normalized_query, 0)` in ascending key order. That serializes only requests with the same criteria.
  - Gap in safety: a concurrent *unconditional* create of a matching resource is not blocked. That is acceptable per FHIR semantics, since conditional create only promises not to duplicate concurrent conditional creates, but this should be a product decision.
  - Correctness is unchanged for identical criteria; hash collisions only add false serialization.
- **Unique-constraint approach:** a `conditional_key(type, criteria_hash) UNIQUE` row inserted in the same transaction (HAPI-like) makes concurrent duplicates fail with 23505 and lets the server retry as a match. It is durable and needs no global lock.
- **Measure before redesigning:** count exclusive-gate acquisitions per run, and sample `wait_event_type = 'Lock' AND wait_event = 'advisory'` in `pg_stat_activity`. If Import bundles carry `ifNoneExist`, this is likely the single largest Import limiter.

### Gaps
- HAPI's current mechanism (reportedly a search-URL uniqueness table) was not confirmed from HAPI docs or source in this session.

---

## Q8. Safe "trusted bulk import" opt-in modes

### Takeaway
Durability-preserving speedups for import:
1. Deferred validation of already-validated or trusted data, an explicit admin opt-in that skips *validation*, never WAL or fsync.
2. Binary COPY directly into target tables for create-only entries.
3. Separate import pool/semaphore.
4. Index build after load **only for empty or new tables**.

UNLOGGED staging plus validate-then-publish is safe for the *published* data, provided the publish step is a normal logged INSERT. Its exact risk is losing the staging contents, not committed data.

### Cited Findings
- PG: build indexes after loading into a fresh table; for existing tables drop, load and recreate, but "think twice before dropping a unique index" (lost error checking) — [PG docs §14.4.3](https://www.postgresql.org/docs/current/populate.html)
- PG: `wal_level = minimal` lets COPY into a table created or truncated in the same transaction skip WAL, but it requires a restart and invalidates prior base backups for archive recovery and standbys ("may lead to data loss") — [PG docs §14.4.7](https://www.postgresql.org/docs/current/populate.html)
- Aidbox: COPY plus index disabling for bulk loads is "not recommended for use on live production" — [Health Samurai 1B resources](https://health-samurai.io/articles/how-to-load-1-billion-fhir-resources-into-aidbox-in-5-hours)
- Aidbox exposes `$load` (synchronous) and `$import` (asynchronous) for bulk data — [Aidbox API overview](https://www.health-samurai.io/docs/aidbox/api/api-overview)
- OctoFHIR already supports `X-Skip-Validation` (`should_skip_validation`, `handlers.rs:1158-1165`; `crates/octofhir-server/src/handlers.rs` skip path)

### Inferences
- **UNLOGGED staging:** `COPY` → `UNLOGGED import_stage` → validate in SQL/Rust → `INSERT INTO live SELECT ... FROM stage` (logged) → `TRUNCATE stage`.
  - Risk: on crash the stage is truncated (by design), so in-flight, not-yet-published batches must be re-sent.
  - Committed live data is fully durable.
  - Stage contents are not replicated to standbys.
  - The live INSERT still pays full WAL, so the benefit is only in the validation and dedupe phase. Worth it only if validation needs multiple passes over the data.
- **Deferred validation** (accept now, validate async and flag) changes API semantics: invalid data becomes visible. Make it an explicit `$import` job mode, not the default transaction-bundle path.
- **Never for live data:** `ALTER TABLE ... SET UNLOGGED`, `wal_level=minimal` on a server with replicas or PITR, or disabling unique/PK indexes on populated tables.

### Gaps
- No HAPI or Medplum bulk-import internals (e.g. HAPI `$import` batch2 job settings) were retrieved.

---

## Q9. Unsafe options — evaluated and rejected

### Takeaway
Reject `synchronous_commit=off` (even per-transaction), `fsync=off`, `full_page_writes=off`, and UNLOGGED tables for live data. Each either loses acknowledged writes on crash or risks corruption. `commit_delay` is the durable alternative for group-commit gains.

### Cited Findings
- **`synchronous_commit = off`:**
  - Selectable per transaction: "the commit mode of each transaction … depends on the value of synchronous_commit when transaction commit begins."
  - On crash in the risk window, "changes made during that transaction will be lost" — "loss of the last few transactions."
  - The risk window is up to 3 × `wal_writer_delay`.
  - An immediate shutdown counts as a crash.

  For a FHIR server this means a client got 201 Created and the resource is gone, a violation of the stated durability constraint. — [PG docs: Asynchronous commit](https://www.postgresql.org/docs/current/wal-async-commit.html)
- **`fsync = off`** is server-wide; a crash "could result in arbitrarily bad corruption of the database state" — [PG docs: Asynchronous commit](https://www.postgresql.org/docs/current/wal-async-commit.html)
- **`commit_delay`** "is actually a synchronous commit method" — the safe alternative — [PG docs: Asynchronous commit](https://www.postgresql.org/docs/current/wal-async-commit.html)
- **`wal_level = minimal` for WAL-skipping COPY** invalidates earlier base backups for archive recovery and standbys ("may lead to data loss") — [PG docs §14.4.7](https://www.postgresql.org/docs/current/populate.html)
- **`full_page_writes`** exists "to ensure data page consistency"; turning it off risks torn-page corruption on crash — [PG docs: WAL configuration](https://www.postgresql.org/docs/current/wal-configuration.html)
- **Unlogged tables** are not WAL-logged, are truncated automatically after a crash or unclean shutdown, and are not replicated to standbys — [PG docs: CREATE TABLE (UNLOGGED)](https://www.postgresql.org/docs/current/sql-createtable.html). This is from known doc content; the page was not fetched in this session.

### Inferences
- Any benchmark comparison against servers that use these settings (or Aurora/managed-storage semantics) should be flagged as not apples-to-apples.

### Gaps
- None material.

---

## Q10. Concrete local experiments (ordered by expected value / cost)

### Takeaway
Measure first: WAL bytes and FPIs per op, wait events, and gate contention. Then A/B the cheap safe changes one at a time on the same dataset and PG config.

### Cited Findings
(Methods rely on the sources above; the experiments are proposals.)

### Inferences
1. **Instrument a baseline (both benchmarks).**
   - Reset and snapshot `pg_stat_wal` (`wal_records`, `wal_fpi`, `wal_bytes`, `wal_buffers_full`), `pg_stat_checkpointer` (`num_timed` / `num_requested`), and `pg_stat_io` (WAL rows) before and after.
   - Enable `pg_stat_statements` and get the top-10 statements by `total_exec_time` and `calls`; also check plan count, which shows inlined-literal churn.
   - Sample `pg_stat_activity.wait_event` every 100 ms: look for `Lock/advisory`, `LWLock/WALWrite`, `LWLock/WALInsert`, `IO/WALSync`, `LWLock/BufferMapping`, `Client/ClientRead` (the last means the app is the bottleneck).
   - Compute WAL bytes per resource and FPI share.
2. **Gate contention.** Count calls to `lock_conditional_writes` (add a tracing counter) and check whether Import bundles contain `ifNoneExist`.
   - If yes: A/B a scoped advisory key per (type, criteria hash) vs the global key (`crud.rs:35-53`, `handlers.rs:5604-5612`).
   - Also A/B removing the per-statement `octofhir_write_gate` trigger (`schema.rs:256-261`) once scoped locks are in place.
3. **Stable SQL text.** Bind txid in `create_batch_with_tx` (`crud.rs:1275-1296`) and the If-Match version in `update_snapshot` (`crud.rs:836-841`) instead of inlining.
   - Verify via `SELECT count(*) FROM pg_prepared_statements` per connection and `pg_stat_statements` distinct query ids.
   - Optionally raise `statement_cache_capacity` (sqlx default 100) if more than 100 distinct statements are hot across ~150 types × operations.
4. **History via CTE instead of triggers** for single update and delete. pgbench custom script with a 1-row UPDATE on a 4 KB JSONB table, three variants:
   - (a) current triggers
   - (b) static per-table trigger function (no `EXECUTE format`)
   - (c) data-modifying CTE, no triggers

   Report TPS and WAL bytes per op.
5. **Fast path coverage.** Log or count which create/update path each CRUD request takes (`handlers.rs:1126`). If the benchmark runs with a policy context, collapse the slow path into a single CTE statement (check + lock + insert).
6. **Pool sweep:** `pool_size` ∈ {12, 16, 24, 32, 48} for CRUD, and a separate import semaphore ∈ {4, 8, 12, 20}. Record req/s, p99 and DB CPU.
7. **WAL knobs** (one at a time, durable):
   - `wal_compression` ∈ {off, lz4, zstd}
   - `max_wal_size` ∈ {8, 32, 64 GB}, watching `num_requested` checkpoints
   - `commit_delay` ∈ {0, ½·pg_test_fsync, pg_test_fsync} with `commit_siblings` ∈ {5, 10}, for CRUD only
   - `wal_buffers` 64MB vs 256MB (should show no gain unless `wal_buffers_full > 0`)
8. **Binary COPY for create-only bundle entries** (sqlx `copy_in_raw`) directly into the target table vs the current UNNEST. Measure resources/s and WAL bytes. Expect a small difference at ~1.6k entries per bundle (Tiger: COPY weak at 1k-row batches).
9. **Fold reference check into the create statement** (Q6). Measure CRUD create latency and pool wait time (`sqlx` acquire time histogram).
10. **Index audit** on a few hot types:
    - `pg_stat_user_indexes.idx_scan = 0` during the benchmark, plus the size and count of functional and GIN indexes; `indexed_params` gates them per memory notes.
    - Each extra index on a JSONB-updated table means another WAL record and FPI on every update.
    - GIN `fastupdate` pending-list behaviour also affects write latency spikes.

### Gaps
- None of the experiments have been run; all expected gains above are hypotheses grounded in the cited sources and code reading.
