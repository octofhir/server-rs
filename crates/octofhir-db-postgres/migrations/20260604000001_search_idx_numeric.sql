-- Number and Quantity search sidecars.
--
-- These tables replace production JSONB numeric casts for FHIR number and
-- quantity search. Writers pre-extract numeric values into NUMERIC columns so
-- selective comparisons can use B-tree range scans per resource partition.

CREATE TABLE IF NOT EXISTS search_idx_number (
    resource_type   TEXT    NOT NULL,
    resource_id     TEXT    NOT NULL,
    param_code      TEXT    NOT NULL,
    value_num       NUMERIC NOT NULL
) PARTITION BY LIST (resource_type);

CREATE INDEX IF NOT EXISTS idx_search_idx_number_value
    ON search_idx_number (resource_type, param_code, value_num);

CREATE INDEX IF NOT EXISTS idx_search_idx_number_by_resource
    ON search_idx_number (resource_type, resource_id);

CREATE TABLE IF NOT EXISTS search_idx_quantity (
    resource_type   TEXT    NOT NULL,
    resource_id     TEXT    NOT NULL,
    param_code      TEXT    NOT NULL,
    value_num       NUMERIC NOT NULL,
    system          TEXT,
    code            TEXT,
    unit            TEXT
) PARTITION BY LIST (resource_type);

CREATE INDEX IF NOT EXISTS idx_search_idx_quantity_value
    ON search_idx_quantity (resource_type, param_code, value_num);

CREATE INDEX IF NOT EXISTS idx_search_idx_quantity_system_code_value
    ON search_idx_quantity (resource_type, param_code, system, code, value_num)
    WHERE code IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_search_idx_quantity_system_unit_value
    ON search_idx_quantity (resource_type, param_code, system, unit, value_num)
    WHERE unit IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_search_idx_quantity_by_resource
    ON search_idx_quantity (resource_type, resource_id);

COMMENT ON TABLE search_idx_number IS
    'Denormalised number index. value_num stores extracted FHIR number values for prefix/range search without JSONB casts.';

COMMENT ON TABLE search_idx_quantity IS
    'Denormalised quantity index. value_num plus optional system/code/unit support FHIR quantity search without JSONB numeric casts.';
