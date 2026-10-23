-- Retain the previous live body and a separate deletion revision. A transition
-- table keeps UPDATE/DELETE archiving set-based, including native bulk writes.
CREATE OR REPLACE FUNCTION archive_to_history()
RETURNS TRIGGER AS $$
DECLARE
    deletion_version BIGINT;
    deletion_timestamp TIMESTAMPTZ;
BEGIN
    EXECUTE format(
        'INSERT INTO %I (id, txid, created_at, updated_at, resource, status)
         SELECT id, txid, created_at, updated_at, resource, status FROM old_rows
         ON CONFLICT (id, txid) DO NOTHING', TG_TABLE_NAME || '_history');
    IF TG_OP = 'DELETE' AND EXISTS (SELECT 1 FROM old_rows) THEN
        deletion_version := COALESCE(NULLIF(current_setting('octofhir.write_version', true), '')::bigint,
                                     nextval('_transaction_txid_seq'));
        deletion_timestamp := octofhir_resource_timestamp();
        IF EXISTS (SELECT 1 FROM old_rows WHERE txid >= deletion_version OR updated_at > deletion_timestamp) THEN
            RAISE EXCEPTION 'Deletion metadata precedes the locked resource revision'
                USING ERRCODE = '40001';
        END IF;
        -- A tombstone retains the last known attributes for policies and delete
        -- filters, but is never returned as a live/versioned resource body.
        EXECUTE format(
            'INSERT INTO %I (id, txid, created_at, updated_at, resource, status)
             SELECT id, $1, created_at, $2,
                    jsonb_set(resource, ''{meta}'', COALESCE(resource->''meta'', ''{}''::jsonb) ||
                        jsonb_build_object(''versionId'', $1::text, ''lastUpdated'',
                            to_char($2 AT TIME ZONE ''UTC'', ''YYYY-MM-DD"T"HH24:MI:SS.MS"Z"''))),
                    ''deleted''::resource_status FROM old_rows', TG_TABLE_NAME || '_history')
            USING deletion_version, deletion_timestamp;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
