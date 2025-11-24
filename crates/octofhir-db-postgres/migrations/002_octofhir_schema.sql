-- OctoFHIR internal conformance resources schema
-- This schema stores internal Implementation Guide resources:
-- StructureDefinitions, ValueSets, CodeSystems, SearchParameters
-- These are used to define custom OctoFHIR resources like App, CustomOperation, etc.

-- Create the octofhir schema for internal conformance resources
CREATE SCHEMA IF NOT EXISTS octofhir;

-- ============================================================================
-- CONFORMANCE RESOURCE TABLES
-- ============================================================================

-- StructureDefinition table - stores resource/profile definitions
CREATE TABLE IF NOT EXISTS octofhir.structuredefinition (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url TEXT NOT NULL,
    version TEXT,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    kind TEXT NOT NULL,  -- primitive-type, complex-type, resource, logical
    type TEXT,
    base_definition TEXT,
    derivation TEXT,  -- specialization, constraint
    txid BIGINT NOT NULL REFERENCES public._transaction(txid),
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resource JSONB NOT NULL,
    UNIQUE (url, version)
);

-- StructureDefinition history table
CREATE TABLE IF NOT EXISTS octofhir.structuredefinition_history (
    id UUID NOT NULL,
    url TEXT NOT NULL,
    version TEXT,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    kind TEXT NOT NULL,
    type TEXT,
    base_definition TEXT,
    derivation TEXT,
    txid BIGINT NOT NULL,
    ts TIMESTAMPTZ NOT NULL,
    resource JSONB NOT NULL,
    PRIMARY KEY (id, txid)
);

-- ValueSet table - stores value set definitions
CREATE TABLE IF NOT EXISTS octofhir.valueset (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url TEXT NOT NULL,
    version TEXT,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    txid BIGINT NOT NULL REFERENCES public._transaction(txid),
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resource JSONB NOT NULL,
    UNIQUE (url, version)
);

-- ValueSet history table
CREATE TABLE IF NOT EXISTS octofhir.valueset_history (
    id UUID NOT NULL,
    url TEXT NOT NULL,
    version TEXT,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    txid BIGINT NOT NULL,
    ts TIMESTAMPTZ NOT NULL,
    resource JSONB NOT NULL,
    PRIMARY KEY (id, txid)
);

-- CodeSystem table - stores code system definitions
CREATE TABLE IF NOT EXISTS octofhir.codesystem (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url TEXT NOT NULL,
    version TEXT,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    content TEXT NOT NULL DEFAULT 'complete',  -- not-present, example, fragment, complete, supplement
    txid BIGINT NOT NULL REFERENCES public._transaction(txid),
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resource JSONB NOT NULL,
    UNIQUE (url, version)
);

-- CodeSystem history table
CREATE TABLE IF NOT EXISTS octofhir.codesystem_history (
    id UUID NOT NULL,
    url TEXT NOT NULL,
    version TEXT,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    content TEXT NOT NULL,
    txid BIGINT NOT NULL,
    ts TIMESTAMPTZ NOT NULL,
    resource JSONB NOT NULL,
    PRIMARY KEY (id, txid)
);

-- SearchParameter table - stores search parameter definitions
CREATE TABLE IF NOT EXISTS octofhir.searchparameter (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url TEXT NOT NULL,
    version TEXT,
    name TEXT NOT NULL,
    code TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    base TEXT[] NOT NULL,  -- resource types this applies to
    type TEXT NOT NULL,  -- number, date, string, token, reference, composite, quantity, uri, special
    expression TEXT,
    txid BIGINT NOT NULL REFERENCES public._transaction(txid),
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resource JSONB NOT NULL,
    UNIQUE (url, version)
);

