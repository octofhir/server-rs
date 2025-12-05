-- Migration: 006_temp_valueset_tables.sql
-- Large ValueSet expansion optimization using temporary tables

CREATE UNLOGGED TABLE IF NOT EXISTS temp_valueset_codes (
    session_id TEXT NOT NULL,
    code TEXT NOT NULL,
    system TEXT,
    display TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_temp_valueset_session_code
ON temp_valueset_codes(session_id, code, system);

CREATE INDEX IF NOT EXISTS idx_temp_valueset_cleanup
ON temp_valueset_codes(created_at);

CREATE OR REPLACE FUNCTION cleanup_temp_valueset_codes()
RETURNS void AS $func$
BEGIN
    DELETE FROM temp_valueset_codes
    WHERE created_at < NOW() - INTERVAL '1 hour';
END;
$func$ LANGUAGE plpgsql;
