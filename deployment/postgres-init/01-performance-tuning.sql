-- PostgreSQL Performance Tuning for OctoFHIR
-- This file is automatically executed when PostgreSQL container starts

-- Enable useful extensions
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Performance tuning settings (adjust based on your server resources)
-- These are overridden by docker-compose environment variables, but included for reference

-- Connection settings
ALTER SYSTEM SET max_connections = 200;

-- Memory settings
ALTER SYSTEM SET shared_buffers = '256MB';
ALTER SYSTEM SET effective_cache_size = '1GB';
ALTER SYSTEM SET maintenance_work_mem = '64MB';
ALTER SYSTEM SET work_mem = '16MB';

-- WAL settings
ALTER SYSTEM SET wal_buffers = '8MB';
ALTER SYSTEM SET checkpoint_completion_target = 0.9;
ALTER SYSTEM SET max_wal_size = '1GB';
ALTER SYSTEM SET min_wal_size = '80MB';

-- Query planner settings
ALTER SYSTEM SET random_page_cost = 1.1;
ALTER SYSTEM SET effective_io_concurrency = 200;

-- Logging
ALTER SYSTEM SET log_min_duration_statement = 1000;  -- Log queries slower than 1s
ALTER SYSTEM SET log_line_prefix = '%t [%p]: [%l-1] user=%u,db=%d,app=%a,client=%h ';
ALTER SYSTEM SET log_checkpoints = on;
ALTER SYSTEM SET log_connections = on;
ALTER SYSTEM SET log_disconnections = on;
ALTER SYSTEM SET log_lock_waits = on;

-- Autovacuum tuning
ALTER SYSTEM SET autovacuum_max_workers = 3;
ALTER SYSTEM SET autovacuum_naptime = '10s';

-- Reload configuration
SELECT pg_reload_conf();
