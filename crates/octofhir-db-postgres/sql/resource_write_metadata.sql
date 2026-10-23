-- Write-time metadata is the source of truth for JSON, columns and HTTP headers.
-- Previewed Bundle writes adopt the same reserved timestamp on final checkout.
CREATE OR REPLACE FUNCTION octofhir_resource_timestamp()
RETURNS TIMESTAMPTZ AS $$
    SELECT date_trunc('milliseconds', COALESCE(
        NULLIF(current_setting('octofhir.write_timestamp', true), '')::timestamptz,
        statement_timestamp()
    ));
$$ LANGUAGE sql STABLE;

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
    NEW.resource = jsonb_set(NEW.resource, '{meta}',
        COALESCE(NEW.resource->'meta', '{}'::jsonb) || jsonb_build_object(
            'versionId', NEW.txid::text,
            'lastUpdated', to_char(NEW.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
        ));
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- One grouped check for INSERT, including resurrection of deleted identities.
-- Internal tables have no history and do not install this trigger.
CREATE OR REPLACE FUNCTION octofhir_guard_insert_revisions()
RETURNS TRIGGER AS $$
DECLARE
    stale BOOLEAN;
    oldest_version BIGINT;
    oldest_timestamp TIMESTAMPTZ;
BEGIN
    SELECT min(txid), min(updated_at) INTO oldest_version, oldest_timestamp FROM new_rows;
    IF oldest_version IS NULL THEN
        RETURN NULL;
    END IF;
    -- These coarse bounds cannot discard a violating revision: its version
    -- or timestamp must also exceed the corresponding minimum of the batch.
    -- BRIN removes old history ranges before the exact identity comparison.
    EXECUTE format(
        'SELECT EXISTS (SELECT 1 FROM %I h JOIN new_rows n ON h.id = n.id
         WHERE (h.txid >= $1 OR h.updated_at > $2)
           AND (h.txid >= n.txid OR h.updated_at > n.updated_at))', TG_TABLE_NAME || '_history')
        INTO stale USING oldest_version, oldest_timestamp;
    IF stale THEN
        RAISE EXCEPTION 'Write metadata precedes a retained resource revision'
            USING ERRCODE = '40001';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
