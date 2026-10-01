# FHIR audit remediation

Scope: address the audit findings in dependency order, against the configured
FHIR release and applicable SMART/Bulk Data specifications. The development
bootstrap client intentionally retains its broad policy and default scopes.
Negotiating narrower token scopes must still constrain that client.

## Authorization and revocation

Implemented and verified with unit and disposable PostgreSQL HTTP tests:

- Negotiated SMART scopes constrain access policy decisions.
- SMART v2 instance history uses read; type/system history uses search.
- Policy contexts retain patient/encounter launch context and detect POST search.
- Batch entries are authorized independently; transaction entries are checked
  before writes. Denied transaction entries roll back the whole request.
- Patient resource checks reject foreign references and cross-compartment
  rewrites. Requests requiring query constraints currently fail closed.
- Revocation checks precede auth cache hits. Revocation persistence uses the
  actual table schema; wrong token hints do not suppress revocation. Storage
  failures return a retryable error.
- Bootstrap initialization completes before the policy cache is populated.

Remaining authorization work:

- Push patient and filtered SMART grant constraints into search, includes,
  history and aggregate operations, with correct pagination and totals.
- Authorize the exact resource snapshot read or mutated by handlers, including
  cached reads and concurrent compartment changes.
- Propagate authenticated identity and grants into named operation handlers and
  workers; remove the named-operation scope bypass.
- Verify JWT expiration boundaries and introspection hint fallback.

## Bundle write contracts

Implemented and verified with focused HTTP regressions:

- Direct PUT requires a matching resource ID and resource type.
- Batch PUT/PATCH validate the resulting resource; PATCH honors If-Match.
- PATCH uses the shared JSON Patch/FHIRPath engines. Bundle PATCH accepts FHIR
  resource representations, rather than a raw array in entry.resource.
- Transaction PATCH validates before acquiring its write connection and checks
  the snapshot version during the write.
- Transactions reject repeated mutations of one resource identity.
- POST assigns a server ID, including in batch and transaction.
- FHIRPath selections preserve physical locations, support indexed and filtered
  selection, enforce singleton targets, and reject invalid indices.
- Conditional matching ignores result controls such as _count, offset, summary
  and projection. Each predicate has its own complete cardinality check;
  URL-encoded identifier tokens are decoded by the normal search parser.

Remaining write/concurrency work:

- Resolve conditional mutations on the primary database within an atomic
  concurrency protocol, including overlapping predicates and transactions.
- Verify conditional PUT body ID semantics for zero/one/multiple matches.
- Complete FHIRPath Patch coverage for choice/anonymous types and primitive
  extensions; keep array values and primitive metadata aligned.
- Complete cache invalidation for batch/conditional deletes and cross-replica
  writes, and prevent stale reads from crossing authorization boundaries.

## Operations and advertised capabilities

Implemented:

- $validate uses explicit profiles from Parameters and POST query parameters.
  An unavailable requested profile returns an error. OperationOutcome always
  contains at least one issue, and validation warnings retain their severity.

Remaining:

- Make CapabilityStatement reflect registered, implemented interactions and
  operations; correct operation names and definition identifiers.
- Do not advertise US Core conformance merely because its package is installed.
- Refresh capability caches on package/operation changes.
- Map supported R6 version spellings consistently across configuration and model
  loading.
- Replace evaluateMeasure placeholder behavior with a correct implementation or
  an explicit unsupported contract and corresponding capability declaration.

## Bulk data, asynchronous jobs and outbound requests

- Apply Group membership, export level and grant filters to Bulk Data exports.
- Correct kickoff status/URL, output formats, _since validation and manifests;
  report partial failures instead of silent success.
- Persist trusted job ownership and authorization; enforce them for status,
  result, file download and cancellation.
- Cancellation must stop work and cannot be overwritten by completion.
- Add durable job claims/recovery and restart handling.
- Implement async Bundle execution advertised by submission endpoints.
- Constrain import/webhook outbound requests, redirects and timeouts.

## Lifecycle and delivery

- Handle SIGTERM and configured HTTP read/write timeouts.
- Persist reliable resource events for audit/subscriptions; handle lag/retries,
  previous snapshots and delete filter behavior.
- Run focused regressions, existing transaction/rollback suites, Clippy with
  warnings denied, and the required workspace checks after the final changes.

## Verification checkpoint

- Server library: 356 tests passed.
- PostgreSQL transaction/batch suite: 21 tests passed, including rollback,
  concurrent writes, conditional cardinality and explicit validation profiles.
- SMART authorization/revocation HTTP regression: passed.
- Clippy for server, auth and auth-postgres libraries/tests: warnings denied,
  passed.
- Full workspace/all-feature checks remain for the final remediation state.

The PostgreSQL suite rebuilds the process-global canonical registry for each
temporary database and serializes fixture lifetimes. Concurrent requests within
tests still run concurrently. On macOS, run the complete suite with a sufficient
file descriptor limit (`ulimit -n 4096`) to avoid EMFILE during repeated setup.

## Specification references

- [FHIR R4 RESTful API](https://hl7.org/fhir/R4/http.html)
- [FHIR R4 FHIRPath Patch](https://hl7.org/fhir/R4/fhirpatch.html)
- [FHIR R4 Bundle](https://hl7.org/fhir/R4/bundle-definitions.html)
- [SMART STU 2.2 scopes](https://hl7.org/fhir/smart-app-launch/STU2.2/scopes-and-launch-context.html)
- [OAuth token revocation, RFC 7009](https://www.rfc-editor.org/rfc/rfc7009.html)
- [FHIR R4 validate](https://hl7.org/fhir/R4/resource-operation-validate.html)
- [FHIR R4 OperationOutcome](https://hl7.org/fhir/R4/operationoutcome-definitions.html)
- [Bulk Data export](https://hl7.org/fhir/uv/bulkdata/STU2/export.html)
