-- FHIR Canonical Manager (FCM) Schema
-- This schema stores FHIR package resources from Implementation Guides
-- managed by the canonical manager. Resources are stored with JSONB content
-- and enhanced search fields for fast querying.

-- Create the fcm schema for canonical manager package data
CREATE SCHEMA IF NOT EXISTS fcm;

-- ============================================================================
-- METADATA TABLE
-- ============================================================================

-- Metadata table for schema versioning and configuration
CREATE TABLE IF NOT EXISTS fcm.metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert schema version
INSERT INTO fcm.metadata (key, value) VALUES ('schema_version', '1.0.0')
ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW();

-- ============================================================================
-- PACKAGES TABLE
-- ============================================================================

-- Packages table - stores FHIR package metadata
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

-- ============================================================================
-- RESOURCES TABLE
-- ============================================================================

-- Resources table with JSONB content and enhanced search fields
CREATE TABLE IF NOT EXISTS fcm.resources (
    id SERIAL PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    url TEXT,
    name TEXT,
    version TEXT,

    -- StructureDefinition specific fields
    sd_kind TEXT,
    sd_derivation TEXT,
    sd_type TEXT,
    sd_base_definition TEXT,
    sd_abstract BOOLEAN,
    sd_impose_profiles JSONB,
    sd_characteristics JSONB,
    sd_flavor TEXT,

    -- Package reference
    package_name TEXT NOT NULL,
    package_version TEXT NOT NULL,
    fhir_version TEXT NOT NULL,
    content_hash TEXT NOT NULL,

    -- JSONB content storage (replaces CAS file storage)
    content JSONB NOT NULL,

    -- Enhanced search fields (extracted from content for fast queries)
    id_lower TEXT GENERATED ALWAYS AS (lower(resource_id)) STORED,
    name_lower TEXT GENERATED ALWAYS AS (lower(name)) STORED,
    url_lower TEXT GENERATED ALWAYS AS (lower(url)) STORED,
    title TEXT,  -- extracted from content.title
    description TEXT,  -- extracted from content.description
    status TEXT,  -- extracted from content.status (active, draft, retired)
    publisher TEXT,  -- extracted from content.publisher

    -- Full-text search vector
    search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(name, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(title, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(description, '')), 'C')
    ) STORED,

    FOREIGN KEY(package_name, package_version)
        REFERENCES fcm.packages(name, version) ON DELETE CASCADE
);

-- ============================================================================
-- INDEXES
-- ============================================================================

-- Package indexes
CREATE INDEX IF NOT EXISTS idx_fcm_package_name_version ON fcm.packages(name, version);
CREATE INDEX IF NOT EXISTS idx_fcm_package_priority ON fcm.packages(priority DESC);
CREATE INDEX IF NOT EXISTS idx_fcm_package_fhir_version ON fcm.packages(fhir_version);

-- Resource lookup indexes
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

-- GIN indexes for JSONB and full-text search
CREATE INDEX IF NOT EXISTS idx_fcm_resource_content ON fcm.resources USING GIN (content jsonb_path_ops);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_search ON fcm.resources USING GIN (search_vector);

-- Composite indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_fcm_resource_type_fhir_flavor ON fcm.resources(resource_type, fhir_version, sd_flavor);
CREATE INDEX IF NOT EXISTS idx_fcm_resource_priority_lookup ON fcm.resources(url, package_name, package_version)
    INCLUDE (resource_type, sd_flavor);

-- Index for base URL pattern matching (for version fallback queries)
CREATE INDEX IF NOT EXISTS idx_fcm_resource_url_pattern ON fcm.resources(url text_pattern_ops);

-- ============================================================================
-- NOTIFICATION TRIGGERS FOR HOT-RELOAD
-- ============================================================================

-- Notification function for package changes
CREATE OR REPLACE FUNCTION fcm.notify_package_change()
RETURNS TRIGGER AS $$
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
$$ LANGUAGE plpgsql;

-- Trigger for package changes
DROP TRIGGER IF EXISTS fcm_packages_notify ON fcm.packages;
CREATE TRIGGER fcm_packages_notify
    AFTER INSERT OR UPDATE OR DELETE ON fcm.packages
    FOR EACH ROW EXECUTE FUNCTION fcm.notify_package_change();

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Function to extract search fields from content when inserting/updating
CREATE OR REPLACE FUNCTION fcm.extract_search_fields()
RETURNS TRIGGER AS $$
BEGIN
    NEW.title := NEW.content->>'title';
    NEW.description := NEW.content->>'description';
    NEW.status := NEW.content->>'status';
    NEW.publisher := NEW.content->>'publisher';
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to automatically extract search fields
DROP TRIGGER IF EXISTS fcm_resources_extract_fields ON fcm.resources;
CREATE TRIGGER fcm_resources_extract_fields
    BEFORE INSERT OR UPDATE ON fcm.resources
    FOR EACH ROW EXECUTE FUNCTION fcm.extract_search_fields();

-- Function to update package resource count
CREATE OR REPLACE FUNCTION fcm.update_package_resource_count()
RETURNS TRIGGER AS $$
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
$$ LANGUAGE plpgsql;

-- Trigger to maintain resource count
DROP TRIGGER IF EXISTS fcm_resources_count ON fcm.resources;
CREATE TRIGGER fcm_resources_count
    AFTER INSERT OR DELETE ON fcm.resources
    FOR EACH ROW EXECUTE FUNCTION fcm.update_package_resource_count();

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON SCHEMA fcm IS 'FHIR Canonical Manager - stores FHIR packages and resources from Implementation Guides';
COMMENT ON TABLE fcm.packages IS 'Stores metadata for installed FHIR packages (Implementation Guides)';
COMMENT ON TABLE fcm.resources IS 'Stores FHIR conformance resources (StructureDefinition, ValueSet, etc.) with JSONB content';
COMMENT ON COLUMN fcm.resources.content IS 'Full FHIR resource content stored as JSONB';
COMMENT ON COLUMN fcm.resources.search_vector IS 'Full-text search vector for efficient text searches';
COMMENT ON COLUMN fcm.resources.sd_flavor IS 'StructureDefinition flavor: resource, complex-type, primitive-type, extension, profile, logical';