-- SearchParameter history table
CREATE TABLE IF NOT EXISTS octofhir.searchparameter_history (
    id UUID NOT NULL,
    url TEXT NOT NULL,
    version TEXT,
    name TEXT NOT NULL,
    code TEXT NOT NULL,
    status TEXT NOT NULL,
    base TEXT[] NOT NULL,
    type TEXT NOT NULL,
    expression TEXT,
    txid BIGINT NOT NULL,
    ts TIMESTAMPTZ NOT NULL,
    resource JSONB NOT NULL,
    PRIMARY KEY (id, txid)
);

-- ============================================================================
-- INDEXES
-- ============================================================================

-- StructureDefinition indexes
CREATE INDEX IF NOT EXISTS idx_octofhir_sd_url ON octofhir.structuredefinition(url);
CREATE INDEX IF NOT EXISTS idx_octofhir_sd_name ON octofhir.structuredefinition(name);
CREATE INDEX IF NOT EXISTS idx_octofhir_sd_type ON octofhir.structuredefinition(type);
CREATE INDEX IF NOT EXISTS idx_octofhir_sd_kind ON octofhir.structuredefinition(kind);
CREATE INDEX IF NOT EXISTS idx_octofhir_sd_status ON octofhir.structuredefinition(status);
CREATE INDEX IF NOT EXISTS idx_octofhir_sd_gin ON octofhir.structuredefinition USING GIN (resource jsonb_path_ops);
CREATE INDEX IF NOT EXISTS idx_octofhir_sd_history_id ON octofhir.structuredefinition_history(id);
CREATE INDEX IF NOT EXISTS idx_octofhir_sd_history_ts ON octofhir.structuredefinition_history(ts);

-- ValueSet indexes
CREATE INDEX IF NOT EXISTS idx_octofhir_vs_url ON octofhir.valueset(url);
CREATE INDEX IF NOT EXISTS idx_octofhir_vs_name ON octofhir.valueset(name);
CREATE INDEX IF NOT EXISTS idx_octofhir_vs_status ON octofhir.valueset(status);
CREATE INDEX IF NOT EXISTS idx_octofhir_vs_gin ON octofhir.valueset USING GIN (resource jsonb_path_ops);
CREATE INDEX IF NOT EXISTS idx_octofhir_vs_history_id ON octofhir.valueset_history(id);
CREATE INDEX IF NOT EXISTS idx_octofhir_vs_history_ts ON octofhir.valueset_history(ts);

-- CodeSystem indexes
CREATE INDEX IF NOT EXISTS idx_octofhir_cs_url ON octofhir.codesystem(url);
CREATE INDEX IF NOT EXISTS idx_octofhir_cs_name ON octofhir.codesystem(name);
CREATE INDEX IF NOT EXISTS idx_octofhir_cs_status ON octofhir.codesystem(status);
CREATE INDEX IF NOT EXISTS idx_octofhir_cs_gin ON octofhir.codesystem USING GIN (resource jsonb_path_ops);
CREATE INDEX IF NOT EXISTS idx_octofhir_cs_history_id ON octofhir.codesystem_history(id);
CREATE INDEX IF NOT EXISTS idx_octofhir_cs_history_ts ON octofhir.codesystem_history(ts);

-- SearchParameter indexes
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_url ON octofhir.searchparameter(url);
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_name ON octofhir.searchparameter(name);
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_code ON octofhir.searchparameter(code);
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_base ON octofhir.searchparameter USING GIN (base);
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_type ON octofhir.searchparameter(type);
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_status ON octofhir.searchparameter(status);
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_gin ON octofhir.searchparameter USING GIN (resource jsonb_path_ops);
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_history_id ON octofhir.searchparameter_history(id);
CREATE INDEX IF NOT EXISTS idx_octofhir_sp_history_ts ON octofhir.searchparameter_history(ts);

-- ============================================================================
-- HISTORY TRIGGER FUNCTION
-- ============================================================================

-- Generic history archive function for octofhir schema
CREATE OR REPLACE FUNCTION octofhir.archive_to_history()
RETURNS TRIGGER AS $$
BEGIN
    EXECUTE format(
        'INSERT INTO octofhir.%I_history SELECT ($1).*',
        TG_TABLE_NAME
    ) USING OLD;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- History triggers for each table
