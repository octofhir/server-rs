-- $bulk-submit (Bulk Data Access IG): a submission is identified by its submitter and
-- submissionId, and holds any number of manifests. Each resource a manifest writes is
-- recorded, because submissionStatus=stopped and replacesManifestUrl must undo exactly
-- that manifest's data.

CREATE TABLE IF NOT EXISTS bulk_submissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    submitter_system TEXT NOT NULL,
    submitter_value TEXT NOT NULL,
    submission_id TEXT NOT NULL,
    client_id TEXT,
    status TEXT NOT NULL DEFAULT 'in-progress',
    revert_job_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT bulk_submissions_identity UNIQUE (submitter_system, submitter_value, submission_id),
    CONSTRAINT bulk_submissions_status CHECK (status IN ('in-progress', 'completed', 'stopped'))
);

CREATE TABLE IF NOT EXISTS bulk_submission_manifests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    submission UUID NOT NULL REFERENCES bulk_submissions (id) ON DELETE CASCADE,
    manifest_url TEXT NOT NULL,
    job_id UUID,
    status TEXT NOT NULL DEFAULT 'queued',
    -- Outcome file items of the status manifest, written when the job ends.
    outcome JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT bulk_submission_manifest_url UNIQUE (submission, manifest_url),
    CONSTRAINT bulk_submission_manifest_status
        CHECK (status IN ('queued', 'processing', 'completed', 'failed', 'replaced', 'stopped'))
);

-- prior_version is NULL when the manifest created the resource. It is recorded before
-- the write commits, so a replayed chunk keeps the first value it saw.
CREATE TABLE IF NOT EXISTS bulk_submission_writes (
    seq BIGSERIAL PRIMARY KEY,
    manifest UUID NOT NULL REFERENCES bulk_submission_manifests (id) ON DELETE CASCADE,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    prior_version TEXT,
    written_version TEXT,
    CONSTRAINT bulk_submission_write_target UNIQUE (manifest, resource_type, resource_id)
);
