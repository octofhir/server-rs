-- String search sidecar — `search_idx_string`.
--
-- Default and `:contains` string search use `pg_trgm` GIN over a
-- normalised text projection. The previous JSONB-only path
-- (`f_unaccent_lower(jsonb_path) LIKE …`) has no functional index for
-- trigram patterns and sequentially scans the resource partition.
--
-- Layout mirrors `search_idx_date` / `search_idx_reference`:
-- partitioned by `resource_type`, populated synchronously by the writer
-- in the same transaction as the resource INSERT/UPDATE. Partitions are
-- materialised lazily by `ensure_search_partition*`.

CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- `btree_gin` lets the multicolumn GIN index combine equality on
-- `param_code` with the trigram opclass on `value_norm`. Without it, GIN
-- would only index one column, forcing a separate filter for `param_code`
-- and reading more index pages than necessary.
CREATE EXTENSION IF NOT EXISTS btree_gin;

-- Sidecar table.
--
-- `value_norm` is `f_unaccent_lower(value_exact)` precomputed by the
-- writer. Trigram GIN lives on this column. `value_exact` keeps the
-- original case + diacritics for `:exact` semantics and any future
-- `ORDER BY` paths.
CREATE TABLE IF NOT EXISTS search_idx_string (
    resource_type   TEXT NOT NULL,
    resource_id     TEXT NOT NULL,
    param_code      TEXT NOT NULL,
    value_norm      TEXT NOT NULL,
    value_exact     TEXT NOT NULL
) PARTITION BY LIST (resource_type);

-- Default prefix and `:contains` modifier — trigram GIN over normalised text.
-- `(param_code, value_norm gin_trgm_ops)` lets the planner combine the
-- equality on `param_code` with the trigram match into one Bitmap Index
-- Scan per partition.
CREATE INDEX IF NOT EXISTS idx_search_idx_string_norm_trgm
    ON search_idx_string
    USING gin (param_code, value_norm gin_trgm_ops);

-- `:exact` modifier — btree on the exact value, useful for equality and
-- range/prefix `ORDER BY` if we add one. Kept narrow for write cost.
CREATE INDEX IF NOT EXISTS idx_search_idx_string_exact_btree
    ON search_idx_string (resource_type, param_code, value_exact);

-- Composite key for cheap `DELETE` on resource update / delete.
CREATE INDEX IF NOT EXISTS idx_search_idx_string_by_resource
    ON search_idx_string (resource_type, resource_id);

COMMENT ON TABLE search_idx_string IS
    'Denormalised string index. value_norm is unaccented-lowercase for trigram match; value_exact preserves the original for :exact. Partitioned by resource_type.';
