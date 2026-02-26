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
    pnpm install && pnpm -C ui build && cargo run --bin octofhir-server

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

# Start PostgreSQL database
db-up:
    docker compose up -d

# Stop PostgreSQL database
db-down:
    docker compose down

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

# =============================================================================
# CLI Tool
# =============================================================================

# Build CLI tool
cli-build:
    cargo build -p octofhir-cli

# Run CLI tool with arguments
cli *ARGS:
    cargo run -p octofhir-cli -- {{ARGS}}

# Install CLI locally
cli-install:
    cargo install --path crates/octofhir-cli

# Test CLI against local OctoFHIR server (server must be running)
cli-test-local:
    @echo "Testing CLI against local OctoFHIR server..."
    cargo run -p octofhir-cli -- --server http://localhost:8888 status
    cargo run -p octofhir-cli -- --server http://localhost:8888 login --username admin --password admin123
    cargo run -p octofhir-cli -- --server http://localhost:8888 whoami
    cargo run -p octofhir-cli -- --server http://localhost:8888 metadata --format table
    cargo run -p octofhir-cli -- --server http://localhost:8888 search Patient --format table

# Start Aidbox for CLI testing
aidbox-up:
    docker compose -f docker-compose.aidbox.yml up -d

# Stop Aidbox
aidbox-down:
    docker compose -f docker-compose.aidbox.yml down

# Test CLI against Aidbox (Aidbox must be running)
cli-test-aidbox:
    @echo "Testing CLI against Aidbox..."
    cargo run -p octofhir-cli -- --server http://localhost:8080 status
    cargo run -p octofhir-cli -- --server http://localhost:8080 metadata --format table

# =============================================================================
# k6 Load Testing
# =============================================================================

flame:
    CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --release --bin octofhir-server -o ./flamegraph.svg --post-process 'head -200000'

# Build release binary with debug symbols (bench profile)
profile-build:
    cargo build --profile bench --bin octofhir-server

# Run server with macOS sample profiler (120s capture). Run k6 benchmarks in another terminal.
profile:
    @echo "Starting server under profiler (120s capture)..."
    @echo "Run 'just bench-crud' in another terminal to generate load"
    target/release/octofhir-server &
    @sleep 5
    sample octofhir-server 120 -f profile_output.txt
    @echo "Profile saved to profile_output.txt"

# k6 variables
K6_BASE_URL := "http://localhost:8888/fhir"
K6_AUTH_USER := "admin"
K6_AUTH_PASSWORD := "admin123"
K6_CLIENT_ID := "k6-test"

# Create k6-test OAuth client and AccessPolicy (run once, saves secret to .k6-secret)
k6-setup:
   bun run scripts/k6-setup.ts

# =============================================================================
# Performance Benchmarks (k6/)
# =============================================================================

# Run all benchmarks
bench-all: bench-crud bench-search bench-transaction bench-concurrent

# Run CRUD benchmark (3 min, 100 VUs)
bench-crud:
    @mkdir -p benchmark-results
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run k6/benchmarks/crud.js

# Run search benchmark (6 min total: 3 min seed + 3 min search)
bench-search:
    @mkdir -p benchmark-results
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run k6/benchmarks/search.js

# Run transaction/batch benchmark (7 min, varying VUs)
bench-transaction:
    @mkdir -p benchmark-results
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run k6/benchmarks/transaction.js

# Run concurrent users benchmark (8 min, 10-300 VUs ramp)
bench-concurrent:
    @mkdir -p benchmark-results
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run k6/benchmarks/concurrent.js

# Run bulk export benchmark (15 min)
bench-bulk:
    @mkdir -p benchmark-results
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run k6/benchmarks/bulk.js

# Quick benchmark validation (single iteration, no thresholds)
bench-validate:
    @mkdir -p benchmark-results
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run --iterations 1 --vus 1 --no-thresholds k6/benchmarks/crud.js

# Verbose benchmark validation with HTTP debug
bench-validate-verbose:
    @mkdir -p benchmark-results
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run --iterations 1 --vus 1 --no-thresholds --http-debug=full k6/benchmarks/crud.js

