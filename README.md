# OctoFHIR Server

> **Note:** This project is under active development and not yet production-ready.

A high-performance FHIR R4B server in Rust.

## Quick Start

```bash
cargo run                    # Run server
cargo test                   # Run tests
cargo build --release        # Build release binary
```

## Development

```bash
cargo watch -x run           # Auto-reload on changes
cargo clippy                 # Lint
cargo fmt                    # Format code
cargo doc --open             # Generate and open docs
```

## Configuration

Configuration via `octofhir.toml`. Override path with `OCTOFHIR_CONFIG` env var.
