-- Durable Requestor/Watcher state. Requests contain typed metadata references,
-- never serialized DICOM datasets, bearer tokens, images or inference payloads.
CREATE TABLE dicom_ups_sources (
 connection_name text PRIMARY KEY,
 config_fingerprint text NOT NULL,
 subscription_prefix text NOT NULL DEFAULT ('OFHIR ' || replace(gen_random_uuid()::text,'-','') || ' '),
 watch_ready_until timestamptz,
 watch_execution uuid,
 watch_error text,
 updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE dicom_ups_tasks (
 task_id text PRIMARY KEY,
 connection_name text NOT NULL REFERENCES dicom_ups_sources(connection_name),
 workitem_uid text NOT NULL UNIQUE,
 request_metadata jsonb NOT NULL CHECK (jsonb_typeof(request_metadata)='object'),
 requested_by text NOT NULL,
 cancel_requested boolean NOT NULL DEFAULT false,
 cancel_delivered boolean NOT NULL DEFAULT false,
 desired_revision bigint NOT NULL DEFAULT 1,
 applied_revision bigint NOT NULL DEFAULT 0,
 pending_job uuid REFERENCES async_jobs(id) ON DELETE SET NULL,
 remote_state text CHECK (remote_state IN ('SCHEDULED','IN PROGRESS','COMPLETED','CANCELED')),
 created_confirmed boolean NOT NULL DEFAULT false,
 creation_started boolean NOT NULL DEFAULT false,
 cancelled_before_submission boolean NOT NULL DEFAULT false,
 subscription_release_execution uuid,
 attempts integer NOT NULL DEFAULT 0,
 next_attempt_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 last_error text,
 last_applied_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK (desired_revision>=applied_revision)
);
CREATE INDEX dicom_ups_due ON dicom_ups_tasks(connection_name,next_attempt_at);

-- The function is shared with SchemaManager for Task tables created after migration.

DO $task$ BEGIN
 IF to_regclass('task') IS NOT NULL THEN
  DROP TRIGGER IF EXISTS dicom_ups_guard ON task;
  CREATE TRIGGER dicom_ups_guard BEFORE INSERT OR UPDATE OR DELETE ON task
   FOR EACH ROW EXECUTE FUNCTION octofhir_dicom_task_guard();
 END IF;
END $task$;