DROP TRIGGER IF EXISTS structuredefinition_history_trigger ON octofhir.structuredefinition;
CREATE TRIGGER structuredefinition_history_trigger
    BEFORE UPDATE OR DELETE ON octofhir.structuredefinition
    FOR EACH ROW EXECUTE FUNCTION octofhir.archive_to_history();

DROP TRIGGER IF EXISTS valueset_history_trigger ON octofhir.valueset;
CREATE TRIGGER valueset_history_trigger
    BEFORE UPDATE OR DELETE ON octofhir.valueset
    FOR EACH ROW EXECUTE FUNCTION octofhir.archive_to_history();

DROP TRIGGER IF EXISTS codesystem_history_trigger ON octofhir.codesystem;
CREATE TRIGGER codesystem_history_trigger
    BEFORE UPDATE OR DELETE ON octofhir.codesystem
    FOR EACH ROW EXECUTE FUNCTION octofhir.archive_to_history();

DROP TRIGGER IF EXISTS searchparameter_history_trigger ON octofhir.searchparameter;
CREATE TRIGGER searchparameter_history_trigger
    BEFORE UPDATE OR DELETE ON octofhir.searchparameter
    FOR EACH ROW EXECUTE FUNCTION octofhir.archive_to_history();

-- ============================================================================
-- NOTIFICATION TRIGGERS FOR HOT-RELOAD
-- ============================================================================

-- Notification function for conformance resource changes
CREATE OR REPLACE FUNCTION octofhir.notify_conformance_change()
RETURNS TRIGGER AS $$
DECLARE
    payload JSONB;
BEGIN
    payload := jsonb_build_object(
        'table', TG_TABLE_NAME,
        'operation', TG_OP,
        'id', COALESCE(NEW.id, OLD.id)::TEXT,
        'url', COALESCE(NEW.url, OLD.url)
    );

    PERFORM pg_notify('octofhir_conformance_changes', payload::TEXT);

    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Notification triggers for hot-reload
DROP TRIGGER IF EXISTS structuredefinition_notify_trigger ON octofhir.structuredefinition;
CREATE TRIGGER structuredefinition_notify_trigger
    AFTER INSERT OR UPDATE OR DELETE ON octofhir.structuredefinition
    FOR EACH ROW EXECUTE FUNCTION octofhir.notify_conformance_change();

DROP TRIGGER IF EXISTS valueset_notify_trigger ON octofhir.valueset;
CREATE TRIGGER valueset_notify_trigger
    AFTER INSERT OR UPDATE OR DELETE ON octofhir.valueset
    FOR EACH ROW EXECUTE FUNCTION octofhir.notify_conformance_change();

DROP TRIGGER IF EXISTS codesystem_notify_trigger ON octofhir.codesystem;
CREATE TRIGGER codesystem_notify_trigger
    AFTER INSERT OR UPDATE OR DELETE ON octofhir.codesystem
    FOR EACH ROW EXECUTE FUNCTION octofhir.notify_conformance_change();

DROP TRIGGER IF EXISTS searchparameter_notify_trigger ON octofhir.searchparameter;
CREATE TRIGGER searchparameter_notify_trigger
    AFTER INSERT OR UPDATE OR DELETE ON octofhir.searchparameter
    FOR EACH ROW EXECUTE FUNCTION octofhir.notify_conformance_change();

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON SCHEMA octofhir IS 'Internal OctoFHIR conformance resources for custom resource types';
COMMENT ON TABLE octofhir.structuredefinition IS 'Stores StructureDefinitions for internal OctoFHIR resource types (App, CustomOperation, etc.)';
COMMENT ON TABLE octofhir.valueset IS 'Stores ValueSets for internal OctoFHIR terminology';
COMMENT ON TABLE octofhir.codesystem IS 'Stores CodeSystems for internal OctoFHIR terminology';
COMMENT ON TABLE octofhir.searchparameter IS 'Stores SearchParameters for internal OctoFHIR resources';
