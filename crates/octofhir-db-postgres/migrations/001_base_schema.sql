-- Base schema for OctoFHIR PostgreSQL storage
-- This creates the foundational types and tables needed by all resource tables.

-- Transaction log for atomicity
CREATE TABLE IF NOT EXISTS _transaction (
    txid BIGSERIAL PRIMARY KEY,
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status VARCHAR(16) NOT NULL DEFAULT 'committed'
);

-- Resource status enum
DO $$ BEGIN
    CREATE TYPE resource_status AS ENUM ('created', 'updated', 'deleted');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Index on transaction timestamp for history queries
CREATE INDEX IF NOT EXISTS idx_transaction_ts ON _transaction(ts);
