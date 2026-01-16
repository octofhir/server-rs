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
        'cache', 'logging', 'otel', 'storage', 'redis', 'validation', 'fhir', 'packages',
        'db_console'
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
    app_id TEXT,
    app_name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_operations_category ON operations(category);
CREATE INDEX IF NOT EXISTS idx_operations_module ON operations(module);
CREATE INDEX IF NOT EXISTS idx_operations_public ON operations(public);
CREATE INDEX IF NOT EXISTS idx_operations_app_id ON operations(app_id);

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

-- FHIRSchemas table - stores pre-converted FHIRSchemas for on-demand loading
CREATE TABLE IF NOT EXISTS fcm.fhirschemas (
    id SERIAL PRIMARY KEY,
    url TEXT NOT NULL,                    -- Canonical URL of source StructureDefinition
    version TEXT,                         -- StructureDefinition version
    package_name TEXT NOT NULL,           -- Source package
    package_version TEXT NOT NULL,        -- Source package version
    fhir_version TEXT NOT NULL,           -- FHIR version (R4, R4B, R5, R6)
    schema_type TEXT NOT NULL,            -- 'resource' | 'complex-type' | 'extension' | etc.
    content JSONB NOT NULL,               -- The FHIRSchema JSON
    content_hash TEXT NOT NULL,           -- Hash for cache invalidation
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(url, package_name, package_version),
    FOREIGN KEY(package_name, package_version)
        REFERENCES fcm.packages(name, version) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_fcm_fhirschemas_url ON fcm.fhirschemas(url);
CREATE INDEX IF NOT EXISTS idx_fcm_fhirschemas_url_fhir ON fcm.fhirschemas(url, fhir_version);
CREATE INDEX IF NOT EXISTS idx_fcm_fhirschemas_package ON fcm.fhirschemas(package_name, package_version);
CREATE INDEX IF NOT EXISTS idx_fcm_fhirschemas_type ON fcm.fhirschemas(schema_type);
CREATE INDEX IF NOT EXISTS idx_fcm_fhirschemas_content_hash ON fcm.fhirschemas(content_hash);
-- Index for schema lookup by name (extracted from JSONB content)
-- Used by ModelProvider::get_schema() for on-demand schema loading
CREATE INDEX IF NOT EXISTS idx_fcm_fhirschemas_name_fhir ON fcm.fhirschemas((content->>'name'), fhir_version);

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
-- AUTH SCHEMA (OAuth 2.0 / SMART on FHIR)
-- ============================================================================

CREATE SCHEMA IF NOT EXISTS octofhir_auth;

-- SMART launch context storage (EHR launch flow)
CREATE TABLE IF NOT EXISTS octofhir_auth.smart_launch_context (
    launch_id VARCHAR(64) PRIMARY KEY,
    context_data JSONB NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_smart_launch_context_expires
    ON octofhir_auth.smart_launch_context(expires_at);

-- OAuth authorize flow sessions (temporary, tracks login/consent flow before code issuance)
CREATE TABLE IF NOT EXISTS octofhir_auth.authorize_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id TEXT,
    authorization_request JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_authorize_sessions_expires
    ON octofhir_auth.authorize_sessions(expires_at);

-- User consent records (persistent, for skipping consent on repeat authorization)
CREATE TABLE IF NOT EXISTS octofhir_auth.user_consents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id TEXT NOT NULL,
    client_id TEXT NOT NULL,
    scopes TEXT[] NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, client_id)
);

