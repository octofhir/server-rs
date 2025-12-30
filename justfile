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
# k6 Load Testing
# =============================================================================


flame:
    CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --release --bin octofhir-server -o ./flamegraph.svg --post-process 'head -200000'

# k6 variables
K6_BASE_URL := "http://localhost:8888/fhir"
K6_AUTH_USER := "admin"
K6_AUTH_PASSWORD := "admin123"
K6_CLIENT_ID := "k6-test"

# Create k6-test OAuth client and AccessPolicy (run once, saves secret to .k6-secret)
k6-setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Getting admin token..."
    TOKEN=$(curl -s -X POST 'http://localhost:8888/auth/token' \
      -H 'Content-Type: application/x-www-form-urlencoded' \
      -d 'grant_type=password&username={{K6_AUTH_USER}}&password={{K6_AUTH_PASSWORD}}&client_id=octofhir-ui' \
      | jq -r '.access_token')

    echo "Checking if k6-test client exists..."
    EXISTS=$(curl -s "http://localhost:8888/Client?clientId=k6-test" \
      -H "Authorization: Bearer $TOKEN" | jq '.total')

    if [ "$EXISTS" -gt "0" ]; then
      echo "Client k6-test already exists, regenerating secret..."
    else
      echo "Creating k6-test client..."
      curl -s -X POST 'http://localhost:8888/Client' \
        -H "Authorization: Bearer $TOKEN" \
        -H 'Content-Type: application/json' \
        -d '{"resourceType":"Client","clientId":"k6-test","name":"K6 Load Testing Client","confidential":true,"active":true,"grantTypes":["password","client_credentials"],"scopes":["openid","user/*.cruds","system/*.cruds"]}'
      echo ""
    fi

    echo "Generating client secret..."
    SECRET=$(curl -s -X POST 'http://localhost:8888/admin/clients/k6-test/regenerate-secret' \
      -H "Authorization: Bearer $TOKEN" \
      -H 'Content-Type: application/json' | jq -r '.clientSecret')

    echo "$SECRET" > .k6-secret
    echo "Client secret saved to .k6-secret"

    echo "Creating/updating AccessPolicy for k6-test..."
    curl -s -X PUT 'http://localhost:8888/AccessPolicy/00000000-0000-0000-0000-000000000002' \
      -H "Authorization: Bearer $TOKEN" \
      -H 'Content-Type: application/json' \
      -H 'X-Skip-Validation: true' \
      -d '{"resourceType":"AccessPolicy","id":"00000000-0000-0000-0000-000000000002","name":"K6 Test Full Access","description":"Allow all operations for admin users via k6-test client","active":true,"priority":1,"matcher":{"clients":["k6-test"],"roles":["admin"]},"engine":{"type":"allow"}}'
    echo ""

    echo "Done! You can now run: just k6-crud-test"

# Run k6 CRUD test (single iteration for validation)
k6-crud-test:
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run --iterations 1 --vus 1 k6-private/crud.js

# Run k6 search test (single iteration for validation)
k6-search-test:
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run --iterations 1 --vus 1 k6-private/search.js

# Run k6 CRUD load test (5 minutes, 300 VUs)
k6-crud-load:
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run k6-private/crud.js

# Run k6 search load test (5 minutes, 10 VUs)
k6-search-load:
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) \
    k6 run k6-private/search.js

# Run k6 import test (requires BUNDLE_URL)
k6-import bundle_url:
    AUTH_USER={{K6_AUTH_USER}} AUTH_PASSWORD={{K6_AUTH_PASSWORD}} \
    BASE_URL={{K6_BASE_URL}} CLIENT_ID={{K6_CLIENT_ID}} \
    CLIENT_SECRET=$(cat .k6-secret) BUNDLE_URL={{bundle_url}} \
    k6 run k6-private/import.js

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
