# 10GB Observation Ingestion Plan

This document defines the execution plan for bulk loading approximately 10GB of `Observation` resources while preserving search correctness and predictable performance.

## Goals

1. Load the dataset without breaking reference/date search semantics.
2. Keep index tables (`search_idx_reference`, `search_idx_date`) consistent with stored resources.
3. Provide an operational recovery path if index drift is detected.
4. Keep runtime impact bounded during ingestion.

## Non-Goals

1. Re-introducing a separate string index table.
2. Designing a generic ETL platform in this iteration.

## Assumptions

1. PostgreSQL is the primary storage.
2. Ingestion goes through application write paths (`create`/`update`), not direct SQL inserts into resource tables.
3. Dataset quality is trusted enough to optionally use `X-Skip-Validation: true`.

## Phase 0: Preflight (Before Loading)

1. Capacity check:
   - Reserve at least 3x dataset size in free disk space for table growth, indexes, WAL, and VACUUM overhead.
2. Baseline metrics:
   - Record current counts of `Observation`, `search_idx_reference`, `search_idx_date`.
   - Capture baseline latency for representative queries.
3. Access control:
   - Ensure human/operator roles do not perform ad-hoc `DELETE/UPDATE` on `search_idx_*` in production.
4. Route check:
   - Use FHIR API path prefix (`/fhir/...`) for load traffic.

## Phase 1: Correctness Hardening (Mandatory)

1. Introduce strict indexing mode for writes:
   - `create/update` must not end in a committed resource without index refresh.
2. Make write atomicity explicit:
   - Preferred: resource write + index refresh in the same DB transaction.
   - Fallback: compensating rollback/delete when index write fails.
3. Add reindex capability:
   - Reindex one resource (`resourceType + id`).
   - Reindex whole resource type (batched).
   - Reindex all types (batched, resumable).
4. Add index consistency checks:
   - Detect resources that have searchable reference/date fields but missing rows in `search_idx_*`.
5. Expose operational controls:
   - Admin command/endpoint for reindex start/status.
   - Metrics for indexed rows, reindex progress, and drift count.

## Phase 2: Load Execution

1. Load strategy:
   - Start with moderate parallelism (for example 4 workers), then scale up based on DB telemetry.
2. Request format:
   - Use authenticated writes through `/fhir/{resourceType}/{id}`.
   - For trusted data, include `X-Skip-Validation: true` to reduce CPU cost.
3. Throttling:
   - Keep p95 write latency stable; reduce concurrency if lock waits or WAL pressure grow.
4. Checkpoints:
   - Every fixed batch size (for example each 100k resources), persist progress marker and run quick consistency checks.

## Phase 3: Post-Load Verification

1. Count verification:
   - Verify total loaded `Observation` count.
2. Index presence checks:
   - Validate that observations with `subject.reference` have rows in `search_idx_reference`.
   - Validate that observations with `effectiveDateTime` or `effectivePeriod` have rows in `search_idx_date`.
3. Search smoke tests:
   - `subject` search.
   - date range search.
   - `_include`/`_revinclude` scenarios touching loaded observations.

### SQL: Missing Reference Index Rows (subject/patient)

```sql
SELECT o.id
FROM observation o
WHERE o.resource ? 'subject'
  AND (o.resource->'subject' ? 'reference')
  AND NOT EXISTS (
      SELECT 1
      FROM search_idx_reference sir
      WHERE sir.resource_type = 'Observation'
        AND sir.resource_id = o.id
        AND sir.param_code IN ('subject', 'patient')
  )
LIMIT 100;
```

### SQL: Missing Date Index Rows

```sql
SELECT o.id
FROM observation o
WHERE (o.resource ? 'effectiveDateTime' OR o.resource ? 'effectivePeriod')
  AND NOT EXISTS (
      SELECT 1
      FROM search_idx_date sid
      WHERE sid.resource_type = 'Observation'
        AND sid.resource_id = o.id
        AND sid.param_code = 'date'
  )
LIMIT 100;
```

## Phase 4: Recovery and Operations

1. If drift is detected:
   - Run reindex for affected resource type.
   - Re-run consistency queries until zero misses.
2. If ingestion throughput collapses:
   - Reduce worker count.
   - Inspect locks, WAL generation, autovacuum lag, and checkpoint behavior.
3. If disk pressure rises:
   - Pause ingestion.
   - Stabilize DB (VACUUM/maintenance), then resume from checkpoint.

## Acceptance Criteria

1. No missing rows in consistency checks after ingestion completion.
2. Reindex flow is tested and documented (resource, type, full).
3. Reference/date search behavior matches baseline correctness expectations.
4. Operational runbook exists for drift recovery.

## Implementation Backlog (Proposed Order)

1. P0: Strict indexing mode with atomic write semantics.
2. P0: Reindex API/CLI + batched worker.
3. P0: Consistency checker SQL + metrics and alerting.
4. P1: Automated nightly drift check in non-prod and prod.
5. P1: Load-test profile for 10GB Observation dataset and tuning baseline.