CREATE INDEX IF NOT EXISTS idx_user_consents_user_client
    ON octofhir_auth.user_consents(user_id, client_id);

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
-- FHIR REFERENCE HELPER FUNCTIONS
-- ============================================================================
--
-- These functions parse FHIR references for use in SQL JOINs.
-- They return NULL for non-local references (contained, URN, external),
-- allowing JOINs to naturally filter them out.
--
-- Supported formats:
--   Relative:   "Patient/123"              -> type='Patient', id='123'
--   Versioned:  "Patient/123/_history/1"   -> type='Patient', id='123'
--   Absolute:   "http://localhost:8888/fhir/Patient/123"
--               (only if URL matches octofhir.base_url setting)
--
-- Returns NULL for:
--   Contained:  "#contained-id"
--   URN:        "urn:uuid:xxx", "urn:oid:xxx"
--   External:   URLs not matching local base_url
--   Invalid:    Malformed references
--
-- Usage Example:
--   SELECT o.resource, p.resource
--   FROM observation o
--   JOIN patient p
--     ON p.id = fhir_ref_id(o.resource->'subject'->>'reference')
--   WHERE fhir_ref_type(o.resource->'subject'->>'reference') = 'Patient'
--     AND o.status != 'deleted';
--
-- For absolute URL support, set the base URL:
--   SET octofhir.base_url = 'http://localhost:8888/fhir';
--
-- ============================================================================

-- Extract the resource ID from a FHIR reference string.
-- Returns NULL for non-local/non-resolvable references.
CREATE OR REPLACE FUNCTION fhir_ref_id(reference TEXT)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
PARALLEL SAFE
AS $$
DECLARE
    base_url TEXT;
    path TEXT;
    parts TEXT[];
    type_part TEXT;
    id_part TEXT;
