-- Migration: Add NOTIFY function for gateway resources (App, CustomOperation)
-- This enables hot-reload of gateway routes when App or CustomOperation resources change
--
-- NOTE: This schema uses table-per-resource pattern (e.g., `app`, `customoperation` tables).
-- This migration creates only the notification function.
-- The SchemaManager applies triggers when creating `app` and `customoperation` tables.

-- Function to notify on gateway resource changes
-- Works with table-per-resource pattern - uses TG_TABLE_NAME to identify resource type
CREATE OR REPLACE FUNCTION notify_gateway_resource_change()
RETURNS TRIGGER AS $$
DECLARE
    resource_json JSONB;
    notification_payload JSON;
BEGIN
    -- Get the resource JSON from the appropriate row
    IF TG_OP = 'DELETE' THEN
        resource_json := OLD.resource;
    ELSE
        resource_json := NEW.resource;
    END IF;

    -- Build notification payload
    -- Use TG_TABLE_NAME to get resource type (table name = resource type in lowercase)
    notification_payload := json_build_object(
        'table', TG_TABLE_NAME,
        'resource_type', resource_json->>'resourceType',
        'operation', TG_OP,
        'id', COALESCE(NEW.id, OLD.id)::TEXT
    );

    -- Send notification
    PERFORM pg_notify('octofhir_gateway_changes', notification_payload::text);

    RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

-- Note: Triggers are created dynamically by SchemaManager when 'app' or 'customoperation'
-- tables are created. See SchemaManager.create_gateway_trigger() for implementation.
