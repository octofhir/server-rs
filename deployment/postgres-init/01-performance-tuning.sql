-- PostgreSQL Extensions for OctoFHIR
-- This file is automatically executed when PostgreSQL container starts for the first time.
-- Performance settings are configured via docker-compose command flags (not ALTER SYSTEM).

-- Enable useful extensions
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
