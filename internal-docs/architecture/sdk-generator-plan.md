# SDK Generator Plan

## Decisions

- XML support is out of scope and intentionally not planned.
- SDK generation is a better competitive investment than XML parity.
- The generator should live in this repository as a workspace binary, but reuse `inkgen` as a library from the parent directory.
- First target language: TypeScript.

## Why This Matters

Today the server has strong protocol and platform features, but the developer onboarding story is still mostly HTTP/OpenAPI/manual integration. A built-in SDK generator would improve:

- adoption for app developers
- consistency of client integrations
- stickiness for profile-heavy implementations
- differentiation versus servers that expose APIs but do not own the typed client workflow

This is especially useful because the repo already has:

- OpenAPI description in [docs/api/openapi.yaml](/Users/alexanderstreltsov/work/octofhir/server-rs/docs/api/openapi.yaml)
- custom resources and IG loading
- GraphQL, SQL on FHIR, CQL, auth, subscriptions

So a generated SDK can cover not only generic FHIR CRUD, but also OctoFHIR-specific capabilities.

## Current Constraints

### In This Repo

- There is no SDK generation crate yet.
- There is already a CLI crate: [crates/octofhir-cli](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-cli)
- The repo already publishes an OpenAPI surface, but that alone is not enough for profile-aware SDK generation.

### In Parent Directory

`inkgen` exists at `/Users/alexanderstreltsov/work/octofhir/inkgen` and is already split into reusable crates:

- `inkgen-core`
- `inkgen-typescript`
- `inkgen-rust`
- `inkgen-cli`

Relevant files:

- [../inkgen/Cargo.toml](/Users/alexanderstreltsov/work/octofhir/inkgen/Cargo.toml)
- [../inkgen/crates/inkgen-core/src/lib.rs](/Users/alexanderstreltsov/work/octofhir/inkgen/crates/inkgen-core/src/lib.rs)
- [../inkgen/crates/inkgen-typescript/src/lib.rs](/Users/alexanderstreltsov/work/octofhir/inkgen/crates/inkgen-typescript/src/lib.rs)
- [../inkgen/crates/inkgen-cli/src/main.rs](/Users/alexanderstreltsov/work/octofhir/inkgen/crates/inkgen-cli/src/main.rs)

That means library integration is realistic and preferable to shelling out into another CLI.

## Recommended Architecture

## Option A

Create a new crate in this workspace:

- `crates/octofhir-sdkgen`

With:

- `[[bin]] octofhir-sdkgen`
- optional library module for reuse by admin UI jobs or future automation hooks

Dependencies:

- path dependency to `inkgen-core`
- path dependency to `inkgen-typescript`
- existing workspace crates for config, auth, canonical/package access as needed

Example dependency shape:

```toml
inkgen-core = { path = "../inkgen/crates/inkgen-core" }
inkgen-typescript = { path = "../inkgen/crates/inkgen-typescript" }
```

This is the cleanest option because it:

- keeps concerns isolated from `octofhir-cli`
- avoids bloating operational CLI with build/generation concerns
- gives room for future generators and packaging logic

## Option B

Add `sdk generate` subcommands to [crates/octofhir-cli](/Users/alexanderstreltsov/work/octofhir/server-rs/crates/octofhir-cli).

This is acceptable only if the intended UX is strictly local developer tooling and you do not expect:

- generator-specific config growth
- packaging/publishing workflows
- multiple backends
- admin/API-triggered generation later

My recommendation is Option A first, then optionally expose it through `octofhir-cli` as a wrapper.

## Product Shape

The first usable deliverable should not be a generic OpenAPI client. It should be an OctoFHIR-aware SDK generator with two modes.

### Mode 1: FHIR/Profile SDK

Source:

- canonical packages already used by the server
- loaded IGs
- StructureDefinitions
- ValueSets

Output:

- TypeScript resources
- profile-aware types/classes
- ValueSet helpers
- search builders
- runtime validation helpers where useful

This is where `inkgen` is strongest today.

### Mode 2: Service Client Layer

Source:

- [docs/api/openapi.yaml](/Users/alexanderstreltsov/work/octofhir/server-rs/docs/api/openapi.yaml)

Output:

- thin typed client for server-specific endpoints:
- auth
- admin API
- GraphQL helper client
- async job polling
- bulk/export/import/reindex helpers

This layer may be generated separately from OpenAPI or implemented as handwritten templates on top of generated FHIR/profile models.

## Recommended MVP

Phase 1 should generate only TypeScript artifacts for canonical/profile models and a thin handwritten OctoFHIR transport.

Deliverables:

- `octofhir-sdkgen generate typescript`
- input from local config or package list
- output directory selection
- package/profile filtering
- generated `README.md` in output
- generated `package.json` and `tsconfig.json`
- optional auth-aware fetch client wrapper for OctoFHIR endpoints