BEGIN
    -- Handle NULL or empty input
    IF reference IS NULL OR reference = '' THEN
        RETURN NULL;
    END IF;

    -- Skip contained references (#id)
    IF reference LIKE '#%' THEN
        RETURN NULL;
    END IF;

    -- Skip URN references (urn:uuid:xxx, urn:oid:xxx)
    IF reference LIKE 'urn:%' THEN
        RETURN NULL;
    END IF;

    -- Determine the path to parse
    IF reference LIKE '%://%' THEN
        -- Absolute URL - check if local
        BEGIN
            base_url := current_setting('octofhir.base_url', true);
        EXCEPTION WHEN OTHERS THEN
            base_url := NULL;
        END;

        IF base_url IS NULL OR base_url = '' THEN
            -- No base URL configured - treat all absolute URLs as external
            RETURN NULL;
        END IF;

        -- Normalize: remove trailing slash from base_url
        base_url := rtrim(base_url, '/');

        -- Check if reference starts with our base URL
        IF NOT (reference LIKE base_url || '/%' OR reference = base_url) THEN
            -- External reference
            RETURN NULL;
        END IF;

        -- Extract path after base URL
        path := substring(reference FROM length(base_url) + 2);
    ELSE
        -- Relative reference
        path := reference;
    END IF;

    -- Remove leading slash if present
    path := ltrim(path, '/');

    -- Split path into parts
    parts := string_to_array(path, '/');

    -- Validate: need at least Type/id
    IF array_length(parts, 1) < 2 THEN
        RETURN NULL;
    END IF;

    type_part := parts[1];
    id_part := parts[2];

    -- Validate resource type (must start with uppercase letter)
    IF type_part IS NULL OR type_part = ''
       OR NOT (substring(type_part FROM 1 FOR 1) ~ '[A-Z]') THEN
        RETURN NULL;
    END IF;

    -- Validate ID (must not be empty, and not be _history)
    IF id_part IS NULL OR id_part = '' OR id_part = '_history' THEN
        RETURN NULL;
    END IF;

    RETURN id_part;
END;
$$;

-- Extract the resource type from a FHIR reference string.
-- Returns NULL for non-local/non-resolvable references.
CREATE OR REPLACE FUNCTION fhir_ref_type(reference TEXT)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
PARALLEL SAFE
AS $$
DECLARE
    base_url TEXT;
    path TEXT;
    parts TEXT[];
    type_part TEXT;
    id_part TEXT;
BEGIN
    -- Handle NULL or empty input
    IF reference IS NULL OR reference = '' THEN
        RETURN NULL;
    END IF;

    -- Skip contained references (#id)
    IF reference LIKE '#%' THEN
        RETURN NULL;
    END IF;

    -- Skip URN references (urn:uuid:xxx, urn:oid:xxx)
    IF reference LIKE 'urn:%' THEN
        RETURN NULL;
    END IF;

    -- Determine the path to parse
    IF reference LIKE '%://%' THEN
        -- Absolute URL - check if local
        BEGIN
            base_url := current_setting('octofhir.base_url', true);
        EXCEPTION WHEN OTHERS THEN
            base_url := NULL;
        END;

        IF base_url IS NULL OR base_url = '' THEN
            -- No base URL configured - treat all absolute URLs as external
            RETURN NULL;
        END IF;

        -- Normalize: remove trailing slash from base_url
        base_url := rtrim(base_url, '/');

        -- Check if reference starts with our base URL
        IF NOT (reference LIKE base_url || '/%' OR reference = base_url) THEN
            -- External reference
            RETURN NULL;
        END IF;

        -- Extract path after base URL
        path := substring(reference FROM length(base_url) + 2);
    ELSE
        -- Relative reference
        path := reference;
    END IF;

    -- Remove leading slash if present
    path := ltrim(path, '/');

    -- Split path into parts
    parts := string_to_array(path, '/');

    -- Validate: need at least Type/id
    IF array_length(parts, 1) < 2 THEN
        RETURN NULL;
    END IF;

    type_part := parts[1];
    id_part := parts[2];

    -- Validate resource type (must start with uppercase letter)
    IF type_part IS NULL OR type_part = ''
       OR NOT (substring(type_part FROM 1 FOR 1) ~ '[A-Z]') THEN
        RETURN NULL;
    END IF;

    -- Validate ID (must not be empty, and not be _history)
    IF id_part IS NULL OR id_part = '' OR id_part = '_history' THEN
        RETURN NULL;
    END IF;

    RETURN type_part;
END;
$$;

-- Parse a FHIR reference into (resource_type, resource_id).
-- More efficient than calling fhir_ref_type and fhir_ref_id separately.
-- Returns (NULL, NULL) for non-local references.
CREATE OR REPLACE FUNCTION fhir_ref_parse(reference TEXT)
RETURNS TABLE(resource_type TEXT, resource_id TEXT)
LANGUAGE plpgsql
IMMUTABLE
PARALLEL SAFE
AS $$
DECLARE
    base_url TEXT;
    path TEXT;
    parts TEXT[];
    type_part TEXT;
    id_part TEXT;
BEGIN
    -- Handle NULL or empty input
    IF reference IS NULL OR reference = '' THEN
        RETURN QUERY SELECT NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    -- Skip contained references (#id)
    IF reference LIKE '#%' THEN
        RETURN QUERY SELECT NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    -- Skip URN references (urn:uuid:xxx, urn:oid:xxx)
    IF reference LIKE 'urn:%' THEN
        RETURN QUERY SELECT NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    -- Determine the path to parse
    IF reference LIKE '%://%' THEN
        -- Absolute URL - check if local
        BEGIN
            base_url := current_setting('octofhir.base_url', true);
        EXCEPTION WHEN OTHERS THEN
            base_url := NULL;
        END;

        IF base_url IS NULL OR base_url = '' THEN
            RETURN QUERY SELECT NULL::TEXT, NULL::TEXT;
            RETURN;
        END IF;

        base_url := rtrim(base_url, '/');

        IF NOT (reference LIKE base_url || '/%' OR reference = base_url) THEN
            RETURN QUERY SELECT NULL::TEXT, NULL::TEXT;
            RETURN;
        END IF;

        path := substring(reference FROM length(base_url) + 2);
    ELSE
        path := reference;
    END IF;

    path := ltrim(path, '/');
    parts := string_to_array(path, '/');

    IF array_length(parts, 1) < 2 THEN
        RETURN QUERY SELECT NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    type_part := parts[1];
    id_part := parts[2];

    IF type_part IS NULL OR type_part = ''
       OR NOT (substring(type_part FROM 1 FOR 1) ~ '[A-Z]') THEN
        RETURN QUERY SELECT NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    IF id_part IS NULL OR id_part = '' OR id_part = '_history' THEN
        RETURN QUERY SELECT NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    RETURN QUERY SELECT type_part, id_part;
END;
$$;

COMMENT ON FUNCTION fhir_ref_id(TEXT) IS
'Extract the resource ID from a FHIR reference string.
Returns NULL for non-local references (contained, URN, external URLs).
Example: fhir_ref_id(''Patient/123'') returns ''123''';

COMMENT ON FUNCTION fhir_ref_type(TEXT) IS
'Extract the resource type from a FHIR reference string.
Returns NULL for non-local references (contained, URN, external URLs).
Example: fhir_ref_type(''Patient/123'') returns ''Patient''';

COMMENT ON FUNCTION fhir_ref_parse(TEXT) IS
'Parse a FHIR reference into (resource_type, resource_id).
More efficient than calling fhir_ref_type and fhir_ref_id separately.
Returns (NULL, NULL) for non-local references.
Example: SELECT * FROM fhir_ref_parse(''Patient/123'')';

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON SCHEMA fcm IS 'FHIR Canonical Manager - stores FHIR packages and resources from Implementation Guides';
COMMENT ON TABLE _transaction IS 'Transaction log for FHIR resource versioning';
COMMENT ON TABLE async_jobs IS 'Tracks asynchronous FHIR operations for Prefer: respond-async pattern';
COMMENT ON TABLE operations IS 'Registry of all server operations for UI display and policy targeting';
COMMENT ON TABLE fcm.packages IS 'Stores metadata for installed FHIR packages (Implementation Guides)';
COMMENT ON TABLE fcm.resources IS 'Stores FHIR conformance resources (StructureDefinition, ValueSet, etc.) with JSONB content';
COMMENT ON TABLE fcm.fhirschemas IS 'Pre-converted FHIRSchemas from StructureDefinitions for on-demand loading';

-- ============================================================================
-- AUTOMATION (JavaScript event-driven workflows)
-- ============================================================================

-- Automation status enum
DO $$ BEGIN
    CREATE TYPE automation_status AS ENUM ('active', 'inactive', 'error');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Automation trigger type enum
DO $$ BEGIN
    CREATE TYPE automation_trigger_type AS ENUM ('resource_event', 'cron', 'manual');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Main automation table
CREATE TABLE IF NOT EXISTS automation (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    description TEXT,
    source_code TEXT NOT NULL,
    compiled_code TEXT,
    status automation_status NOT NULL DEFAULT 'inactive',
    version INT NOT NULL DEFAULT 1,
    timeout_ms INT NOT NULL DEFAULT 5000,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Automation triggers table
CREATE TABLE IF NOT EXISTS automation_trigger (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    automation_id UUID NOT NULL REFERENCES automation(id) ON DELETE CASCADE,
    trigger_type automation_trigger_type NOT NULL,
    resource_type TEXT,
    event_types TEXT[],
    fhirpath_filter TEXT,
    cron_expression TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Constraints
    CONSTRAINT valid_resource_trigger CHECK (
        trigger_type != 'resource_event' OR resource_type IS NOT NULL
    ),
    CONSTRAINT valid_cron_trigger CHECK (
        trigger_type != 'cron' OR cron_expression IS NOT NULL
    )
);

-- Automation execution log table
CREATE TABLE IF NOT EXISTS automation_execution (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    automation_id UUID NOT NULL REFERENCES automation(id) ON DELETE CASCADE,
    trigger_id UUID REFERENCES automation_trigger(id) ON DELETE SET NULL,
    status TEXT NOT NULL,
    input JSONB,
    output JSONB,
    error TEXT,
    logs JSONB,  -- Structured execution logs from execution.log() API
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    duration_ms INT
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_automation_status ON automation(status);
CREATE INDEX IF NOT EXISTS idx_automation_trigger_automation_id ON automation_trigger(automation_id);
CREATE INDEX IF NOT EXISTS idx_automation_trigger_resource ON automation_trigger(resource_type, trigger_type)
    WHERE trigger_type = 'resource_event';
CREATE INDEX IF NOT EXISTS idx_automation_execution_automation_id ON automation_execution(automation_id);
CREATE INDEX IF NOT EXISTS idx_automation_execution_started_at ON automation_execution(started_at DESC);

-- Comments for documentation
COMMENT ON TABLE automation IS 'JavaScript automations for event-driven workflows';
COMMENT ON TABLE automation_trigger IS 'Triggers that activate automations';
COMMENT ON TABLE automation_execution IS 'Log of automation executions';
COMMENT ON COLUMN automation.source_code IS 'Source code (TypeScript or JavaScript)';
COMMENT ON COLUMN automation.compiled_code IS 'Compiled JavaScript (transpiled from TypeScript at deploy time)';
COMMENT ON COLUMN automation.status IS 'Automation status: active (will run), inactive (disabled), error (compilation failed)';
COMMENT ON COLUMN automation.timeout_ms IS 'Maximum execution time in milliseconds';
COMMENT ON COLUMN automation_trigger.resource_type IS 'FHIR resource type for resource_event triggers';
COMMENT ON COLUMN automation_trigger.event_types IS 'Array of event types: created, updated, deleted';
COMMENT ON COLUMN automation_trigger.fhirpath_filter IS 'Optional FHIRPath expression to filter events';
COMMENT ON COLUMN automation_trigger.cron_expression IS 'Cron expression for scheduled triggers';

-- ============================================================================
-- R5 TOPIC-BASED SUBSCRIPTIONS
-- ============================================================================
-- Supports both R5 native subscriptions and R4/R4B via Backport IG
--
-- NOTE: SubscriptionTopic and Subscription resource tables are created automatically
-- when their definitions are loaded from an IG:
-- - R5: hl7.fhir.r5.core (SubscriptionTopic is native)
-- - R4/R4B: hl7.fhir.uv.subscriptions-backport (Backport IG)
--
-- This section only creates operational tables for event queuing and delivery.

-- Events waiting to be delivered to subscriptions
-- Uses SELECT FOR UPDATE SKIP LOCKED for distributed processing
CREATE TABLE IF NOT EXISTS subscription_event (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Subscription reference (FHIR resource ID)
    subscription_id TEXT NOT NULL,

    -- Topic that triggered this event (canonical URL)
    topic_url TEXT NOT NULL,

    -- Event metadata
    event_type TEXT NOT NULL CHECK (event_type IN ('handshake', 'heartbeat', 'event-notification')),
    event_number BIGINT NOT NULL,  -- Monotonic sequence per subscription

    -- Triggering resource change (for event-notification type)
    focus_resource_type TEXT,
    focus_resource_id TEXT,
    focus_event TEXT CHECK (focus_event IS NULL OR focus_event IN ('create', 'update', 'delete')),

    -- Pre-rendered notification bundle (FHIR Bundle with type 'subscription-notification')
    notification_bundle JSONB NOT NULL,

    -- Queue status
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending',     -- Waiting to be processed
        'processing',  -- Currently being delivered
        'delivered',   -- Successfully delivered
        'failed',      -- Permanent failure (max retries exceeded)
        'expired'      -- TTL exceeded
    )),

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ,      -- When processing started
    delivered_at TIMESTAMPTZ,      -- When successfully delivered

    -- Retry handling
    attempts INT NOT NULL DEFAULT 0,
    max_attempts INT NOT NULL DEFAULT 5,
    next_retry_at TIMESTAMPTZ DEFAULT NOW(),  -- When to retry (NULL = immediate)
    last_error TEXT,

    -- Expiration (default 24 hours)
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '24 hours')
);

-- Index for efficient queue polling (most critical for performance)
CREATE INDEX IF NOT EXISTS idx_sub_event_pending
    ON subscription_event(next_retry_at, created_at)
    WHERE status IN ('pending');

-- Index for subscription-specific queries
CREATE INDEX IF NOT EXISTS idx_sub_event_subscription
    ON subscription_event(subscription_id, event_number DESC);

-- Index for cleanup of expired events
CREATE INDEX IF NOT EXISTS idx_sub_event_expires
    ON subscription_event(expires_at)
    WHERE status NOT IN ('delivered', 'failed');

-- Index for status queries
CREATE INDEX IF NOT EXISTS idx_sub_event_status
    ON subscription_event(subscription_id, status);

-- Historical record of delivery attempts for debugging and analytics
CREATE TABLE IF NOT EXISTS subscription_delivery (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id UUID NOT NULL REFERENCES subscription_event(id) ON DELETE CASCADE,
    subscription_id TEXT NOT NULL,

    -- Attempt details
    attempt_number INT NOT NULL,
    channel_type TEXT NOT NULL,  -- 'rest-hook', 'websocket', 'email'

    -- Timing
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    response_time_ms INT,

    -- Result
    success BOOLEAN,
    http_status INT,
    error_code TEXT,
    error_message TEXT,

    -- Debug info (optional, can be disabled for privacy)
    request_url TEXT,
    response_body_preview TEXT  -- First 1000 chars of response for debugging
);

CREATE INDEX IF NOT EXISTS idx_sub_delivery_event
    ON subscription_delivery(event_id);

CREATE INDEX IF NOT EXISTS idx_sub_delivery_subscription
    ON subscription_delivery(subscription_id, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_sub_delivery_failed
    ON subscription_delivery(subscription_id, success)
    WHERE success = FALSE;

-- Track active WebSocket connections for subscription delivery
-- Required for multi-instance deployments to route events correctly
CREATE TABLE IF NOT EXISTS subscription_websocket (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscription_id TEXT NOT NULL,

    -- Server instance that holds the connection
    server_instance TEXT NOT NULL,

    -- Connection state
    connected_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Unique constraint: one connection per subscription per server
    UNIQUE(subscription_id, server_instance)
);

CREATE INDEX IF NOT EXISTS idx_sub_ws_subscription
    ON subscription_websocket(subscription_id);

-- Index for stale connection cleanup
CREATE INDEX IF NOT EXISTS idx_sub_ws_heartbeat
    ON subscription_websocket(last_heartbeat_at);

-- Cached subscription status for efficient $status operation
-- Updated by delivery processor, avoids expensive aggregate queries
CREATE TABLE IF NOT EXISTS subscription_status (
    subscription_id TEXT PRIMARY KEY,

    -- Counters
    events_since_subscription_start BIGINT NOT NULL DEFAULT 0,
    events_in_notification BIGINT NOT NULL DEFAULT 0,  -- Current batch

    -- Error tracking
    error_count INT NOT NULL DEFAULT 0,
    last_error_at TIMESTAMPTZ,
    last_error_message TEXT,

    -- Delivery tracking
    last_delivery_at TIMESTAMPTZ,
    last_event_number BIGINT NOT NULL DEFAULT 0,

    -- Topic reference
    topic_url TEXT,

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Get next event number for a subscription (thread-safe)
CREATE OR REPLACE FUNCTION next_subscription_event_number(p_subscription_id TEXT)
RETURNS BIGINT AS $$
DECLARE
    v_next BIGINT;
BEGIN
    -- Use subscription_status for efficient lookup
    INSERT INTO subscription_status (subscription_id, last_event_number)
    VALUES (p_subscription_id, 1)
    ON CONFLICT (subscription_id) DO UPDATE
        SET last_event_number = subscription_status.last_event_number + 1,
            updated_at = NOW()
    RETURNING last_event_number INTO v_next;

    RETURN v_next;
END;
$$ LANGUAGE plpgsql;

-- Claim pending events for processing (distributed lock pattern)
-- Returns events that are ready to be processed
CREATE OR REPLACE FUNCTION claim_subscription_events(
    p_limit INT DEFAULT 100,
    p_processor_id TEXT DEFAULT NULL
)
RETURNS SETOF subscription_event AS $$
BEGIN
    RETURN QUERY
    WITH claimed AS (
        SELECT id
        FROM subscription_event
        WHERE status = 'pending'
          AND next_retry_at <= NOW()
          AND expires_at > NOW()
        ORDER BY next_retry_at ASC, created_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT p_limit
    )
    UPDATE subscription_event e
    SET status = 'processing',
        processed_at = NOW()
    FROM claimed c
    WHERE e.id = c.id
    RETURNING e.*;
END;
$$ LANGUAGE plpgsql;

-- Mark event as delivered
CREATE OR REPLACE FUNCTION mark_event_delivered(p_event_id UUID)
RETURNS VOID AS $$
BEGIN
    UPDATE subscription_event
    SET status = 'delivered',
        delivered_at = NOW()
    WHERE id = p_event_id;

    -- Update subscription status
    UPDATE subscription_status ss
    SET events_since_subscription_start = events_since_subscription_start + 1,
        last_delivery_at = NOW(),
        updated_at = NOW()
    FROM subscription_event se
    WHERE se.id = p_event_id
      AND ss.subscription_id = se.subscription_id;
END;
$$ LANGUAGE plpgsql;

-- Mark event for retry with exponential backoff
CREATE OR REPLACE FUNCTION mark_event_retry(
    p_event_id UUID,
    p_error TEXT DEFAULT NULL
)
RETURNS VOID AS $$
DECLARE
    v_attempts INT;
    v_max_attempts INT;
    v_delay_seconds INT;
BEGIN
    SELECT attempts, max_attempts INTO v_attempts, v_max_attempts
    FROM subscription_event
    WHERE id = p_event_id;

    v_attempts := v_attempts + 1;

    IF v_attempts >= v_max_attempts THEN
        -- Max retries exceeded, mark as failed
        UPDATE subscription_event
        SET status = 'failed',
            attempts = v_attempts,
            last_error = p_error
        WHERE id = p_event_id;

        -- Update subscription status error count
        UPDATE subscription_status ss
        SET error_count = error_count + 1,
            last_error_at = NOW(),
            last_error_message = p_error,
            updated_at = NOW()
        FROM subscription_event se
        WHERE se.id = p_event_id
          AND ss.subscription_id = se.subscription_id;
    ELSE
        -- Calculate exponential backoff: 60, 120, 240, 480, 960 seconds
        v_delay_seconds := 60 * (1 << (v_attempts - 1));

        UPDATE subscription_event
        SET status = 'pending',
            attempts = v_attempts,
            next_retry_at = NOW() + (v_delay_seconds || ' seconds')::INTERVAL,
            last_error = p_error
        WHERE id = p_event_id;
    END IF;
END;
$$ LANGUAGE plpgsql;

-- Cleanup expired and old delivered events
CREATE OR REPLACE FUNCTION cleanup_subscription_events(
    p_delivered_retention_hours INT DEFAULT 24,
    p_failed_retention_hours INT DEFAULT 168  -- 7 days
)
RETURNS INT AS $$
DECLARE
    v_deleted INT;
BEGIN
    WITH deleted AS (
        DELETE FROM subscription_event
        WHERE (status = 'expired')
           OR (status = 'delivered' AND delivered_at < NOW() - (p_delivered_retention_hours || ' hours')::INTERVAL)
           OR (status = 'failed' AND created_at < NOW() - (p_failed_retention_hours || ' hours')::INTERVAL)
           OR (expires_at < NOW() AND status NOT IN ('delivered', 'failed'))
        RETURNING id
    )
    SELECT COUNT(*) INTO v_deleted FROM deleted;

    RETURN v_deleted;
END;
$$ LANGUAGE plpgsql;

-- Cleanup stale WebSocket connections (no heartbeat for 5 minutes)
CREATE OR REPLACE FUNCTION cleanup_stale_websocket_connections()
RETURNS INT AS $$
DECLARE
    v_deleted INT;
BEGIN
    WITH deleted AS (
        DELETE FROM subscription_websocket
        WHERE last_heartbeat_at < NOW() - INTERVAL '5 minutes'
        RETURNING id
    )
    SELECT COUNT(*) INTO v_deleted FROM deleted;

    RETURN v_deleted;
END;
$$ LANGUAGE plpgsql;

COMMENT ON TABLE subscription_event IS 'Queue of subscription notification events pending delivery';
COMMENT ON TABLE subscription_delivery IS 'Historical record of delivery attempts for debugging';
COMMENT ON TABLE subscription_websocket IS 'Active WebSocket connections for subscription delivery';
COMMENT ON TABLE subscription_status IS 'Cached subscription status for efficient $status operation';
COMMENT ON FUNCTION claim_subscription_events IS 'Atomically claim pending events for processing using SKIP LOCKED';
COMMENT ON FUNCTION mark_event_delivered IS 'Mark event as successfully delivered and update status';
COMMENT ON FUNCTION mark_event_retry IS 'Schedule event for retry with exponential backoff';

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
