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

See `config/server.toml` for configuration options.

## Documentation

- [Implementation Plan](./IMPLEMENTATION_PLAN.md)
- [Microtasks Breakdown](./MICROTASKS.md)

## Development

This project uses a microtask-based development approach. See `MICROTASKS.md` for detailed task breakdown and progress tracking.