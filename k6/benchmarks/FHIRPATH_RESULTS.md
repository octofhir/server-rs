# Local FHIRPath HTTP comparison

Date: 2026-09-09. Host: aarch64-apple-darwin. Rust: 1.97.1.
Baseline FHIRPath: fdf70795f76f872c2cc429ca9b16a712c4f6c7b0 (0.4.54).
Candidate FHIRPath: d82a67c (release version remains unchanged until workflow bump).

Both native server binaries use the same release profile, server configuration
and isolated PostgreSQL 18 Docker database. Builds finish before measurement.
No production database or existing Docker containers were modified.
The server workspace uses a local crates.io patch for ../fhirpath-rs.

## Final Docker results

Read-only POST /fhir/$fhirpath; Patient with 100 names; 8 concurrent k6 VUs,
15 seconds per case, 26 setup/warmup calls. Each response is checked for HTTP 200
and the expected numeric FHIRPath result. All checks passed; HTTP errors: zero.

| Case | Baseline req/s | Candidate req/s | Ratio | Baseline p99 ms | Candidate p99 ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| regex | 1032 | 5815 | 5.64x | 38.00 | 8.52 |
| where | 6839 | 7447 | 1.09x | 6.42 | 5.98 |
| distinct | 6692 | 7326 | 1.09x | 7.49 | 5.59 |

Cases are defined in fhirpath.js: regex filtering, equality filtering, and union
of name.family with itself followed by count(). The last case is not the
10,000-integer distinct microbenchmark; do not conflate their speedups.

These are short, warm, loopback closed-loop measurements on a shared development
machine, not production capacity estimates. Client work, HTTP/JSON and model
navigation are included. PostgreSQL is used for server setup/provider data;
these expressions do not benchmark FHIR storage writes or SQL throughput.
Only the paired Docker run is reported above; preliminary native-PostgreSQL
numbers are not mixed into the comparison.

## Reproduce / inspect

- Current binary: target/release/octofhir-server (final local FHIRPath build).
- Baseline binary: /private/tmp/fhirpath-release.HqtPXI/server-baseline.
- Candidate snapshot: /private/tmp/fhirpath-release.HqtPXI/server-after.
- Isolated config: /private/tmp/fhirpath-release.HqtPXI/octofhir-docker.toml.
- Raw summaries: /private/tmp/fhirpath-release.HqtPXI/http-docker-{baseline,after}-{regex,where,distinct}.json.
- Docker container: octofhir-fhirpath-release-pg, PostgreSQL 18, localhost:15440.
- HTTP benchmark port: localhost:18888.
- The benchmark server was stopped after testing. Docker PostgreSQL remains
  running for follow-up work; the earlier temporary native PostgreSQL was stopped.
- The isolated configuration permits anonymous local access: do not use it in production.

From the server workspace, start the binary with the isolated configuration:

```sh
RUST_LOG=warn target/release/octofhir-server \
  --config /private/tmp/fhirpath-release.HqtPXI/octofhir-docker.toml
# Separate terminal:
CASE=regex k6 run k6/benchmarks/fhirpath.js
CASE=where k6 run k6/benchmarks/fhirpath.js
CASE=distinct k6 run k6/benchmarks/fhirpath.js
```

Use the same database/config and stop one binary before starting the other.
Stop the test database when no longer needed with:
`docker stop octofhir-fhirpath-release-pg` (preserves its data).

## Library release checks

The FHIRPath commit passed 662 workspace tests (7 intentionally ignored),
1176/1176 compliance cases, strict all-targets/all-features Clippy,
documentation with warnings denied, and verified crate packaging.
The packaged crate's dependency audit reports zero vulnerabilities and warnings.
The full CLI/dev-tools workspace still has the existing lru 0.16.4 advisory
through canonical-manager; see ../fhirpath-rs/docs/EVALUATOR_PERFORMANCE.md for
the detailed plan checklist, microbenchmarks and scope boundaries.

The FHIRPath commit has no Co-authored-by trailer. It was subsequently pushed as
`d82a67c` at the user's request; no package was published. The coordinated local
follow-up is documented in [VALIDATION_RESULTS.md](VALIDATION_RESULTS.md).
Server Cargo.toml/Cargo.lock and benchmark artifacts are intentionally separate,
uncommitted local integration changes.
