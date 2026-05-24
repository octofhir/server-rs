# Index Advisor Design

This document tracks the workload-driven index advisor for OctoFHIR.

## Baseline

OctoFHIR intentionally uses one PostgreSQL table per FHIR resource type.
That shape is kept as the baseline because it gives natural resource-type pruning,
smaller per-table indexes, simpler operational isolation for large types such as
`Observation`, and predictable DDL per resource type.

The advisor must work with this model. It should not recommend collapsing all
resources into one table, and it should not recommend broad sharding as the first
answer.

## API Surface

Initial read-only endpoints:

- `GET /api/db-console/index-advisor?resourceType=Observation`
- `POST /api/db-console/index-advisor/analyze`

`GET` inspects PostgreSQL metadata and observed SQL from `pg_stat_statements`
when available.

`POST` accepts real FHIR request shapes supplied by the user:

```json
{
  "resourceType": "Observation",
  "queries": [
    "GET /fhir/Observation?subject=Patient/123&code=http://loinc.org|8867-4",
    "GET /fhir/Observation?date=ge2024-01-01&_sort=-_lastUpdated"
  ]
}
```

The advisor returns candidate recommendations and SQL text, but never executes
DDL. Creating indexes remains an explicit operator action.

## Recommendation Rules

### Safe Resource-Table Recommendation

For a concrete resource table, recommend:

```sql
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_<table>_active_updated_id
ON "<table>" (updated_at DESC, id)
WHERE status != 'deleted';
```

Reason: default search, history, and export paths need stable keyset pagination
on active current resources. This is a narrow B-tree and has much lower write
amplification than JSONB expression indexes.

### Reference Search

Do not recommend per-resource JSONB reference indexes.

Reference traffic should use `search_idx_reference`, because the same projection
powers:

- reference search
- chained search
- `_include`
- `_revinclude`

If reference queries are slow, fix query routing, fanout caps, or projection
freshness before adding extra resource-table indexes.

### Date Search

Date traffic should use `search_idx_date`.

If the advisor sees repeated date searches, it should first flag read-path
routing to `search_idx_date`. Adding more indexes to the resource JSONB table is
usually the wrong first fix because date projection rows are already written on
the write path.

### Token/String/Quantity Search

For repeated token, string, quantity, or number searches, the advisor should
prefer sparse projection-table recommendations over broad JSONB indexes.

Do not auto-generate expression indexes from `SearchParameter.expression` until
the expression has a proven SQL mapping. Wrong expression indexes are dangerous:
they can be expensive, incomplete, and semantically incorrect for FHIR arrays and
choice types.

### Index Hygiene

The advisor may flag large indexes with zero recorded scans, but it must label
them as review-only. `pg_stat_user_indexes` resets after PostgreSQL restart, and
rare critical queries may still need an index.

## Next Steps

1. Add frontend panel in DB Console for the advisor response.
2. Feed advisor with normalized FHIR search telemetry, not only submitted
   request examples and `pg_stat_statements`.
3. Add EXPLAIN integration for candidate SQL shapes.
4. Add projection recommendations for token/string/quantity with estimated write
   amplification.
5. Add tests for FHIR request parsing and recommendation stability.
