# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

OctoFHIR is a high-performance FHIR server written in Rust with a React web console. Supports FHIR R4, R4B, R5, and R6. The project is under active development.

## Build & Development Commands

Always use debug builds for development and testing. **Release builds are very slow (5-10+ minutes)** — only use `--release` for benchmarks and final deployment. For quick verification, always use debug builds (`cargo build`, `cargo test`, `cargo run`). A `justfile` is available with common commands (`just --list`).

```bash
# Start dependencies (PostgreSQL on port 5450, Redis on port 6380)
docker compose up -d         # or: just db-up

# Rust server (debug builds)
cargo run                    # Run server on http://localhost:8888
cargo test                   # Run all tests
cargo test <test_name>       # Run single test
cargo test -p octofhir-auth  # Run tests for specific crate
cargo build                  # Build debug binary
cargo clippy                 # Lint (or: just lint)
cargo fmt                    # Format code (or: just fmt)
cargo watch -x run           # Auto-reload on changes (or: just dev)

# Tests with logging (useful for debugging)
RUST_LOG=debug cargo test <test_name> -- --nocapture

# Release builds (benchmarks and deployment)
cargo build --release                          # Build release binary
cargo build --release --features vendored-openssl  # Build with vendored OpenSSL (portable)
cargo bench                                    # Run benchmarks (or: just bench)

# UI (React + Vite, uses pnpm)
cd ui && pnpm install        # Install dependencies
cd ui && pnpm dev            # Dev server on http://localhost:4000/ui/
cd ui && pnpm build          # Production build (output to ui/dist/)
cd ui && pnpm typecheck      # TypeScript check
cd ui && pnpm lint           # oxlint
cd ui && pnpm format         # oxlint --fix

# Database access (non-standard port 5450)
PGPASSWORD=postgres psql -h localhost -p 5450 -U postgres -d octofhir

# Load testing with k6
just k6-setup                # Create test client (run once)
just k6-crud-test            # Single iteration CRUD test
just k6-crud-load            # Full load test (5 min, 300 VUs)
```

## Database Setup

PostgreSQL runs on port 5450 (not default 5432). Default connection: `postgres://postgres:postgres@localhost:5450/octofhir`. Redis runs on port 6380 (optional, for horizontal scaling).

### Dynamic Schema (No Migrations for Resources)

**FHIR resource tables are NOT created via migrations.** Tables are created dynamically at runtime by `SchemaManager` (`crates/octofhir-db-postgres/src/schema.rs`) when a resource type is first accessed. This design:

- Eliminates the need for migrations when adding support for new FHIR resource types
- Allows resource types to be defined by loading FHIR Implementation Guides (IGs)
- Creates tables with: `id`, `txid`, `created_at`, `updated_at`, `resource` (JSONB), `status`
- Automatically creates history tables (`{resource}_history`) and triggers for versioning
- Creates GIN indexes for efficient JSONB queries

**Migrations** in `crates/octofhir-db-postgres/migrations/` are only for:
- Core infrastructure tables (`_transaction`, enums, functions)
- Auth tables (users, clients, sessions, policies)
- FCM (canonical manager) storage
- Async jobs queue

## Configuration

Server configuration is in `octofhir.toml`. Override path with `OCTOFHIR_CONFIG` env var. Configuration can also be overridden via environment variables with `OCTOFHIR__` prefix (e.g., `OCTOFHIR__SERVER__PORT`).

## Architecture

### Workspace Crates (`crates/`)

- **octofhir-server**: Main HTTP server binary. Contains Axum routes, handlers, middleware, caching (Redis + local DashMap), and the embedded UI.

- **octofhir-core**: Core FHIR types, error handling, ID generation, and monitoring utilities.

- **octofhir-storage**: Storage abstraction layer defining the `FhirStorage` trait. No implementations—just the contract.

- **octofhir-db-postgres**: PostgreSQL storage backend implementing `FhirStorage`. Uses sqlx with migrations in `migrations/`.

- **octofhir-auth**: OAuth 2.0 / SMART on FHIR authentication. Includes:
  - Token management (JWT with RS256/RS384/ES384)
  - Policy engine with QuickJS scripting
  - External IdP federation
  - Rate limiting and audit logging

- **octofhir-auth-postgres**: PostgreSQL storage adapters for auth entities (users, clients, sessions, tokens, policies).

- **octofhir-search**: FHIR search parameter parsing and SQL query generation. Loads search parameters from FHIR packages via `octofhir-canonical-manager`.

- **octofhir-config**: Configuration management with hot-reload support via file watching.

- **octofhir-api**: HTTP API types (ApiError, ApiResponse, Bundle, CapabilityStatement) and FHIR-compliant error responses as OperationOutcome.

- **octofhir-sof**: SQL on FHIR implementation for ViewDefinition resources and tabular data export.

- **octofhir-graphql**: GraphQL API layer using `async-graphql` with dynamic schema generation from FHIR schemas.

- **octofhir-notifications**: Notification service for sending alerts (email, SMS, push) via configurable providers with template support and scheduling.

### External Dependencies

Local path dependencies (must exist alongside this repo):

- `octofhir-fhirpath`: FHIRPath evaluation engine (`../fhirpath-rs/crates/octofhir-fhirpath`)
- `octofhir-fhirschema`: FHIR schema validation (`../fhirschema/octofhir-fhirschema`)
- `octofhir-canonical-manager`: Package management for FHIR canonical resources (`../canonical-manager`)

