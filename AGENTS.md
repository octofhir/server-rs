# Repository Guidelines

## Project Structure & Modules
- Workspace: Rust with multiple crates under `crates/`.
- `crates/octofhir-server`: HTTP binary (`octofhir-server`), config/watch, routes, observability.
- `crates/octofhir-core`: core FHIR types, errors, utilities.
- `crates/octofhir-db`: storage abstraction and in-memory backend.
- `crates/octofhir-search`: search parsing and engine.
- `crates/octofhir-api`: shared API surface.
- Config: `octofhir.toml` (override via `OCTOFHIR_CONFIG`).
- Tests: integration tests in `crates/*/tests`.

## Build, Test, Develop
- Build: `just build` (or `cargo build`).
- Run: `just run` (uses `OCTOFHIR_CONFIG`, `RUST_LOG`, OTEL envs) or `cargo run --bin octofhir-server`.
- Dev (auto-reload): `just dev` (requires `cargo-watch`; install via `just install-tools`).
- Test: `just test` or `cargo test --all --all-features`.
- Lint: `just lint` → `cargo clippy -- -D warnings`.
- Format: `just fmt` → `cargo fmt --all`.
- Docs: `just doc`.
- OTEL sandbox: `just otel-up` / `just otel-down`.

## Coding Style & Conventions
- Formatting: 4 spaces, max width 100, Unix newlines; imports/modules reordered (see `.rustfmt.toml`).
- Lints: fix all Clippy warnings; do not ignore without justification.
- Naming: crates `octofhir-*`; modules/functions `snake_case`; types/enums `CamelCase`; constants `SCREAMING_SNAKE_CASE`.
- Errors: prefer `thiserror`; return `anyhow::Result` where appropriate.

## Testing Guidelines
- Frameworks: `tokio::test` for async, integration tests in `crates/octofhir-server/tests` (e.g., `server_endpoints.rs`).
- Conventions: name files by behavior (e.g., `config_parsing.rs`); assert HTTP JSON using `reqwest` and `serde_json`.
- Run selective tests: `cargo test -p octofhir-server <name>`.
- Coverage (optional): `just coverage` (requires `cargo-tarpaulin`).

## Commits & Pull Requests
- Commits: follow Conventional Commits (`feat:`, `fix:`, `chore:`). Use imperative mood and scope when useful.
- PRs: include summary, rationale, linked issues, and testing notes. Add config or API examples when applicable. Keep PRs focused and small.

## Security & Configuration
- Sensitive config via env vars; example overrides: `OCTOFHIR__SEARCH__DEFAULT_COUNT=9`.
- Local deps: workspace references `../canonical-manager` and `../fhir-model-rs`; ensure they exist when working on related features.
- Observability: configure OTLP via `OTEL_EXPORTER_OTLP_ENDPOINT` and toggle in `octofhir.toml`.

