# OctoFHIR Development Commands
# Run `just --list` to see all available commands

# Variables (override via environment)
set export := true
CONFIG := "octofhir.toml"
RUST_LOG := "info"
OTEL_EXPORTER_OTLP_ENDPOINT := "http://localhost:4318"

# Default recipe - show available commands
default:
    @just --list

# Build all crates in workspace
build:
    RUST_LOG={{RUST_LOG}} cargo build

# Run server with config file
run:
    OCTOFHIR_CONFIG={{CONFIG}} \
    RUST_LOG={{RUST_LOG}} \
    OTEL_EXPORTER_OTLP_ENDPOINT={{OTEL_EXPORTER_OTLP_ENDPOINT}} \
    cd ui && pnpm run build && cd .. && cargo run --bin octofhir-server

# Developer mode: auto-rebuild and run on changes (requires cargo-watch)
dev:
    OCTOFHIR_CONFIG={{CONFIG}} \
    RUST_LOG={{RUST_LOG}} \
    OTEL_EXPORTER_OTLP_ENDPOINT={{OTEL_EXPORTER_OTLP_ENDPOINT}} \
    cargo watch -x 'run --bin octofhir-server'

# Run all tests
test:
    RUST_LOG={{RUST_LOG}} cargo test --all --all-features

# Format code
fmt:
    cargo fmt --all

# Lint (clippy)  
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Check (fast type check)
check:
    cargo check --all-targets --all-features

# Build docs
doc:
    cargo doc --no-deps --all-features

# Docs: dev and build (Astro + Starlight)
docs-dev:
    cd docs && pnpm install && pnpm dev

docs-build:
    cd docs && pnpm install && pnpm build

# Clean build artifacts
clean:
    cargo clean

# Install development tools
install-tools:
    cargo install cargo-watch

# Print example server config
example-config:
    @cat octofhir.toml

# Start local OTEL collector (requires Docker)
otel-up:
    docker run --rm -d -p 4318:4318 -p 16686:16686 --name otelcol jaegertracing/all-in-one:latest

# Stop local OTEL collector
otel-down:
    docker rm -f otelcol || true

# Run benchmarks (when implemented)
bench:
    cargo bench

# Generate coverage report (requires cargo-tarpaulin)
coverage:
    cargo tarpaulin --out html --output-dir coverage

# Update dependencies
update:
    cargo update

# Audit dependencies for security vulnerabilities
audit:
    cargo audit