Remote dependencies:

- `octofhir-fhir-model`: FHIR resource model with terminology support (crates.io)
- `banshee_*`: SQL parser/completion crates (crates.io, repo: `octofhir/banshee`) for DB console LSP

### UI (`ui/`)

React 19 application using:

- `@octofhir/ui-kit` (Base UI + CSS Modules + `--octo-*` tokens) for every UI primitive
- TanStack Query for data fetching
- Effector for state management
- Monaco Editor for SQL editor (DB console) and GraphQL
- GraphiQL for GraphQL playground
- oxlint for linting (no ESLint/Prettier)

The UI is embedded into the server binary at build time via `include_dir`. Build the UI first (`pnpm build` in `ui/`) before running `cargo build --release` for production. During development, run the UI dev server separately (`pnpm dev`) for hot reload.

### Database

PostgreSQL with migrations in `crates/octofhir-db-postgres/migrations/`. Key tables:

- Resource storage (FHIR resources as JSONB)
- Auth tables (users, clients, sessions, tokens, access policies)
- FCM (canonical manager) storage
- Async jobs queue

### Caching

Two-tier cache: local DashMap (L1) + optional Redis (L2) for horizontal scaling. Cache invalidation via Redis pub/sub.

## Key API Routes

- `/healthz` - health check endpoint for check server readiness
- `/fhir/metadata` - FHIR CapabilityStatement
- `/fhir/{ResourceType}` - FHIR REST API (CRUD, search)
- `/$graphql` - GraphQL endpoint
- `/oauth/*` - OAuth 2.0 / SMART on FHIR endpoints
- `/api/*` - Internal API (UI, settings, packages, LSP)
- `/admin/*` - Admin API (users, clients, configuration)
- `/ui` - Embedded web console

## Key Patterns

- Storage traits define contracts in `octofhir-storage`, implementations in `octofhir-db-postgres`
- Auth storage traits in `octofhir-auth`, implementations in `octofhir-auth-postgres`
- Axum extractors for authentication: `BearerAuth`, `OptionalBearerAuth`, `AdminAuth`
- Policy evaluation uses `PolicyEvaluator` with QuickJS script engine
- FHIR responses always use `application/fhir+json` content type

## Testing the Server

Default admin credentials (configured in `octofhir.toml`): `admin` / `admin123`. Get a token:

```bash
curl -X POST http://localhost:8888/oauth/token \
  -d "grant_type=password&username=admin&password=admin123"
```

### Testing with Authorization (Bun Scripts)

For testing authenticated endpoints, create JS/TS scripts in the `scripts/` directory and run them with bun:

```bash
bun run scripts/test-crud.ts      # Run CRUD tests
bun run scripts/test-search.ts    # Run search tests
```

Benefits of using bun scripts over curl:

- Easier token management (obtain and reuse tokens automatically)
- Complex test scenarios with assertions
- Better JSON handling and response validation
- Reusable helper functions for common operations

Example script structure:

```typescript
const BASE_URL = "http://localhost:8888";

// Get token
const tokenRes = await fetch(`${BASE_URL}/oauth/token`, {
  method: "POST",
  headers: { "Content-Type": "application/x-www-form-urlencoded" },
  body: "grant_type=password&username=admin&password=admin123",
});
const { access_token } = await tokenRes.json();

// Make authenticated request
const res = await fetch(`${BASE_URL}/fhir/Patient`, {
  headers: { Authorization: `Bearer ${access_token}` },
});
console.log(await res.json());
```

## Design System Rules

Direction: dense, keyboard-first console (Terminal density) on a tinted panel sidebar, one Marigold
accent. Reference page: `/ui/_styleguide?dir=hybrid` (dev only). Source of truth: `packages/ui-kit`.

### 1. Tokens & theming

- All colours, radii, spacing and type come from `--octo-*` CSS variables generated from
  `packages/ui-kit/src/shared/theme/tokens.ts`. No hardcoded hex, no Tailwind, no Gravity/Mantine vars.
- Light and dark are equal citizens; every token is scheme-aware. Check both themes.
- Monaco editors use the shared themes in `ui/src/shared/monaco/theme.ts` (keep in sync with tokens).

### 2. Accent (Marigold) — one accent only

- `--octo-accent-primary` is a **fill** (buttons, selected marker, slashes); text on it uses
  `--octo-accent-on-primary` (dark). Accent-coloured **text** uses `--octo-accent-primary-text`.
- Status colours are semantic only: success green, warning orange, error crimson, info blue.
- Badges/buttons are neutral unless a colour is given; use `color="primary"` deliberately.
- Keep the brand distinct: no red–orange brand accents, and never add per-page hues.

### 3. Layout & geometry

- Controls and table rows are 28px (`md`); radii 3/5/6px (sm/md/lg); 1px `--octo-border-subtle`.
- Page titles are mono 18px via `WorkspacePageLayout`; the location bar (`fhir / Patient / id`)
  already shows where you are, so no breadcrumbs or eyebrows.
- No gradients, glows, glassmorphism or coloured side stripes; selection = flat fill.
- FHIR addresses render as mono path chips (`ReferenceLink`: `Type/id`).

### 4. Keyboard

- ⌘K opens the global command palette everywhere; pages add commands with
  `usePaletteContribution` (Effector store in `ui/src/shared/command-palette`).

