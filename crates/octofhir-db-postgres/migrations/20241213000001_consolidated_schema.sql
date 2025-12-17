-- OctoFHIR Consolidated Database Schema
-- Single migration with TEXT-based IDs (FHIR-compliant)
-- Resource IDs are TEXT to support both UUIDs and custom string IDs

-- ============================================================================
-- EXTENSIONS
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ============================================================================
-- BASE TABLES
-- ============================================================================

-- Transaction log for atomicity
CREATE TABLE IF NOT EXISTS _transaction (
    txid BIGSERIAL PRIMARY KEY,
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status VARCHAR(16) NOT NULL DEFAULT 'committed'
);

CREATE INDEX IF NOT EXISTS idx_transaction_ts ON _transaction(ts);

-- Resource status enum
DO $$ BEGIN
    CREATE TYPE resource_status AS ENUM ('created', 'updated', 'deleted');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- ============================================================================
-- CONFIGURATION MANAGEMENT
-- ============================================================================

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

-- ============================================================================
-- ASYNC JOBS INFRASTRUCTURE
-- ============================================================================

CREATE TABLE IF NOT EXISTS async_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    status VARCHAR(20) NOT NULL DEFAULT 'queued',
    request_type VARCHAR(50) NOT NULL,
    request_method VARCHAR(10) NOT NULL,
    request_url TEXT NOT NULL,
    request_body JSONB,
    request_headers JSONB,
    result JSONB,
    progress FLOAT DEFAULT 0,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    client_id VARCHAR(255),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '24 hours'),
    CONSTRAINT valid_status CHECK (status IN ('queued', 'in_progress', 'completed', 'failed', 'cancelled')),
    CONSTRAINT valid_progress CHECK (progress >= 0 AND progress <= 1),
    CONSTRAINT valid_method CHECK (request_method IN ('GET', 'POST', 'PUT', 'PATCH', 'DELETE'))
);

CREATE INDEX IF NOT EXISTS idx_async_jobs_status ON async_jobs(status) WHERE status IN ('queued', 'in_progress');
CREATE INDEX IF NOT EXISTS idx_async_jobs_client ON async_jobs(client_id, created_at DESC) WHERE client_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_async_jobs_expires ON async_jobs(expires_at) WHERE status IN ('queued', 'in_progress', 'completed', 'failed');
CREATE INDEX IF NOT EXISTS idx_async_jobs_created ON async_jobs(created_at DESC);

CREATE OR REPLACE FUNCTION update_async_jobs_updated_at()
RETURNS TRIGGER AS $func$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$func$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trigger_async_jobs_updated_at ON async_jobs;
CREATE TRIGGER trigger_async_jobs_updated_at
    BEFORE UPDATE ON async_jobs
    FOR EACH ROW
    EXECUTE FUNCTION update_async_jobs_updated_at();

CREATE OR REPLACE VIEW active_async_jobs AS
SELECT
    id,
    status,
    request_type,
    request_method,
    request_url,
    progress,
    created_at,
    updated_at,
    client_id,
    EXTRACT(EPOCH FROM (NOW() - created_at)) as age_seconds
FROM async_jobs
WHERE status IN ('queued', 'in_progress')
ORDER BY created_at ASC;

-- ============================================================================
-- OPERATION REGISTRY
-- ============================================================================

CREATE TABLE IF NOT EXISTS operations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    category TEXT NOT NULL,
    methods JSONB NOT NULL DEFAULT '[]'::jsonb,
    path_pattern TEXT NOT NULL,
    public BOOLEAN NOT NULL DEFAULT false,
    module TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_operations_category ON operations(category);
CREATE INDEX IF NOT EXISTS idx_operations_module ON operations(module);
CREATE INDEX IF NOT EXISTS idx_operations_public ON operations(public);

CREATE OR REPLACE FUNCTION update_operations_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS tr_operations_updated_at ON operations;
CREATE TRIGGER tr_operations_updated_at
    BEFORE UPDATE ON operations
    FOR EACH ROW
    EXECUTE FUNCTION update_operations_updated_at();

-- ============================================================================
-- FCM (FHIR CANONICAL MANAGER) SCHEMA
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS fcm;

-- Metadata table
CREATE TABLE IF NOT EXISTS fcm.metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO fcm.metadata (key, value) VALUES ('schema_version', '1.0.0')
ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW();

-- Packages table
CREATE TABLE IF NOT EXISTS fcm.packages (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    package_path TEXT,
    fhir_version TEXT,
    manifest_hash TEXT NOT NULL,
    installed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resource_count INTEGER NOT NULL DEFAULT 0,
    priority INTEGER DEFAULT 0,
    UNIQUE(name, version)
);

