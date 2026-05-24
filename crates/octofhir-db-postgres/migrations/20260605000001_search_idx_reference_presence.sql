CREATE INDEX IF NOT EXISTS idx_ref_presence
    ON search_idx_reference (resource_type, param_code, resource_id);
