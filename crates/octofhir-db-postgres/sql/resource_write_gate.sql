-- Shared by ordinary resource mutations, exclusive for conditional resolution.
-- Two fixed advisory keys keep the namespace independent of query text or types.
CREATE OR REPLACE FUNCTION octofhir_guard_resource_write()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_advisory_xact_lock_shared(1330006098, 1);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
