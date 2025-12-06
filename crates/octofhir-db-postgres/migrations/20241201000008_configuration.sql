-- Configuration Management Schema
-- Stores runtime configuration with versioning and change tracking
-- Uses public schema with underscore prefix for system tables

-- Configuration table
CREATE TABLE IF NOT EXISTS _configuration (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    value JSONB NOT NULL,
    description TEXT,
    is_secret BOOLEAN DEFAULT FALSE,
    txid BIGINT NOT NULL REFERENCES _transaction(txid),
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by TEXT,
    CONSTRAINT valid_category CHECK (category IN (
        'server', 'search', 'auth', 'terminology', 'features',
        'cache', 'logging', 'otel', 'storage', 'redis', 'validation', 'fhir', 'packages'
    ))
);

-- Configuration history table for audit trail
CREATE TABLE IF NOT EXISTS _configuration_history (
    id UUID NOT NULL,
    key TEXT NOT NULL,
    category TEXT NOT NULL,
    value JSONB NOT NULL,
    description TEXT,
    is_secret BOOLEAN,
    txid BIGINT NOT NULL,
    ts TIMESTAMPTZ NOT NULL,
    updated_by TEXT,
    PRIMARY KEY (id, txid)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_configuration_key ON _configuration(key);
CREATE INDEX IF NOT EXISTS idx_configuration_category ON _configuration(category);
CREATE INDEX IF NOT EXISTS idx_configuration_ts ON _configuration(ts);
CREATE INDEX IF NOT EXISTS idx_configuration_history_key ON _configuration_history(key);
CREATE INDEX IF NOT EXISTS idx_configuration_history_ts ON _configuration_history(ts);

-- Archive to history function
CREATE OR REPLACE FUNCTION archive_config_to_history()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO _configuration_history (id, key, category, value, description, is_secret, txid, ts, updated_by)
    VALUES (OLD.id, OLD.key, OLD.category, OLD.value, OLD.description, OLD.is_secret, OLD.txid, OLD.ts, OLD.updated_by);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- History trigger
DROP TRIGGER IF EXISTS configuration_history_trigger ON _configuration;
CREATE TRIGGER configuration_history_trigger
    BEFORE UPDATE OR DELETE ON _configuration
    FOR EACH ROW EXECUTE FUNCTION archive_config_to_history();

-- NOTIFY trigger for hot-reload
CREATE OR REPLACE FUNCTION notify_config_change()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('octofhir_config_changes', json_build_object(
        'key', COALESCE(NEW.key, OLD.key),
        'category', COALESCE(NEW.category, OLD.category),
        'operation', TG_OP
    )::text);
    RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS config_notify_trigger ON _configuration;
CREATE TRIGGER config_notify_trigger
    AFTER INSERT OR UPDATE OR DELETE ON _configuration
    FOR EACH ROW EXECUTE FUNCTION notify_config_change();

-- Ensure we have a transaction record for the default config values
INSERT INTO _transaction (status) VALUES ('committed');

-- Insert default feature flags using the newly created transaction
INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT
    'search.optimization.enabled',
    'features',
    'true'::jsonb,
    'Enable query optimization in search engine',
    false,
    (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM _configuration WHERE key = 'search.optimization.enabled'
);

INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT
    'terminology.external.enabled',
    'features',
    'true'::jsonb,
    'Allow external terminology server lookups',
    false,
    (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM _configuration WHERE key = 'terminology.external.enabled'
);

INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT
    'validation.skip.allowed',
    'features',
    'false'::jsonb,
    'Allow X-Skip-Validation header',
    false,
    (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM _configuration WHERE key = 'validation.skip.allowed'
);

INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT
    'auth.smart_on_fhir.enabled',
    'features',
    'true'::jsonb,
    'Enable SMART on FHIR authentication',
    false,
    (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM _configuration WHERE key = 'auth.smart_on_fhir.enabled'
);

INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT
    'cache.redis.enabled',
    'features',
    'false'::jsonb,
    'Use Redis as cache backend',
    false,
    (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM _configuration WHERE key = 'cache.redis.enabled'
);