-- Resources table
CREATE TABLE IF NOT EXISTS fcm.resources (
    id SERIAL PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    url TEXT,
    name TEXT,
    version TEXT,
    sd_kind TEXT,
    sd_derivation TEXT,
    sd_type TEXT,
    sd_base_definition TEXT,
    sd_abstract BOOLEAN,
    sd_impose_profiles JSONB,
    sd_characteristics JSONB,
    sd_flavor TEXT,
    package_name TEXT NOT NULL,
    package_version TEXT NOT NULL,
    fhir_version TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    content JSONB NOT NULL,
    id_lower TEXT,
    name_lower TEXT,
    url_lower TEXT,
    title TEXT,
    description TEXT,
    status TEXT,
    publisher TEXT,
    search_vector tsvector,
    FOREIGN KEY(package_name, package_version)
        REFERENCES fcm.packages(name, version) ON DELETE CASCADE
);

-- FCM Indexes
CREATE INDEX IF NOT EXISTS idx_fcm_package_name_version ON fcm.packages(name, version);
CREATE INDEX IF NOT EXISTS idx_fcm_package_priority ON fcm.packages(priority DESC);
CREATE INDEX IF NOT EXISTS idx_fcm_package_fhir_version ON fcm.packages(fhir_version);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_url ON fcm.resources(url);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_url_lower ON fcm.resources(url_lower);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_id_type ON fcm.resources(id_lower, resource_type);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_name_type ON fcm.resources(name_lower, resource_type);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_type ON fcm.resources(resource_type);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_type_flavor ON fcm.resources(resource_type, sd_flavor);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_sd_flavor ON fcm.resources(sd_flavor) WHERE sd_flavor IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_fcm_resource_package ON fcm.resources(package_name, package_version);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_fhir_version ON fcm.resources(fhir_version);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_status ON fcm.resources(status);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_content_hash ON fcm.resources(content_hash);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_content ON fcm.resources USING GIN (content jsonb_path_ops);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_search ON fcm.resources USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_type_fhir_flavor ON fcm.resources(resource_type, fhir_version, sd_flavor);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_priority_lookup ON fcm.resources(url, package_name, package_version)
    INCLUDE (resource_type, sd_flavor);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_url_pattern ON fcm.resources(url text_pattern_ops);

