-- ============================================================================
-- DB CONSOLE QUERY HISTORY
-- ============================================================================
-- Stores SQL query execution history per user for the DB Console feature.

CREATE TABLE IF NOT EXISTS db_console_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id VARCHAR(255) NOT NULL,
    query TEXT NOT NULL,
    execution_time_ms BIGINT,
    row_count INTEGER,
    is_error BOOLEAN NOT NULL DEFAULT FALSE,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_db_console_history_user
    ON db_console_history(user_id, created_at DESC);
