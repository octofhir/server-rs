# Prepared validation: local library integration

Measured 2026-09-09 on aarch64-apple-darwin. Both binaries use release builds and
the same PostgreSQL 18 Docker instance: `octofhir-fhirpath-release-pg`, port 15440.
HTTP listens on 127.0.0.1:18888. No concurrent compilation during measurements.

The candidate is built directly from the existing sibling repositories via the
server workspace's `[patch.crates-io]`: fhir-model-rs, fhirpath-rs and fhirschema.
No cloned repository is used by this build.

## HTTP results: prepared integration, before the VM follow-up

`validation.js`: 100 Patient.contact nodes, 8 VUs, 15 seconds per run.
Each contact has a name and two given names. The benchmark checks successful
validation on every response. Setup also submits an invalid contact and requires
an error mentioning `pat-1`, verifying that FHIRPath invariants actually run.
Requests are read-only `POST /fhir/Patient/$validate`; no FHIR resources are stored.

Run order: previous → candidate → candidate → previous. Metrics include setup
requests (two correctness checks and 25 warm-ups); the VU phase lasts 15 seconds.

| Pair | Previous RPS | Candidate RPS | Ratio | Previous p99 | Candidate p99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Forward | 1584.33 | 4829.16 | 3.05x | 18.31 ms | 8.67 ms |
| Reverse | 1594.73 | 4767.59 | 2.99x | 17.15 ms | 9.22 ms |

All four runs: zero HTTP failures, 100% checks passed. The candidate also passed
the separate HTTP FHIRPath regex smoke test. These results describe this
CPU-oriented validation workload, not general server capacity or workloads
dominated by reference/terminology I/O.

## Provenance and verification

- Previous: saved `server-after` from the earlier d82a67c evaluator optimization.
  SHA-256: `5ae64ea33b5efacb556375912e05541067732aa114e971fe42f3153185a84e5d`.
- Candidate at measurement time: `target/release/octofhir-server`, built with all
  three local libraries. Preserved before the subsequent VM rebuild at
  `/private/tmp/fhirpath-vm.1VHZqr/server-before-vm`. SHA-256:
  `7316d33c3c0dc10a45cbb3c6111667bdb69d9a5bca901fd566bbdc4d853c7037`.
- FHIRPath workspace all-features: 666 passed, 7 ignored. Compliance: 1176/1176.
- fhir-model all-features: 23 passed, 4 ignored.
- fhirschema library/integration/doc tests: 142 passed, 24 ignored.
- Strict Clippy passed for all three libraries (workspace all-features/all-targets
  for FHIRPath; all-features/all-targets for model; package all-targets for schema).
- Regression tests cover typed primitive roots, scope isolation, foreign handles,
  changed variables, legacy evaluators, duplicate diagnostics, malformed batch
  results, concurrent compilation and schema invalidation during in-flight work.

Raw logs and k6 JSON summaries:
`/private/tmp/octofhir-release2.3CgZAK/`.
The previous test configuration remains at
`/private/tmp/fhirpath-release.HqtPXI/octofhir-docker.toml`.
Both benchmark HTTP processes were stopped; the Docker database remains running.

Reproduce against a separately started test server:

```sh
BASE_URL=http://127.0.0.1:18888 NODES=100 VUS=8 DURATION=15s \
  k6 run k6/benchmarks/validation.js
```

## VM follow-up: final local check

The first VM binary was not accepted on microbenchmarks alone: after a validation
workload its HTTP regex path repeatedly fell to 7.4–7.5k RPS vs about 8.9k before.
That binary is preserved as
`/private/tmp/fhirpath-vm.1VHZqr/server-vm-before-contention-fix`,
SHA-256 `b00c9c18d61294b9232fef7534de6019dbbb7a84894b1a3f0ce6945b742f0f84`.
Its unfavorable runs remain in the same artifact directory.

The final follow-up uses numeric frame IDs, task-owned/inline-borrowed programs,
borrowed inline property names and synchronous operators, plus borrowed prepared
regex arguments and bound regexes. Tests retain async join/select, cancellation,
scope isolation, deep execution and custom evaluator compatibility.

Current binary: `target/release/octofhir-server`, built in 8m 51s from the same
three actual sibling repositories. SHA-256:
`043c275356fe22a0dcac47f36e0b3b8fd1c8e480461000cbc79d12107fffef89`.

Final order: before → current → current → before. Each phase runs 15 seconds of
$validate followed by 30 seconds of regex, at 8 VUs. The current process remains
running between its two phases. No compilation or profiling runs concurrently;
PROFILE_EVAL is not enabled in these measurements.

| Workload / pair | Before RPS | Current RPS | Before p99 | Current p99 |
| --- | ---: | ---: | ---: | ---: |
| regex / forward | 8997.98 | 9168.93 | 3.82 ms | 3.71 ms |
| regex / reverse | 9069.10 | 8713.45 | 3.80 ms | 3.72 ms |
| validation / forward | 4774.19 | 4881.69 | 8.76 ms | 9.20 ms |
| validation / reverse | 4770.69 | 4943.02 | 8.39 ms | 8.60 ms |

All eight runs: zero HTTP failures and 100% checks passed, including pat-1.
The larger regex regression did not recur in these checks. However, regex
throughput ranges from approximately -4% to +2% relative to the paired baseline;
validation p99 is slightly higher. These short local tests do not establish a
whole-server performance gain or exclude smaller regressions. A longer stable-host
soak belongs in final release qualification, after the remaining cache work.

Library checks after the final code changes: 681 passed, 7 ignored; actual
`just test-coverage` 1176/1176 (100.0%); strict Clippy and rustdoc passed.
The specification runner now fails on any failed/errored/skipped case or missing
tests, with a minimum of 1176 enforced by just, CI and the release build.
The remote workflows were not launched.

The warm evaluator regex-predicate median improved from 90.77 to 71.26 us.
The new eight-thread lambda benchmark improved from 554 to 484 us relative to
the first VM, not the pre-VM baseline. Full methodology, caveats and earlier
measurements are in the sibling FHIRPath `docs/EVALUATOR_PERFORMANCE.md`.

Final logs: `/private/tmp/fhirpath-vm.1VHZqr/http-final-*`.
All test HTTP servers and profilers were stopped; Docker PostgreSQL remains
running. For diagnostic-only evaluator timing, `PROFILE_EVAL=1` enables an
additional k6 trend read from the server's evaluationTime response metadata;
that timer excludes input conversion and general HTTP handling.

## Release status

This is an unreleased coordinated follow-up, not a completed release batch.
The normal FHIRPath stack-machine executor is now implemented and verified in
the library (681 tests, 1176/1176 specification checks, strict Clippy). Shared
element-type cache invalidation and canonical-manager follow-up remain pending.
The published fhir-model minimum dependency must be raised when
the new model API is released; current sibling path patches intentionally enable
local integration before publication. No new versions, commits, pushes or release
workflows were created by this follow-up.