-- FCM Triggers
CREATE OR REPLACE FUNCTION fcm.extract_search_fields()
RETURNS TRIGGER AS $func$
BEGIN
    NEW.id_lower := lower(NEW.resource_id);
    NEW.name_lower := lower(NEW.name);
    NEW.url_lower := lower(NEW.url);
    NEW.title := NEW.content->>'title';
    NEW.description := NEW.content->>'description';
    NEW.status := NEW.content->>'status';
    NEW.publisher := NEW.content->>'publisher';
    NEW.search_vector :=
        setweight(to_tsvector('english', coalesce(NEW.name, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.title, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(NEW.description, '')), 'C');
    RETURN NEW;
END;
$func$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS fcm_resources_extract_fields ON fcm.resources;
CREATE TRIGGER fcm_resources_extract_fields
    BEFORE INSERT OR UPDATE ON fcm.resources
    FOR EACH ROW EXECUTE FUNCTION fcm.extract_search_fields();

CREATE OR REPLACE FUNCTION fcm.notify_package_change()
RETURNS TRIGGER AS $func$
BEGIN
    PERFORM pg_notify('fcm_package_changes',
        json_build_object(
            'operation', TG_OP,
            'package_name', COALESCE(NEW.name, OLD.name),
            'package_version', COALESCE(NEW.version, OLD.version)
        )::text
    );
    RETURN COALESCE(NEW, OLD);
END;
$func$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS fcm_packages_notify ON fcm.packages;
CREATE TRIGGER fcm_packages_notify
    AFTER INSERT OR UPDATE OR DELETE ON fcm.packages
    FOR EACH ROW EXECUTE FUNCTION fcm.notify_package_change();

CREATE OR REPLACE FUNCTION fcm.update_package_resource_count()
RETURNS TRIGGER AS $func$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE fcm.packages
        SET resource_count = resource_count + 1
        WHERE name = NEW.package_name AND version = NEW.package_version;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE fcm.packages
        SET resource_count = resource_count - 1
        WHERE name = OLD.package_name AND version = OLD.package_version;
    END IF;
    RETURN NULL;
END;
$func$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS fcm_resources_count ON fcm.resources;
CREATE TRIGGER fcm_resources_count
    AFTER INSERT OR DELETE ON fcm.resources
    FOR EACH ROW EXECUTE FUNCTION fcm.update_package_resource_count();

-- ============================================================================
-- TEMPORARY VALUESET TABLES
-- ============================================================================

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

-- ============================================================================
-- GATEWAY NOTIFICATION FUNCTIONS
-- ============================================================================

-- NOTE: Resource tables are created dynamically by SchemaManager
-- This migration only creates the notification functions

CREATE OR REPLACE FUNCTION notify_gateway_resource_change()
RETURNS TRIGGER AS $func$
DECLARE
    resource_json JSONB;
    notification_payload JSON;
BEGIN
    IF TG_OP = 'DELETE' THEN
        resource_json := OLD.resource;
    ELSE
        resource_json := NEW.resource;
    END IF;

    notification_payload := json_build_object(
        'table', TG_TABLE_NAME,
        'resource_type', resource_json->>'resourceType',
        'operation', TG_OP,
        'id', COALESCE(NEW.id, OLD.id)::TEXT
    );

    PERFORM pg_notify('octofhir_gateway_changes', notification_payload::text);

    RETURN COALESCE(NEW, OLD);
END;
$func$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION notify_policy_change()
RETURNS TRIGGER AS $func$
DECLARE
    notification_payload JSON;
    resource_id TEXT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        resource_id := OLD.id::TEXT;
    ELSE
        resource_id := NEW.id::TEXT;
    END IF;

    notification_payload := json_build_object(
        'operation', TG_OP,
        'id', resource_id
    );

    PERFORM pg_notify('octofhir_policy_changes', notification_payload::text);

    RETURN COALESCE(NEW, OLD);
END;
$func$ LANGUAGE plpgsql;

-- ============================================================================
-- DEFAULT CONFIGURATION
-- ============================================================================

-- Insert a transaction for default config
INSERT INTO _transaction (status) VALUES ('committed');

-- Default feature flags
INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT 'search.optimization.enabled', 'features', 'true'::jsonb, 'Enable query optimization in search engine', false, (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (SELECT 1 FROM _configuration WHERE key = 'search.optimization.enabled');

INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT 'terminology.external.enabled', 'features', 'true'::jsonb, 'Allow external terminology server lookups', false, (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (SELECT 1 FROM _configuration WHERE key = 'terminology.external.enabled');

INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT 'validation.skip.allowed', 'features', 'false'::jsonb, 'Allow X-Skip-Validation header', false, (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (SELECT 1 FROM _configuration WHERE key = 'validation.skip.allowed');

INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT 'auth.smart_on_fhir.enabled', 'features', 'true'::jsonb, 'Enable SMART on FHIR authentication', false, (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (SELECT 1 FROM _configuration WHERE key = 'auth.smart_on_fhir.enabled');

INSERT INTO _configuration (key, category, value, description, is_secret, txid)
SELECT 'cache.redis.enabled', 'features', 'false'::jsonb, 'Use Redis as cache backend', false, (SELECT MAX(txid) FROM _transaction)
WHERE NOT EXISTS (SELECT 1 FROM _configuration WHERE key = 'cache.redis.enabled');

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON SCHEMA fcm IS 'FHIR Canonical Manager - stores FHIR packages and resources from Implementation Guides';
COMMENT ON TABLE _transaction IS 'Transaction log for FHIR resource versioning';
COMMENT ON TABLE async_jobs IS 'Tracks asynchronous FHIR operations for Prefer: respond-async pattern';
COMMENT ON TABLE operations IS 'Registry of all server operations for UI display and policy targeting';
COMMENT ON TABLE fcm.packages IS 'Stores metadata for installed FHIR packages (Implementation Guides)';
COMMENT ON TABLE fcm.resources IS 'Stores FHIR conformance resources (StructureDefinition, ValueSet, etc.) with JSONB content';

-- ============================================================================
-- SCHEMA NOTES
-- ============================================================================
--
-- FHIR Resource Tables (Patient, Observation, etc.) are created dynamically
-- by SchemaManager when resources are first accessed. Each resource table has:
--
-- CREATE TABLE "{ResourceType}" (
--     id TEXT PRIMARY KEY,              -- FHIR-compliant string ID (UUID or custom)
--     txid BIGINT NOT NULL REFERENCES _transaction(txid),
--     ts TIMESTAMPTZ NOT NULL,
--     resource JSONB NOT NULL,
--     status resource_status NOT NULL DEFAULT 'created'
-- );
--
-- CREATE TABLE "{ResourceType}_history" (
--     id TEXT NOT NULL,
--     txid BIGINT NOT NULL,
--     ts TIMESTAMPTZ NOT NULL,
--     resource JSONB NOT NULL,
--     status resource_status NOT NULL,
--     PRIMARY KEY (id, txid)
-- );
--
-- See SchemaManager for full implementation.

-- ============================================================================
-- SHARED TRIGGER FUNCTIONS FOR RESOURCE TABLES
-- ============================================================================

-- Shared function to update updated_at timestamp on resource updates
-- Used by all dynamically created resource tables
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
