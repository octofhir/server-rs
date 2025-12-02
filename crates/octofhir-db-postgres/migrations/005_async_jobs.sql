-- Migration: Async Jobs Infrastructure
-- Description: Creates tables and indexes for FHIR asynchronous request pattern support
-- Allows long-running operations to execute in background with status polling

-- Create async_jobs table for tracking background job execution
CREATE TABLE IF NOT EXISTS async_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Job status and lifecycle
    status VARCHAR(20) NOT NULL DEFAULT 'queued',
    request_type VARCHAR(50) NOT NULL,           -- e.g., 'transaction', '$everything', 'search'
    request_method VARCHAR(10) NOT NULL,         -- GET, POST, etc.
    request_url TEXT NOT NULL,                   -- Original request URL
    request_body JSONB,                          -- Request payload if applicable
    request_headers JSONB,                       -- Relevant headers

    -- Result and error handling
    result JSONB,                                -- Final result when completed
    progress FLOAT DEFAULT 0,                    -- Progress from 0.0 to 1.0
    error_message TEXT,                          -- Error details if failed

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,

    -- Client tracking and expiration
    client_id VARCHAR(255),                      -- Client identifier for tracking
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '24 hours'),

    -- Constraints
    CONSTRAINT valid_status CHECK (status IN ('queued', 'in_progress', 'completed', 'failed', 'cancelled')),
    CONSTRAINT valid_progress CHECK (progress >= 0 AND progress <= 1),
    CONSTRAINT valid_method CHECK (request_method IN ('GET', 'POST', 'PUT', 'PATCH', 'DELETE'))
);

-- Index for efficient status queries (find jobs by status)
CREATE INDEX IF NOT EXISTS idx_async_jobs_status ON async_jobs(status) WHERE status IN ('queued', 'in_progress');

-- Index for client-specific job listing
CREATE INDEX IF NOT EXISTS idx_async_jobs_client ON async_jobs(client_id, created_at DESC) WHERE client_id IS NOT NULL;

-- Index for cleanup operations (expired jobs)
CREATE INDEX IF NOT EXISTS idx_async_jobs_expires ON async_jobs(expires_at) WHERE status IN ('queued', 'in_progress', 'completed', 'failed');

-- Index for job timestamps
CREATE INDEX IF NOT EXISTS idx_async_jobs_created ON async_jobs(created_at DESC);

-- Function to automatically update updated_at timestamp
CREATE OR REPLACE FUNCTION update_async_jobs_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to auto-update updated_at on row modification (PostgreSQL 14+)
CREATE OR REPLACE TRIGGER trigger_async_jobs_updated_at
    BEFORE UPDATE ON async_jobs
    FOR EACH ROW
    EXECUTE FUNCTION update_async_jobs_updated_at();

-- Create view for active jobs (commonly queried)
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

COMMENT ON TABLE async_jobs IS 'Tracks asynchronous FHIR operations for Prefer: respond-async pattern';
COMMENT ON COLUMN async_jobs.status IS 'Current job status: queued, in_progress, completed, failed, cancelled';
COMMENT ON COLUMN async_jobs.progress IS 'Job progress from 0.0 (started) to 1.0 (complete)';
COMMENT ON COLUMN async_jobs.expires_at IS 'Job results expire after this timestamp (default 24 hours)';
