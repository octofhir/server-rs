# =============================================================================
# OctoFHIR Server Dockerfile
# Multi-stage build for minimal production image
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Build UI (Node.js)
# -----------------------------------------------------------------------------
FROM node:22-slim AS ui-builder

WORKDIR /app

# Install pnpm via npm
RUN npm install -g pnpm

# Copy package files first for layer caching
COPY ui/package.json ui/pnpm-lock.yaml ./

# Install dependencies
RUN pnpm install --frozen-lockfile

# Copy UI source
COPY ui/ ./

# Build production bundle
RUN pnpm build

# -----------------------------------------------------------------------------
# Stage 2: Build Rust binary
# -----------------------------------------------------------------------------
FROM rust:1.92-slim-bookworm AS rust-builder

# Install build dependencies including JavaScriptCore for bot automation
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    git \
    # JSC dependencies for bot automation
    libjavascriptcoregtk-4.1-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy Cargo files first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Copy internal IGs (required for include_str! in bootstrap.rs)
COPY igs/ ./igs/

# Copy built UI
COPY --from=ui-builder /app/dist ./ui/dist

# Build release binary
RUN CARGO_PROFILE_RELEASE_LTO=thin \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
    CARGO_PROFILE_RELEASE_DEBUG=0 \
    cargo build --release --bin octofhir-server

# -----------------------------------------------------------------------------
# Stage 3: Runtime image (minimal)
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies including JavaScriptCore for bot automation
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    wget \
    # JSC runtime for bot automation
    libjavascriptcoregtk-4.1-0 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r octofhir && useradd -r -g octofhir octofhir

# Create directories
RUN mkdir -p /opt/octofhir/config /opt/octofhir/data/.fhir \
    && chown -R octofhir:octofhir /opt/octofhir

WORKDIR /opt/octofhir

# Copy binary
COPY --from=rust-builder /build/target/release/octofhir-server /usr/local/bin/

# Create default configuration
RUN cat > /opt/octofhir/config/octofhir.toml << 'EOF'
# OctoFHIR Server Docker Configuration
# Override settings via environment variables with OCTOFHIR__ prefix
# Example: OCTOFHIR__SERVER__PORT=9000

[server]
host = "0.0.0.0"
port = 8888
read_timeout_ms = 30000
write_timeout_ms = 30000
body_limit_bytes = 10485760

[storage.postgres]
host = "postgres"
port = 5432
user = "postgres"
password = "postgres"
database = "octofhir"
pool_size = 20
connect_timeout_ms = 10000
idle_timeout_ms = 60000

[fhir]
version = "R4"

[validation]
allow_skip_validation = false

[search]
default_count = 20
max_count = 500

[packages]
path = "/opt/octofhir/data/.fhir"
load = ["hl7.fhir.r4.core#4.0.1"]

[logging]
level = "info"

[otel]
enabled = false
endpoint = ""
sample_ratio = 0.0

[auth]
issuer = "http://localhost:8888"

[auth.oauth]
authorization_code_lifetime = "10m"
access_token_lifetime = "1h"
refresh_token_lifetime = "90d"
refresh_token_rotation = true
grant_types = ["authorization_code", "client_credentials", "refresh_token", "password"]

[auth.smart]
launch_ehr_enabled = true
launch_standalone_enabled = true
public_clients_allowed = true
confidential_symmetric_allowed = true
confidential_asymmetric_allowed = true
refresh_tokens_enabled = true
openid_enabled = true
dynamic_registration_enabled = false
supported_scopes = [
    "openid", "fhirUser", "launch", "launch/patient", "launch/encounter",
    "offline_access", "online_access", "patient/*.cruds", "user/*.cruds", "system/*.cruds"
]

[auth.signing]
algorithm = "RS384"
key_rotation_days = 90
keys_to_keep = 3

[auth.policy]
default_deny = true
quickjs_enabled = true

[auth.policy.quickjs]
memory_limit_mb = 16
max_stack_size_kb = 256
timeout_ms = 100

[auth.federation]
allow_external_idp = true
auto_provision_users = false
jwks_cache_ttl = "1h"
jwks_refresh_on_failure = true

[auth.rate_limiting]
token_requests_per_minute = 60
token_requests_per_hour = 1000
auth_requests_per_minute = 30
max_failed_attempts = 5
lockout_duration = "5m"

[redis]
enabled = false
url = "redis://redis:6379"
pool_size = 10
timeout_ms = 5000

[cache]
terminology_ttl_secs = 3600
local_cache_max_entries = 10000

[db_console]
enabled = true
sql_mode = "readonly"
lsp_enabled = true

[graphql]
enabled = true
introspection = false
max_depth = 15
max_complexity = 500

[sql_on_fhir]
enabled = true

[audit]
enabled = false

[bootstrap.admin_user]
username = "admin"
password = "admin"
email = "admin@octofhir.local"
EOF

RUN chown -R octofhir:octofhir /opt/octofhir

USER octofhir

ENV OCTOFHIR_CONFIG=/opt/octofhir/config/octofhir.toml
ENV RUST_LOG=info

EXPOSE 8888

HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8888/healthz || exit 1

CMD ["octofhir-server"]

LABEL org.opencontainers.image.source="https://github.com/octofhir/server-rs"
LABEL org.opencontainers.image.description="OctoFHIR - High-performance FHIR server"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"
