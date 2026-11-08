-- Integration metadata only. DICOM objects and pixel payloads stay in PACS.
CREATE TABLE dicom_sync_sources (
    connection_name TEXT PRIMARY KEY CHECK (length(connection_name) BETWEEN 1 AND 64),
    config_fingerprint TEXT NOT NULL,
    scan_offset BIGINT NOT NULL DEFAULT 0 CHECK (scan_offset >= 0),
    scan_not_before TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    last_scan_at TIMESTAMPTZ,
    last_error TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE dicom_sync_studies (
    connection_name TEXT NOT NULL REFERENCES dicom_sync_sources(connection_name),
    study_uid TEXT NOT NULL CHECK (length(study_uid) BETWEEN 1 AND 64),
    desired_revision BIGINT NOT NULL DEFAULT 1,
    applied_revision BIGINT NOT NULL DEFAULT 0,
    pending_job UUID REFERENCES async_jobs(id) ON DELETE SET NULL,
    expected_instances BIGINT CHECK (expected_instances >= 0),
    patient_id TEXT,
    override_state TEXT CHECK (override_state IN ('deleted','rejected')),
    last_sequence BIGINT NOT NULL DEFAULT -1,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    last_error TEXT,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    last_applied_at TIMESTAMPTZ,
    PRIMARY KEY (connection_name, study_uid),
    CHECK (desired_revision >= applied_revision)
);
CREATE INDEX dicom_sync_due ON dicom_sync_studies(next_attempt_at, connection_name)
    WHERE desired_revision > applied_revision;
