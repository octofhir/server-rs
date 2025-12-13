-- Operation registry table
-- Stores definitions of all server operations for UI display and policy targeting

CREATE TABLE IF NOT EXISTS operations (
    -- Unique operation ID (e.g., "fhir.read", "graphql.query")
    id TEXT PRIMARY KEY,

    -- Human-readable name
    name TEXT NOT NULL,

    -- Description of what this operation does
    description TEXT,

    -- Category for grouping (fhir, graphql, system, auth, ui, api)
    category TEXT NOT NULL,

    -- HTTP methods as JSON array (["GET", "POST", etc.])
    methods JSONB NOT NULL DEFAULT '[]'::jsonb,

    -- URL path pattern (e.g., "/{type}/{id}", "/fhir/$graphql")
    path_pattern TEXT NOT NULL,

    -- Whether this operation is public (no auth required)
    public BOOLEAN NOT NULL DEFAULT false,

    -- Module that provides this operation (e.g., "octofhir-server", app ID)
    module TEXT NOT NULL,

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_operations_category ON operations(category);
CREATE INDEX IF NOT EXISTS idx_operations_module ON operations(module);
CREATE INDEX IF NOT EXISTS idx_operations_public ON operations(public);

-- Trigger for updated_at
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

-- Comments for documentation
COMMENT ON TABLE operations IS 'Registry of all server operations for UI display and policy targeting';
COMMENT ON COLUMN operations.id IS 'Unique operation ID (e.g., fhir.read, graphql.query)';
COMMENT ON COLUMN operations.category IS 'Operation category: fhir, graphql, system, auth, ui, api';
COMMENT ON COLUMN operations.methods IS 'HTTP methods as JSON array';
COMMENT ON COLUMN operations.public IS 'If true, operation does not require authentication';
COMMENT ON COLUMN operations.module IS 'Module providing this operation (for API gateway filtering)';
