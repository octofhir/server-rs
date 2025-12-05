-- Configuration Management Schema
-- Stores runtime configuration with versioning and change tracking

-- Configuration table
CREATE TABLE IF NOT EXISTS octofhir.configuration (
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
CREATE TABLE IF NOT EXISTS octofhir.configuration_history (
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
CREATE INDEX IF NOT EXISTS idx_configuration_key ON octofhir.configuration(key);
CREATE INDEX IF NOT EXISTS idx_configuration_category ON octofhir.configuration(category);
CREATE INDEX IF NOT EXISTS idx_configuration_ts ON octofhir.configuration(ts);
CREATE INDEX IF NOT EXISTS idx_configuration_history_key ON octofhir.configuration_history(key);
CREATE INDEX IF NOT EXISTS idx_configuration_history_ts ON octofhir.configuration_history(ts);

-- Archive to history function
CREATE OR REPLACE FUNCTION octofhir.archive_config_to_history()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO octofhir.configuration_history (id, key, category, value, description, is_secret, txid, ts, updated_by)
    VALUES (OLD.id, OLD.key, OLD.category, OLD.value, OLD.description, OLD.is_secret, OLD.txid, OLD.ts, OLD.updated_by);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- History trigger
DROP TRIGGER IF EXISTS configuration_history_trigger ON octofhir.configuration;
CREATE TRIGGER configuration_history_trigger
    BEFORE UPDATE OR DELETE ON octofhir.configuration
    FOR EACH ROW EXECUTE FUNCTION octofhir.archive_config_to_history();

-- NOTIFY trigger for hot-reload
CREATE OR REPLACE FUNCTION octofhir.notify_config_change()
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

DROP TRIGGER IF EXISTS config_notify_trigger ON octofhir.configuration;
CREATE TRIGGER config_notify_trigger
    AFTER INSERT OR UPDATE OR DELETE ON octofhir.configuration
    FOR EACH ROW EXECUTE FUNCTION octofhir.notify_config_change();

-- Insert default feature flags
INSERT INTO octofhir.configuration (key, category, value, description, is_secret, txid)
SELECT
    'search.optimization.enabled',
    'features',
    'true'::jsonb,
    'Enable query optimization in search engine',
    false,
    (SELECT COALESCE(MAX(txid), 0) + 1 FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM octofhir.configuration WHERE key = 'search.optimization.enabled'
);

INSERT INTO octofhir.configuration (key, category, value, description, is_secret, txid)
SELECT
    'terminology.external.enabled',
    'features',
    'true'::jsonb,
    'Allow external terminology server lookups',
    false,
    (SELECT COALESCE(MAX(txid), 0) + 1 FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM octofhir.configuration WHERE key = 'terminology.external.enabled'
);

INSERT INTO octofhir.configuration (key, category, value, description, is_secret, txid)
SELECT
    'validation.skip.allowed',
    'features',
    'false'::jsonb,
    'Allow X-Skip-Validation header',
    false,
    (SELECT COALESCE(MAX(txid), 0) + 1 FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM octofhir.configuration WHERE key = 'validation.skip.allowed'
);

INSERT INTO octofhir.configuration (key, category, value, description, is_secret, txid)
SELECT
    'auth.smart_on_fhir.enabled',
    'features',
    'true'::jsonb,
    'Enable SMART on FHIR authentication',
    false,
    (SELECT COALESCE(MAX(txid), 0) + 1 FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM octofhir.configuration WHERE key = 'auth.smart_on_fhir.enabled'
);

INSERT INTO octofhir.configuration (key, category, value, description, is_secret, txid)
SELECT
    'cache.redis.enabled',
    'features',
    'false'::jsonb,
    'Use Redis as cache backend',
    false,
    (SELECT COALESCE(MAX(txid), 0) + 1 FROM _transaction)
WHERE NOT EXISTS (
    SELECT 1 FROM octofhir.configuration WHERE key = 'cache.redis.enabled'
);
