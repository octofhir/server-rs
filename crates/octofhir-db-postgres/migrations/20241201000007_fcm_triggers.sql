-- FCM Triggers and Functions
-- This migration adds notification triggers and helper functions for the FCM schema

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Function to extract and populate search fields from content when inserting/updating
CREATE OR REPLACE FUNCTION fcm.extract_search_fields()
RETURNS TRIGGER AS $func$
BEGIN
    -- Populate lowercase versions for case-insensitive search
    NEW.id_lower := lower(NEW.resource_id);
    NEW.name_lower := lower(NEW.name);
    NEW.url_lower := lower(NEW.url);

    -- Extract text fields from content
    NEW.title := NEW.content->>'title';
    NEW.description := NEW.content->>'description';
    NEW.status := NEW.content->>'status';
    NEW.publisher := NEW.content->>'publisher';

    -- Build full-text search vector
    NEW.search_vector :=
        setweight(to_tsvector('english', coalesce(NEW.name, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.title, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(NEW.description, '')), 'C');

    RETURN NEW;
END;
$func$ LANGUAGE plpgsql;

-- Trigger to automatically extract search fields
DROP TRIGGER IF EXISTS fcm_resources_extract_fields ON fcm.resources;
CREATE TRIGGER fcm_resources_extract_fields
    BEFORE INSERT OR UPDATE ON fcm.resources
    FOR EACH ROW EXECUTE FUNCTION fcm.extract_search_fields();

-- ============================================================================
-- NOTIFICATION TRIGGERS FOR HOT-RELOAD
-- ============================================================================

-- Notification function for package changes
CREATE OR REPLACE FUNCTION fcm.notify_package_change()
RETURNS TRIGGER AS $func$
BEGIN
    PERFORM pg_notify('fcm_package_changes',
        json_build_object(
            'operation', TG_OP,
            'package_name', COALESCE(NEW.name, OLD.name),
            'package_version', COALESCE(NEW.version, OLD.version)
        )::text
    );
    RETURN COALESCE(NEW, OLD);
END;
$func$ LANGUAGE plpgsql;

-- Trigger for package changes
DROP TRIGGER IF EXISTS fcm_packages_notify ON fcm.packages;
CREATE TRIGGER fcm_packages_notify
    AFTER INSERT OR UPDATE OR DELETE ON fcm.packages
    FOR EACH ROW EXECUTE FUNCTION fcm.notify_package_change();

-- ============================================================================
-- RESOURCE COUNT MANAGEMENT
-- ============================================================================

-- Function to update package resource count
CREATE OR REPLACE FUNCTION fcm.update_package_resource_count()
RETURNS TRIGGER AS $func$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE fcm.packages
        SET resource_count = resource_count + 1
        WHERE name = NEW.package_name AND version = NEW.package_version;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE fcm.packages
        SET resource_count = resource_count - 1
        WHERE name = OLD.package_name AND version = OLD.package_version;
    END IF;
    RETURN NULL;
END;
$func$ LANGUAGE plpgsql;

-- Trigger to maintain resource count
DROP TRIGGER IF EXISTS fcm_resources_count ON fcm.resources;
CREATE TRIGGER fcm_resources_count
    AFTER INSERT OR DELETE ON fcm.resources
    FOR EACH ROW EXECUTE FUNCTION fcm.update_package_resource_count();
