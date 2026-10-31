<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="ui/public/branding/logo-dark.svg">
    <img src="ui/public/branding/logo-light.svg" alt="OctoFHIR" width="320">
  </picture>
</p>

<p align="center">
  A fast FHIR server in Rust (R4, R4B, R5, R6) with a keyboard-first web console.
</p>

> **Note:** OctoFHIR is under active development and not yet production-ready.

## Web console

The server ships its console at `/ui`: a resource browser, REST, SQL, GraphQL, FHIRPath
and CQL consoles, notebooks, ViewDefinitions, and access management. Press `⌘K` to jump
anywhere and `?` for keyboard shortcuts.

```bash
cd ui && pnpm install && pnpm dev   # dev server on http://localhost:4000/ui/
```

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

## Conformance

Results of the [FHIR262](https://github.com/HealthSamurai/fhir262) conformance suite
(suite commit `5f53746`, FHIR R4, 1572 tests):

| Server | CRUD | Search | Validation | Total | Run |
|---|---:|---:|---:|---:|---|
| **OctoFHIR** | 8/8 | 1258/1278 | 286/286 | **1552/1572 (98.7%)** | 2026-10-30, local |
| WSO2 FHIR Server | 7/8 | 1206/1278 | 284/286 | 1497/1572 (95.2%) | 2026-09-14, WSO2² |
| Aidbox | 6/8 | 1147/1278 | 259/286 | 1412/1572 (89.8%) | 2026-05-25, FHIR262 |
| HAPI | 8/8 | 1124/1278 | 256/286 | 1388/1572 (88.3%) | 2026-05-25, FHIR262 |
| Microsoft FHIR Server | 8/8 | 992/1278 | 225/286 | 1225/1572 (77.9%) | 2026-05-25, FHIR262 |
| fhir-candle | 6/8 | 1175/1278 | 26/286 | 1207/1572 (76.8%) | 2026-05-25, FHIR262 |
| Medplum¹ | 8/8 | 31/1278 | 230/286 | 269/1572 (17.1%) | 2026-05-25, FHIR262 |

All runs use the same 1572 tests. "FHIR262" is the suite's own published run;
"WSO2" is the run published in the WSO2 FHIR Server docs.

¹ Medplum's search result most likely reflects an environment failure in that run
rather than its actual search support.

² WSO2 runs the suite from a fork (`prdai-archive/fhir262`) whose harness boots one
server for all test files; the tests themselves are unchanged.

Known failures, all in `tests/Search/intervals.ts` on Periods that touch or straddle
the requested range: 14 `ap` cases (FHIR leaves the `ap` window to the server) and 6
`le` cases whose expectations contradict the suite's own "overlaps request end" case.
OctoFHIR follows the spec's `ge` = `gt` or `eq`, `le` = `lt` or `eq`.

To reproduce, check out FHIR262 next to this repo, add the `impl/octofhir` adapter,
tag the server image as `octofhir-server-local:dev` and run `make test-octofhir`.

## US Core with Inferno

```bash
just inferno-setup       # Clone Inferno (once)
just inferno-server-up   # Start OctoFHIR in Docker
just inferno-up          # Start Inferno
just inferno-seed        # Seed 43 US Core resources
just inferno-ui          # Open http://localhost
# Run the tests in the Inferno UI, then:
just inferno-results     # Generate conformance-results/<timestamp>/
```
