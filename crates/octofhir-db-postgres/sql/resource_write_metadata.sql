-- Write-time metadata is the source of truth for JSON, columns and HTTP headers.
-- Previewed Bundle writes adopt the same reserved timestamp on final checkout.
CREATE OR REPLACE FUNCTION octofhir_resource_timestamp()
RETURNS TIMESTAMPTZ AS $$
    SELECT date_trunc('milliseconds', COALESCE(
        NULLIF(current_setting('octofhir.write_timestamp', true), '')::timestamptz,
        statement_timestamp()
    ));
$$ LANGUAGE sql STABLE;

-- Row trigger for every UPDATE (stamps id/meta, refuses out-of-order
-- revisions) and for INSERTs whose body does not already carry its id/meta:
-- the storage layer stamps them in SQL, so bulk inserts skip this call.
CREATE OR REPLACE FUNCTION octofhir_set_resource_metadata()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = octofhir_resource_timestamp();
    IF TG_OP = 'UPDATE' AND (NEW.txid <= OLD.txid OR NEW.updated_at < OLD.updated_at) THEN
        RAISE EXCEPTION 'Write metadata precedes the locked resource revision'
            USING ERRCODE = '40001';
    END IF;
    IF TG_OP = 'INSERT' THEN
        NEW.created_at = NEW.updated_at;
    END IF;
    NEW.resource = NEW.resource || jsonb_build_object(
        'id', NEW.id,
        'meta', COALESCE(NEW.resource->'meta', '{}'::jsonb) || jsonb_build_object(
            'versionId', NEW.txid::text,
            'lastUpdated', to_char(NEW.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
        ));
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Legacy per-insert history probe; dropped from every table on boot.
DROP FUNCTION IF EXISTS octofhir_guard_insert_revisions() CASCADE;
