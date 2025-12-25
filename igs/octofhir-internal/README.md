# OctoFHIR Internal Implementation Guide

This Implementation Guide defines OctoFHIR's internal conformance resources for custom resource types and API Gateway functionality.

## Overview

The `octofhir.internal` package contains:

- **App**: Groups custom operations under a common base path
- **CustomOperation**: Defines custom API endpoints with configurable behavior
- **ValueSets & CodeSystems**: Supporting terminology

## Resources

### App

An App groups custom operations under a common base path and configuration.

**Key elements:**
- `name`: Human-readable name
- `description`: Detailed description
- `basePath`: Base URL path (e.g., `/api/v1/custom`)
- `active`: Whether the app is active
- `authentication`: Optional authentication configuration

### CustomOperation

A CustomOperation defines a custom API endpoint with configurable behavior.

**Key elements:**
- `app`: Reference to parent App
- `path`: Relative path (supports parameters like `:id`)
- `method`: HTTP method (GET, POST, PUT, DELETE, PATCH)
- `type`: Operation type:
  - `proxy`: Forward to external service
  - `sql`: Execute SQL query
  - `fhirpath`: Evaluate FHIRPath expression
  - `handler`: Invoke custom Rust handler
- `active`: Whether the operation is active

**Type-specific configuration:**
- `proxy.url`: Target URL for proxy operations
- `proxy.timeout`: Request timeout in seconds
- `sql`: SQL query string (with parameters)
- `fhirpath`: FHIRPath expression
- `handler`: Handler identifier

## Terminology

### HTTP Methods ValueSet

Supported HTTP methods: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS

### Operation Types ValueSet

- `proxy`: Forward requests to external service
- `sql`: Execute SQL query against database
- `fhirpath`: Evaluate FHIRPath expression
- `handler`: Invoke registered custom handler

### Operation Outcome Types CodeSystem

Extended codes for `OperationOutcome.issue.details` to provide specific error categorization:

- `non-existent-resource`: A Reference points to a resource that does not exist in the database
- `contained-not-found`: A contained reference (#id) does not exist within the resource
- `bundle-entry-not-found`: A Bundle reference (urn:uuid or fullUrl) does not exist in entries
- `invalid-reference-type`: The resourceType in a Reference is not allowed by targetProfile
- `reference-error`: Generic reference validation error

## Usage

### Database Storage

These resources are stored in the `octofhir` schema in PostgreSQL:

- `octofhir.structuredefinition`
- `octofhir.valueset`
- `octofhir.codesystem`
- `octofhir.searchparameter`

### Loading into Canonical Manager

The conformance resources are automatically synced to the canonical manager on server startup and when resources change (hot-reload).

```rust
use octofhir_db_postgres::{
    PostgresConformanceStorage,
    sync_and_load,
};

// Create conformance storage
let conformance_storage = PostgresConformanceStorage::new(pool.clone());

// Sync to canonical manager
let package_dir = sync_and_load(
    &conformance_storage,
    &base_dir,
    Some(&canonical_manager),
).await?;
```

### Hot-Reload

Changes to conformance resources trigger automatic re-synchronization:

```rust
use octofhir_db_postgres::HotReloadBuilder;

let hot_reload = HotReloadBuilder::new(pool.clone())
    .with_conformance_storage(Arc::new(conformance_storage))
    .with_canonical_manager(Arc::new(canonical_manager))
    .with_base_dir(base_dir)
    .start()?;
```

## Example: Creating an App

```json
{
  "resourceType": "App",
  "name": "External API Integration",
  "description": "Custom operations for integrating with external services",
  "basePath": "/api/v1/external",
  "active": true,
  "authentication": {
    "type": "bearer",
    "required": true
  }
}
```

## Example: Creating a Proxy Operation

```json
{
  "resourceType": "CustomOperation",
  "app": {
    "reference": "App/external-api"
  },
  "path": "/users/:id",
  "method": "GET",
  "type": "proxy",
  "proxy": {
    "url": "https://api.example.com/users/:id",
    "timeout": 30
  },
  "active": true,
  "description": "Proxy user requests to external API"
}
```

## Example: Creating a SQL Operation

```json
{
  "resourceType": "CustomOperation",
  "app": {
    "reference": "App/external-api"
  },
  "path": "/stats",
  "method": "GET",
  "type": "sql",
  "sql": "SELECT resource_type, COUNT(*) as count FROM public.patient GROUP BY resource_type",
  "active": true,
  "description": "Get resource statistics"
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     PostgreSQL                              │
├─────────────────────┬───────────────────────────────────────┤
│   public schema     │          octofhir schema              │
│   (clinical data)   │       (internal conformance)          │
├─────────────────────┼───────────────────────────────────────┤
│ patient             │ structuredefinition                   │
│ observation         │ valueset                              │
│ encounter           │ codesystem                            │
│ ...                 │ searchparameter                       │
│                     │                                       │
│ app        ←────────┼──── validated by SD from octofhir.*   │
│ customoperation     │                                       │
└─────────────────────┴───────────────────────────────────────┘
                              │
                              ▼ sync on startup + hot-reload
                    ┌─────────────────────┐
                    │  Canonical Manager  │
                    │  (in-memory cache)  │
                    └─────────────────────┘
                              │
                              ▼ validates instances
                    ┌─────────────────────┐
                    │   FHIRSchema        │
                    │   Validator         │
                    └─────────────────────┘
```

## Migration

The database schema is created by migration `002_octofhir_schema.sql`:

```sql
CREATE SCHEMA IF NOT EXISTS octofhir;

-- Tables for conformance resources
CREATE TABLE octofhir.structuredefinition (...);
CREATE TABLE octofhir.valueset (...);
CREATE TABLE octofhir.codesystem (...);
CREATE TABLE octofhir.searchparameter (...);

-- History tables for auditing
CREATE TABLE octofhir.structuredefinition_history (...);
-- ...

-- Triggers for NOTIFY on changes
CREATE TRIGGER structuredefinition_notify_trigger ...;
```

## Dependencies

- `hl7.fhir.r4.core`: 4.0.1 (base FHIR R4 specification)

## License

MIT OR Apache-2.0
