# OctoFHIR Server

A high-performance FHIR R4B server implementation in Rust with in-memory storage.

## Features

- ✅ **JSON-only FHIR API** (no XML support)
- ✅ **papaya 0.2 lock-free storage** for high concurrency
- ✅ **OpenTelemetry integration** with OTLP exporter
- ✅ **Live configuration reload** with file watching
- ✅ **Canonical package management** integration
- ✅ **Comprehensive search parameters**

## Quick Start

```bash
# Build the project
just build

# Run with default configuration
just run

# Run in development mode with auto-reload
just dev

# Run tests
just test
```

## Configuration

See `octofhir.toml` for configuration options. Override the path via `OCTOFHIR_CONFIG`. Print an example with `just example-config`.

## Documentation

- Live docs (GitHub Pages): https://octofhir.github.io/server-rs/
- Local docs (Astro + Starlight via pnpm):
  - `just docs-dev` to run dev server in `docs/`
  - `just docs-build` to build static site to `docs/dist`
  - Requires `pnpm` (install from https://pnpm.io/installation)
  
- Additional:
  - [Implementation Plan](./IMPLEMENTATION_PLAN.md)
  - [Remaining Tasks](./remaining_tasks.md)

## Development

This project uses a microtask-based development approach. See `tasks/` for detailed task breakdown and progress tracking.
