# Storage providers and DICOM release

## Scope

Replace the unreleased inline package-S3 settings with reusable database-backed
StorageProvider resources and console administration. Keep image/DICOM objects in
PACS. General Binary/Attachment S3 storage remains a separate deferred draft.

## Delivery

- [x] Owned provider DTO, environment or encrypted stored credentials, explicit
      path/virtual-hosted addressing, common S3/SigV4 transport with guarded egress.
- [x] PostgreSQL migration before IG acquisition, create-only TOML seeds,
      administrative API, optimistic versions and redacted transactional history.
- [x] IG provider/bucket/key/checksum references; remove unreleased inline settings.
- [x] Console list/create/edit/disable/delete and write-only credential rotation.
- [x] Provider, configuration and multi-provider documentation; update local RustFS guide.
- [x] Finish provider API/encryption/restart and real S3/RuCore acceptance checks.
- [x] Complete DICOM release acceptance within the implemented Requestor/Watcher
      and constrained authenticated retrieval scope; do not imply full PACS/IHE/viewer conformance.
- [x] Rust formatting/lint/tests, UI checks/browser verification and documentation build.
- [ ] Verify Docker runtime and release automation; native Linux/macOS binaries,
      embedded console, checksums and amd64/arm64 Docker images.
- [ ] Review and commit task-owned changes, publish v0.0.5, verify downloadable
      assets and versioned container images. Preserve unrelated README, devcontainer
      and report changes.

## Lifecycle and publication

PostgreSQL migrations -> insert missing seed providers -> resolve database provider
snapshot -> acquire pinned package -> initialize clinical models/application routes.
Database edits win over existing TOML seeds. Credential fields never serialize;
key material is encrypted separately using OCTOFHIR_CONFIG_KEY. Each acquisition
uses a snapshot; later acquisitions reread settings. Verified cache hits require
no credentials. Current package registry references protect configured providers
from deletion and follow package configuration reloads.

v0.0.3 is the preceding source-only GitHub release. v0.0.5 adds downloadable
binaries and a reproducible release workflow. Prepare, native binary and native
container jobs and main CI for the exact source commit must pass before
publication. CI and native builds may run concurrently; publication waits for both. Explicit DICOM/backend support limits
are recorded in the guides and release notes.

## Local acceptance record

- Server: 586 library tests; configuration, provider lifecycle/encryption/no-key,
  real RustFS package acquisition/cache, RuCore, import/sync/gateway/UPS integration
  checks passed. CLI help/version/config-precedence tests passed.
- Real isolated Orthanc and dcm4chee retrieval passed; dcm4chee UPS creation,
  performer transition, cancellation acknowledgement and output publication passed.
- Workspace strict Clippy and formatting passed. UI typecheck/lint, 168 tests,
  production build, browser create/persistence and 40-page docs build passed.
- Release workflow passes actionlint. Publication requires the same commit's main
  CI plus all four native binaries and both native Docker builds with smoke checks.

The v0.0.4 candidate was stopped by full CI before publication. Its tag remains
an audit record; v0.0.5 includes the SQL IG selection and bootstrap stack fixes.

Corrected bootstrap acceptance: all four previously failing SQL test binaries,
federated OIDC/SAML sign-in on 2 MiB stacks and RuCore passed locally.

## Portable standard API acceptance

- [x] Publish the server-neutral FHIR/DICOMweb client and reproducible two-server
      scenario; preserve built-in custom operations.
- [x] Verify the same standard R4 transaction/search/Task/Endpoint/WADO workflow
      against OctoFHIR and digest-pinned HAPI with request validation enabled.

Standard API acceptance passed on OctoFHIR 0.0.5 and HAPI FHIR R4 with strict
request validation, using the same unmodified client and synthetic Orthanc frame.
