# Search Indexing Design (Current)

This document describes the current physical indexing model used by OctoFHIR search in PostgreSQL.

## Goals

1. Keep common FHIR search operations fast (`reference`, `_include`, `_revinclude`, date ranges).
2. Avoid unnecessary data duplication for string-heavy fields.
3. Keep write path deterministic: index rows are always derived from resource JSON.

## What Exists

OctoFHIR uses two denormalized search tables (partitioned by `resource_type`):

- `search_idx_reference`
- `search_idx_date`

There is intentionally no `search_idx_string` table in the current design.

## Why This Split

### References

Reference lookups power:

- `reference` parameters
- `_include`
- `_revinclude`
- chained searches

These are join-heavy operations and benefit strongly from normalized rows + BTREE scans.

### Dates

FHIR dates have precision semantics (year/month/day/instant) and prefixes (`eq`, `lt`, `ge`, etc.).
We normalize each date/period into a `(range_start, range_end)` pair and query overlap/range conditions on indexed columns.

### Strings

String fields can be large, repeated, and multi-path. Duplicating all extracted strings into a dedicated table caused storage growth without enough read-path benefit.

Current strategy:

- `:exact` uses JSONB containment (`resource @> ...`) and GIN.
- default prefix/contains use JSON path traversal + `LIKE`/`ILIKE`.

## Physical Schema

Defined in:

- `crates/octofhir-db-postgres/migrations/20241213000001_consolidated_schema.sql`

Search index section creates:

- `search_idx_reference` + targeted BTREE indexes by `ref_kind`
- `search_idx_date` + range/sort BTREE indexes

## Write Path

On `create`/`update`:

1. Load applicable `SearchParameter` definitions for resource type.
2. Extract:
   - normalized references (`extract_references`)
   - normalized date ranges (`extract_dates`)
3. Replace rows for that resource in:
   - `search_idx_reference`
   - `search_idx_date`

On `delete`:

1. Delete rows for that resource from both tables.

Main files:

- `crates/octofhir-db-postgres/src/storage.rs`
- `crates/octofhir-db-postgres/src/search_index.rs`
- `crates/octofhir-core/src/search_index.rs`

## Read Path

### Uses `search_idx_reference`

- `reference` search type
- include/revinclude resolution
- chaining / reverse chaining

### Uses `search_idx_date`

- all `date` search paths (including polymorphic date fields)

### Uses JSONB path SQL (not denormalized table)

- string search (`default`, `:contains`)
- string `:exact` (GIN containment)
- most token/number/quantity paths

Main files:

- `crates/octofhir-search/src/types/reference.rs`
- `crates/octofhir-search/src/types/date.rs`
- `crates/octofhir-search/src/types/string.rs`

## Partitioning

Per-resource-type partitions are created for index tables by `SchemaManager`.

Current partition targets:

- `search_idx_reference`
- `search_idx_date`

File:

- `crates/octofhir-db-postgres/src/schema.rs`

## Operational Runbook

### Verify search index tables

```sql
\dt search_idx*
```

Expected:

- `search_idx_reference` (partitioned table)
- `search_idx_date` (partitioned table)

### Check row volume quickly

```sql
SELECT 'reference' AS kind, count(*) FROM search_idx_reference
UNION ALL
SELECT 'date' AS kind, count(*) FROM search_idx_date;
```

### Validate migration path

Server startup should log:

- `Running database migrations (embedded)`
- `Found 1 migration(s) to apply` (on clean DB)
- `Database migrations completed successfully`

## Trade-Offs

Pros:

- Fast reference/date queries where it matters most.
- Lower storage overhead than indexing all strings in a separate table.
- Simpler operational model (fewer large denormalized tables to maintain).

Cons:

- String prefix/contains still rely on JSON traversal and may need workload-specific tuning.
- Some workloads may still require custom expression/trigram indexes.

## Guidance for Future Changes

If adding a new denormalized index table, require all of the following:

1. Demonstrated read-path bottleneck with measurements.
2. Clear query routing in `octofhir-search` type handlers.
3. Explicit write-path extraction + cleanup behavior.
4. Capacity estimate (storage growth + write amplification).
5. Migration and rollback strategy.

## Related Internal Plans

1. [10GB Observation Ingestion Plan](./observation-10gb-ingestion-plan.md)