# =============================================================================
# UI Commands
# =============================================================================

# Install all workspace dependencies
ui-install:
    pnpm install

# Build UI production bundle
ui-build:
    pnpm -C ui build

# Run UI dev server
ui-dev:
    pnpm -C ui dev

# Typecheck all packages
ui-typecheck:
    pnpm -r typecheck

# Lint all packages
ui-lint:
    pnpm -r lint

# Format all packages
ui-format:
    pnpm -r format

# =============================================================================
# Storybook Commands
# =============================================================================

# Run Storybook dev server
storybook:
    pnpm -C packages/ui-kit storybook

# Build Storybook static site
storybook-build:
    pnpm -C packages/ui-kit storybook:build

# =============================================================================
# Docker Commands (GitHub Container Registry)
# =============================================================================

# Docker image settings
DOCKER_REGISTRY := "ghcr.io"
DOCKER_IMAGE := "octofhir/octofhir-server"
DOCKER_TAG := `git describe --tags --always --dirty 2>/dev/null || echo "dev"`

# Build Docker image
docker-build:
    docker build -t {{DOCKER_REGISTRY}}/{{DOCKER_IMAGE}}:{{DOCKER_TAG}} \
                 -t {{DOCKER_REGISTRY}}/{{DOCKER_IMAGE}}:latest .

# Build Docker image (no cache)
docker-build-fresh:
    docker build --no-cache -t {{DOCKER_REGISTRY}}/{{DOCKER_IMAGE}}:{{DOCKER_TAG}} \
                            -t {{DOCKER_REGISTRY}}/{{DOCKER_IMAGE}}:latest .

# Push Docker image to GitHub Container Registry
docker-push:
    docker push {{DOCKER_REGISTRY}}/{{DOCKER_IMAGE}}:{{DOCKER_TAG}}
    docker push {{DOCKER_REGISTRY}}/{{DOCKER_IMAGE}}:latest

# Build and push Docker image
docker-release: docker-build docker-push

# Run Docker container locally (requires running postgres)
docker-run:
    docker run --rm -it \
        --network octofhir-network \
        -p 8888:8888 \
        -e OCTOFHIR__STORAGE__POSTGRES__HOST=octofhir-postgres \
        -e OCTOFHIR__STORAGE__POSTGRES__PORT=5432 \
        -e OCTOFHIR__BOOTSTRAP__ADMIN_USER__PASSWORD=admin123 \
        {{DOCKER_REGISTRY}}/{{DOCKER_IMAGE}}:latest

# Login to GitHub Container Registry (requires gh cli)
docker-login:
    gh auth token | docker login {{DOCKER_REGISTRY}} -u $(gh api user -q .login) --password-stdin

# =============================================================================
# Conformance Testing (Inferno)
# =============================================================================

# Setup Inferno US Core Test Kit (clone + configure, run once)
inferno-setup:
    bash scripts/inferno/setup.sh

# Start OctoFHIR in Docker for conformance testing
inferno-server-up:
    docker compose -f docker-compose.yml -f docker-compose.inferno.yml up -d --build

# Start Inferno (requires inferno-setup first)
inferno-up:
    cd inferno/us-core-test-kit && docker compose up -d

# Start all services (OctoFHIR + Inferno) with health checks
inferno-start:
    bash scripts/inferno/start.sh

# Stop OctoFHIR conformance server
inferno-server-down:
    docker compose -f docker-compose.yml -f docker-compose.inferno.yml down

# Stop Inferno
inferno-stop:
    cd inferno/us-core-test-kit && docker compose down

# Stop everything (OctoFHIR + Inferno)
inferno-down: inferno-server-down inferno-stop

# Seed test data for Inferno
inferno-seed:
    NODE_TLS_REJECT_UNAUTHORIZED=0 bun run scripts/inferno/seed-data.ts

# Run conformance tests (full workflow: health check + seed + instructions)
inferno-test:
    bun run scripts/inferno/run-tests.ts

# Collect and categorize test results
inferno-results *ARGS:
    bun run scripts/inferno/collect-results.ts {{ARGS}}

# Open Inferno UI in browser
inferno-ui:
    open http://localhost:4567