MVP should explicitly exclude:

- multi-language support
- registry publishing
- UI integration
- remote generation service
- per-tenant SDK generation

## Implementation Plan

## Phase 0: Validation Spike

Goal: prove that `server-rs` can call `inkgen` crates directly.

Tasks:

- add a temporary local experiment crate or branch-only prototype
- wire path dependencies to `../inkgen/crates/inkgen-core` and `../inkgen/crates/inkgen-typescript`
- run a minimal generation flow against one package
- verify no major version or workspace conflicts

Success criteria:

- one command generates TS output from a known package
- no need to shell out to `inkgen-cli`

## Phase 1: New Binary Crate

Goal: add a stable workspace binary.

Tasks:

- create `crates/octofhir-sdkgen`
- add CLI with subcommands:
- `generate typescript`
- `doctor`
- `init`
- define a local config file format, preferably `octofhir-sdkgen.toml`
- support output dir override
- support package selection

Success criteria:

- binary builds in workspace
- one-shot TS generation works from repo root

## Phase 2: Server-Aware Inputs

Goal: make generation reflect the actual OctoFHIR deployment model.

Tasks:

- reuse package list semantics from server config where possible
- support loading packages from current environment
- support generating against loaded IG/custom resources
- optionally inspect CapabilityStatement/OpenAPI for feature toggles

Success criteria:

- generated SDK matches the actual enabled package surface
- custom resource/profile deployments are represented in output

## Phase 3: OctoFHIR Transport Layer

Goal: add a thin typed client around server-specific behavior.

Tasks:

- generate or template wrappers for:
- auth token acquisition
- CRUD/search convenience
- async job polling
- bulk export/import/reindex helpers
- GraphQL convenience client
- normalize error handling to OperationOutcome-aware exceptions/results

Success criteria:

- TS consumers can integrate with OctoFHIR without hand-writing transport glue

## Phase 4: Distribution

Goal: make SDK output consumable in real projects.

Tasks:

- generate package metadata
- support monorepo and standalone output modes
- add version stamping tied to server build or package manifest
- define compatibility contract:
- server version
- FHIR version
- package set hash

Success criteria:

- generated SDK can be installed and versioned predictably

## Suggested CLI Shape

```bash
cargo run -p octofhir-sdkgen -- generate typescript \
  --output ./generated/sdk \
  --package hl7.fhir.r4.core#4.0.1
```

Future commands:

```bash
octofhir-sdkgen init
octofhir-sdkgen doctor
octofhir-sdkgen generate typescript --from-server-config ./octofhir.toml
octofhir-sdkgen generate typescript --from-capabilities http://localhost:8888/fhir/metadata
```

## Configuration Proposal

Suggested file: `octofhir-sdkgen.toml`

```toml
[input]
source = "server-config"
server_config = "./octofhir.toml"

[output]
dir = "./generated/sdk"
package_name = "@octofhir/sdk"

[typescript]
mode = "class"
zod_schemas = true
generate_profiles = true
generate_valuesets = true

[octofhir]
include_admin_api = false
include_graphql_helpers = true
include_bulk_helpers = true
```

## Main Risks

- path dependency on a sibling repo is fine for local development but weak for CI/reproducibility
- `inkgen` API is still evolving, so direct library coupling may create churn
- generated output can drift from actual server behavior if based only on OpenAPI and not on loaded packages/config
- if we merge SDK generation into `octofhir-cli`, that crate may become too broad

## Risk Mitigations

- start with path dependency for speed, then decide whether to:
- vendor selected `inkgen` crates
- publish `inkgen-*` crates
- use git dependencies pinned by revision
- wrap `inkgen` calls behind a thin local adapter crate so API churn is isolated
- stamp generated SDK with package and config metadata

## Competitive Value

This feature is stronger than XML support for current adoption.

It helps compete on:

- developer experience versus HAPI
- product platform feel versus Aidbox
- application integration velocity versus managed cloud FHIR APIs

Most servers stop at API exposure. A good SDK generator turns OctoFHIR into an application platform.

## Recommended Priority

Priority order:

1. Fix search correctness issues first.
2. Start Phase 0 spike for `inkgen` library embedding.
3. Build `crates/octofhir-sdkgen` as isolated binary.
4. Add OctoFHIR-specific transport layer after TS model generation is stable.

## Concrete Next Steps

1. Create `crates/octofhir-sdkgen`.
2. Add path dependencies to sibling `inkgen-core` and `inkgen-typescript`.
3. Port the smallest working TypeScript generation flow from `inkgen-cli`.
4. Define a local config file for generation.
5. Generate one SDK from the same package set used by local OctoFHIR.
6. Add snapshot/integration tests for deterministic output.
