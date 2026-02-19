# Abyxon

> **Note:** This project is under active development and not yet production-ready. Status: Accepted.

Core FHIR server of the OctoFHIR ecosystem, built in Rust.

![Abyxon logo](logo.png)

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
just inferno-setup       # Clone Inferno (once)
  just inferno-server-up   # Start OctoFHIR in Docker
  just inferno-up          # Start Inferno
  just inferno-seed        # Seed 43 US Core resources
  just inferno-ui          # Open <http://localhost>

# Run tests manually in Inferno UI, then

  just inferno-results     # Generate conformance-results/<timestamp>/
  5e15b462-6db3-4bee-b127-a95784220c1c,3876d18a-1bf1-469e-847b-95cf29bef486,9426bb81-c11c-4111-bf53-184190d0c3ec
http://localhost:4567/custom/smart/launch?iss=https://host.docker.internal:8443/fhir&launch=9K5pfV9W1CLiDNPI48Nkxn8909WGrFtGNX2WM-vr-cI