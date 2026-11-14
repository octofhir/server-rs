-- Degraded notification mode: the WebSocket channel is unavailable but a
-- filtered deletion-lock subscription (PS3.18 §11.10) is still held, so the
-- worker may create workitems while the per-task poll drives state convergence.
-- Durable (not in-memory): the watcher and the worker live in different tasks
-- and must agree on the fallback across process restarts.
ALTER TABLE dicom_ups_sources ADD COLUMN poll_only boolean NOT NULL DEFAULT false;
