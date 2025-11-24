-- Migration: Add NOTIFY triggers for gateway resources (App, CustomOperation)
-- This enables hot-reload of gateway routes when App or CustomOperation resources change

-- Create a channel for gateway resource changes
-- We'll use the same notification mechanism as conformance resources

-- Function to notify on gateway resource changes
CREATE OR REPLACE FUNCTION notify_gateway_resource_change()
RETURNS TRIGGER AS $$
DECLARE
    resource_json JSONB;
    resource_type TEXT;
    notification_payload JSON;
BEGIN
    -- Get the resource JSON from the appropriate row
    IF TG_OP = 'DELETE' THEN
        resource_json := OLD.resource;
    ELSE
        resource_json := NEW.resource;
    END IF;

    -- Extract resourceType
    resource_type := resource_json->>'resourceType';

    -- Only process App and CustomOperation resources
    IF resource_type NOT IN ('App', 'CustomOperation') THEN
        RETURN COALESCE(NEW, OLD);
    END IF;

    -- Build notification payload
    notification_payload := json_build_object(
        'table', 'resource',
        'resource_type', resource_type,
        'operation', TG_OP,
        'id', COALESCE(NEW.id, OLD.id),
        'version_id', COALESCE(NEW.version_id, OLD.version_id)
    );

    -- Send notification
    PERFORM pg_notify('octofhir_gateway_changes', notification_payload::text);

    RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

-- Create trigger on resource table for INSERT/UPDATE/DELETE
DROP TRIGGER IF EXISTS gateway_resource_notify ON resource;
CREATE TRIGGER gateway_resource_notify
    AFTER INSERT OR UPDATE OR DELETE ON resource
    FOR EACH ROW
    EXECUTE FUNCTION notify_gateway_resource_change();

-- Add comment
COMMENT ON TRIGGER gateway_resource_notify ON resource IS
    'Sends PostgreSQL NOTIFY event when App or CustomOperation resources change, enabling hot-reload of gateway routes';

-- Test the trigger (optional, for verification)
-- You can run this in psql to verify:
-- LISTEN octofhir_gateway_changes;
-- INSERT INTO resource (id, version_id, resource_type, status, resource) VALUES
--   (gen_random_uuid(), 1, 'App', 'created', '{"resourceType": "App", "name": "Test", "basePath": "/test", "active": true}'::jsonb);
-- You should see a NOTIFY message
