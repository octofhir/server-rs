-- Migration: Add NOTIFY function for AccessPolicy resources
-- This enables hot-reload of access policies when AccessPolicy resources change
--
-- NOTE: This schema uses table-per-resource pattern (e.g., `accesspolicy` table).
-- This migration creates only the notification function.
-- The SchemaManager applies triggers when creating the `accesspolicy` table.

-- Function to notify on policy changes
-- Works with table-per-resource pattern - sends policy ID and operation
CREATE OR REPLACE FUNCTION notify_policy_change()
RETURNS TRIGGER AS $func$
DECLARE
    notification_payload JSON;
    resource_id TEXT;
BEGIN
    -- Get the resource ID from the appropriate row
    IF TG_OP = 'DELETE' THEN
        resource_id := OLD.id::TEXT;
    ELSE
        resource_id := NEW.id::TEXT;
    END IF;

    -- Build notification payload
    notification_payload := json_build_object(
        'operation', TG_OP,
        'id', resource_id
    );

    -- Send notification
    PERFORM pg_notify('octofhir_policy_changes', notification_payload::text);

    RETURN COALESCE(NEW, OLD);
END;
$func$ LANGUAGE plpgsql;

-- Note: Triggers are created dynamically by SchemaManager when 'accesspolicy'
-- table is created. See SchemaManager.create_policy_trigger() for implementation.
