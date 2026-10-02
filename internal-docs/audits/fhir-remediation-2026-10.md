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

Subsequent search implementation:

- GET type searches and POST type/_search carry trusted SMART constraints to
  native SQL. The same predicates restrict pages, lookahead and accurate/count
  totals. Grants form a union; each grant combines patient membership and its
  filter criteria with AND. Repeated filter parameters remain AND conditions.
- Patient search membership uses the loaded CompartmentDefinition search
  parameters, identified by CompartmentDefinition.code rather than its name.
  The Patient resource itself is restricted to the launch Patient ID; Patient.link
  does not extend this access to linked Patient records.
- Relative, local absolute and versioned Patient references can match; references
  to other resource types or foreign servers do not satisfy membership.
- Unknown, empty and result-control grant predicates fail closed even when client
  Prefer handling is lenient. Clients cannot deserialize trusted constraints.
- Type search includes/revincludes carry per-type trusted SQL constraints and
  evaluate the included type's access policies. Unavailable types are omitted.
  Each iterate hop starts from visible resources; constraints precede expansion
  limits. Accurate totals continue to count only primary matches. Local absolute
  references cannot alias a foreign server's resource with the same type and ID.
- Inline expansion uses search permission for the included type as server policy:
  a read-only grant does not permit reverse enumeration. This is an implementation
  choice within underlying access policies, not an explicit SMART include SHALL.
- Boolean token search now compares JSON booleans correctly, including
  Patient.active=true/false and missing values.
- Standard `_id`, `_lastUpdated` and `_profile` retain their storage semantics
  when an IG publishes an equivalent parameter under another canonical URL.
  Relative field paths, Resource paths and paths qualified by the declared
  concrete base type match. Matching still requires the expected code, parameter
  type and exact field path; unrelated expressions do not acquire column semantics.

Aggregate search implementation:

- System GET /fhir and POST /fhir/_search evaluate grants and access policies
  for each requested type. Omitting `_type` searches supported public FHIR types;
  an explicit unsupported type returns 400. Internal administrative resources do
  not enter the default system type set. No allowed type returns 403.
- Aggregate admission also evaluates the corresponding ordinary type-search
  policy, preventing an outer system/compartment route from bypassing its deny.
  Requiring both policy contexts is an explicit server authorization choice.
- Trusted per-type predicates intersect client criteria before a single union,
  sort, page, lookahead and count query. Missing map entries deny that type;
  duplicate types are deduplicated. Trusted maps cannot be supplied by clients.
- Client criteria must be declared for all requested types through
  SearchParameter.base or inherited Resource parameters. Equal parameter codes
  in unrelated type-specific definitions are insufficient. Completely unknown
  criteria retain strict/lenient handling. `_type` and `_format` are controls,
  including with strict handling.
- Type-specific and wildcard compartment searches share this aggregate SQL path;
  compartment membership intersects the caller's grant predicates. POST forms
  merge URL and body criteria, retaining repeated-parameter AND semantics.
  Compartment resource existence is not a separate prerequisite to searching;
  valid IDs with no visible members return an empty searchset.
- Membership currently covers the compartment resource itself and the direct
  links from its loaded definition. Linked Patient record propagation needs
  further work before claiming complete Patient compartment conformance.
- Search pagination addresses the configured FHIR service base and preserves the
  system/type/compartment path and criteria. System links avoid an unregistered
  trailing-slash route. Root administrative type searches retain their own base.

Bundle search and terminology implementation:

- GET type, system and compartment searches inside batch/transaction share
  aggregate admission, trusted grants, inline visibility and searchset rendering
  with ordinary HTTP searches. Relative entry paths use the same `/fhir` policy
  context. Batch failures remain per-entry; transaction admission checks every
  entry before writes.
- Native transaction searches, global counts and inline expansion execute on
  the checked-out write connection. They see the transaction's own writes and
  do not acquire a second connection, including with a pool size of one.
- Terminology expansion occurs before transaction checkout. Client parameters,
  per-type grant predicates and included-type predicates use their own metadata.
  Strict/lenient client handling remains independent of fail-closed grant checks.
- Empty ValueSets compile to false for `:in` and true for `:not-in`; no sentinel
  code is used. `:not-in` negates each entire ValueSet and ORs comma-separated
  alternatives, while repeated occurrences remain AND. Expanded token literals
  escape FHIR separators; the expansion budget includes combined alternatives.
- Unsupported generic CodeSystem hierarchy searches now return an explicit
  400 rather than pretending the seed code is a complete hierarchy. Empty or
  malformed `:above`/`:below` criteria fail even under lenient handling.
  Hierarchy seeds and expanded literals preserve FHIR separator escaping; the
  combined result budget includes the seed and all comma-separated alternatives.

Remaining authorization work:

- Extend patient and filtered SMART query constraints to history and aggregate
  operations. Complete Bundle history/vread and read response projection,
  conditional request headers and POST search/operation interactions.
- Implement Patient.link compartment propagation with its specified direction
  and transitive membership, without automatically expanding SMART launch rights.
- Correct remaining service-base URLs in history, writes and Bundle responses.
- Complete include support for version-specific references, wildcard expansion
  and reference expressions; verify hierarchy-based grant filters, escaping and
  expansion limits, and terminology filters in chains/reverse chains. Add actual
  hierarchy expansion for generic CodeSystems currently rejected as unsupported;
  verify SNOMED hierarchy results, supported system recognition and ECL inputs.
- Ensure search self links report the criteria actually applied when unknown
  parameters are ignored under lenient handling.
- Make duplicate SearchParameter selection deterministic and separate runnable
  definitions from IG declarations that only document conformance expectations.
- Authorize the exact resource snapshot read or mutated by handlers, including
  cached reads and concurrent compartment changes.
- Complete named-operation data access and worker authorization beyond the
  scoped metadata, everything and graph paths described below. Audit validation
  stored CQL library loading, terminology/storage helpers, SOF, Bulk
  Data and subscription operations; carrying context alone does not enforce it.
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
- Every named-operation handler now receives request identity from trusted
  extensions. Resource helpers evaluate ordinary read/search/update permissions
  and policies for the actual data access, rather than assigning one universal
  CRUD scope to every named operation. SMART grants and patient membership are
  compiled into native SQL before selecting resources or counting results.
- Instance `$meta` uses scoped reads. `$meta-add`/`$meta-delete` require update,
  independently of read, check policies against previous and proposed metadata,
  pin the selected version, and verify the resulting grant attributes inside
  the write transaction. Escaping a filtered grant rolls back metadata and
  version changes; successful writes invalidate the local resource cache.
- Metadata edits validate the supplied Meta datatype and resulting resource
  against the base schema and available declared profiles, after ordinary update
  policy checks and before opening the write transaction. Malformed Meta returns
  400; a nonconforming result returns 422 with a generic diagnostic that cannot
  disclose existing clinical fields to an update-only caller. Unchanged references
  are not fetched: this preserves update permission independently of read. Unknown
  declared profiles retain the server's warning policy; verification of profile
  claims is server policy, as allowed by Meta.profile's specification.
- Both metadata mutation operations enforce one typed meta parameter. Addition
  deduplicates profiles/tags/security within a single request as well as against
  stored metadata. Coding identity uses system+code, ignoring display/version;
  profile identity uses the complete canonical. Deletion omits emptied arrays.
  Profile values and their primitive id/extensions remain paired through insertion,
  deduplication and removal; extension-only elements retain their distinct payloads.
- Patient/Encounter/Group `$everything` root reads and related searches, and
  `$graph` focal/forward/reverse/GraphDefinition access, use these helpers.
  Related types denied by policy are omitted; search infrastructure failures
  propagate instead of returning successful partial results.
- `$everything` uses loaded compartment definitions, reads all authorized
  search pages, deduplicates and sorts before global pagination, and applies
  `_type`/`_since` to the root as well as related resources. GET and POST
  Parameters share parsing, including repeated `_type`; invalid counts, instants,
  types and inverted date bounds return FHIR errors. Links retain operation
  criteria and use the configured FHIR base; fullUrl identifies each resource.
- Instance `$fhirpath`, `$cql` and `$evaluate-measure` now use
  scoped access when loading their stored target.
- `$validate` reference checks use an invocation-local validator and resolver.
  Every local existence/body check uses the caller's read grants and ordinary
  read policy, with canonicalized aliases coalescing to one authorized snapshot.
  Relative and configured FHIR-base absolute references share this snapshot.
  Upstream resolver errors are retained and returned through the HTTP handler;
  they cannot become successful validation merely because a resolver error was
  skipped by the underlying validator.
- A hidden/missing target under a constrained grant returns masked 404, because
  its actual existence cannot be established. Missing targets under unrestricted
  read grants remain ordinary invalid-reference issues with HTTP 200. Missing
  read permission or a resource-policy denial returns 403. Supplied content with
  no data reads can still undergo general validation without read permission.
- The named validator verifies the actual reference body against a targetProfile,
  rather than accepting meta.profile as proof of conformance. Unrestricted
  version-specific reads use the requested historical snapshot. Constrained
  historical grants currently return explicit 501 instead of substituting the
  current version or bypassing the grant; historical predicate execution remains
  required before claiming full support.
- Validation outcomes preserve both the errors and warnings returned by the
  validator. Unavailable declared profiles are recognized by their role, including
  URN/relative canonicals; they remain warnings while the base schema is checked.
  Explicitly requested unavailable profiles still fail the operation.
- Invalid/duplicate validation modes and conflicting parameter value choices
  return 400; update/delete modes require an instance. Resource/profile parameters
  enforce 0..1 cardinality. Content is required except for delete; the previous
  instance GET fallback no longer reads a stored resource implicitly.
- Mode-specific validation checks c/u/d independently of read, ordinary policies,
  identity, preprocessing and storage constraints. It speculatively executes the
  write in an always-rolled-back transaction: indexes/history are rolled back,
  pending resource events are discarded, and postprocessing/cache invalidation
  never run. Caller-scoped secondary reference checks still apply when used.
- Update/delete lock the selected live row before evaluating its grants/policies.
  Trusted native predicates select that exact version within the transaction.
  Restriction state survives preparation into per-type predicates. Update checks
  If-Match and matches primary PUT's create-on-update and body identity rules.
  Body meta.versionId is not an If-Match condition. Delete ignores supplied
  content and follows current storage's idempotent deletion constraints.
- Body/write constraint failures produce HTTP 200 OperationOutcome errors;
  missing permission/policy denial remains 403, and hidden constrained targets
  return masked 404. The primary write surface currently denies filtered grants;
  mode checks preserve this behavior instead of predicting an unsupported write.
- POST ignores client id before schema validation, including malformed id values.
  SearchParameter expression syntax is validated before mutation, including when
  schema validation is explicitly skipped. General validation shares this check;
  registry installation cannot fail for this syntax error after a saved row.
- HTTP `$cql` now supplies an evaluation-local resource search provider for both
  expressions and inline libraries, at system/type/instance level. Secondary
  retrieves use native search with the caller's SMART grants and ordinary search
  policies. Client context values are additional criteria, never identity or
  permissions; simultaneous callers do not replace a shared provider.
- Synchronous CQL evaluation runs on a blocking thread with the Tokio runtime
  entered, permitting async storage and FHIRPath callbacks even from a
  single-thread async caller. The upstream provider trait cannot return errors,
  so the first retrieve error is retained per evaluation and rejects its entire
  output with the original host access error. No partial clinical result is
  returned. Every evaluation starts with a fresh error slot.
- CQL retrieves follow all search pages until their configured resource limit;
  exceeding that limit or encountering a page with no progress is an explicit
  error rather than silent truncation. Async retrieves share an evaluation
  deadline. Actual Patient resources supplied as context now retain their ID;
  Patient `_id` criteria use the bare ID, reference criteria use `Patient/id`.
- Patient/Encounter launch context survives either PolicyContextBuilder call
  order, including replacing environment settings. Compartment routes dispatch
  GET named operations before parsing history controls, so unrelated history
  validation cannot intercept operation parameters.

Remaining:

- Complete native atomic primary write authorization for filtered SMART grants
  and share its supported constraints with write validation, replacing the
  temporary compatibility guard that reflects current primary-write denial.
  Preserve repeatable primitive operation parameters through GET/POST-query
  normalization so handler cardinality checks also see duplicate query values.
  Reconcile mode checks with custom non-FHIR administrative/gateway write rules.
  Metadata profile checks do not dereference unchanged references; evaluate
  targetProfile constraints introduced by new profile claims through trusted
  integrity checks with sanitized outcomes, preserving update-only access.
  Complete constrained historical reference predicates and version-aware ordinary
  write reference resolution. Remove declared-profile shortcuts from other
  validation paths where an earlier declaration cannot prove present conformance.
  Invocation-local named validators retain shared model/schema loading but not
  the shared compiler/constraint-plan cache; verify operation latency and consider
  an upstream per-invocation resolver API to share immutable compiled plans.
- Complete `$everything` clinical care-date mapping (a generic `date` search
  parameter is insufficient across resource types), linked Patient propagation,
  referenced supporting resources, Group member reference/activity semantics,
  and type-level invocation. Complete `$graph` reverse pagination, explicit
  traversal-limit errors, versioned references and GraphDefinition constraints.
- Complete CQL model-specific context and property mappings, related-context
  retrieves, multi-code/date criteria and real terminology semantics. The
  existing internal stored-library cache still needs authorization-aware loading
  before exposing it through HTTP/jobs. Tokio timeouts stop waiting and cancel
  async retrieves, but cannot forcibly interrupt an already-running pure CPU
  evaluator; bounded execution/cancellation needs further work.
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

The initial checkpoint saved in commit `6b05f69`:

- Server library: 356 tests passed.
- PostgreSQL transaction/batch suite: 21 tests passed, including rollback,
  concurrent writes, conditional cardinality and explicit validation profiles.
- SMART authorization/revocation HTTP regression: passed.
- Clippy for server, auth and auth-postgres libraries/tests: warnings denied,
  passed.
- Full workspace/all-feature checks remain for the final remediation state.

Subsequent search checkpoint:

- Server library: 360 tests passed; search library: 366 passed.
- PostgreSQL library: 27 passed; reference/search traversal suite: 8 passed.
  Two manual benchmark tests were intentionally ignored across these suites.
- Extended SMART HTTP regression passed with signed Patient launch JWTs,
  restricted scope filters, GET/POST search, accurate/count totals, wrong-client
  query predicates, per-type include filtering and unchanged bootstrap development
  access. A denied included-type policy omits related resources even when SMART
  scopes permit them. PostgreSQL regressions cover denied iterate nodes, expansion
  limits,
  raw/parsed parity, deleted references and foreign-reference ID collisions.
- Clippy for server/search/PostgreSQL libraries and tests passed with warnings
  denied; formatting and diff checks passed.
- Search builds use `/private/tmp/octofhir-remediation-target` with debug info and
  incremental compilation disabled, preserving the cleaned repository target.

Aggregate search verification checkpoint:

- Server library: 360 passed; search library: 367 passed; API library: 29 passed.
  The POST compartment authorization-context regression passed.
- PostgreSQL reference/search traversal suite: 9 passed, including aggregate
  per-type grants, shared criteria, duplicate types, global pagination/counts,
  grant/client intersections and inline visibility; one manual benchmark ignored.
- Extended SMART HTTP regression passed with GET/POST system and compartment
  searches, wildcard compartments, empty compartments, scope/policy denial,
  strict `_type`, count-only totals, root administrative links and following
  GET/POST type/system/compartment pagination URLs.
- Clippy for API/server/search/PostgreSQL/auth libraries and tests passed with
  warnings denied. Formatting and diff checks passed; repository target absent.

Bundle search verification checkpoint:

- Server library: 360 passed; search library: 368 passed; PostgreSQL library:
  27 passed; storage library: 9 passed. One manual library benchmark ignored.
- PostgreSQL reference/search traversal suite: 10 passed, one manual benchmark
  ignored. The added transaction test covers both type orders, trusted grants,
  count-only queries, forward/reverse inclusion, own writes and rollback with a
  single connection and a five-second timeout.
- Extended SMART HTTP regression passed with nested batch/transaction type,
  system and wildcard compartment searches, strict/lenient handling, scoped
  totals, ValueSet grant/client filters and included-type filters. It includes
  real codes that collide with the old empty-set sentinel and escaped commas,
  plus combined expansion limits and comma-separated `:not-in` alternatives.
- The final SMART HTTP regression also passed the Bundle policy-denial cases:
  a denied Observation type is omitted from mixed system/compartment searches
  and reverse includes; Observation-only batch entries return 403 and denied
  transaction admission returns 403 before writes.
- Complete transaction suite: 22 passed after fixture isolation was extended
  to container startup/teardown as well as registry rebuild. This preserves
  concurrent requests within a test and includes the single-connection own-write
  search, count/include and rollback regression. The 50-write transaction also
  passed separately and completed in about 66 ms. Published core archives are
  pinned in these fixtures to reuse the integrity-checked cache without package
  metadata preflight. Earlier fixture metadata/pool failures were resolved;
  response assertions and performance thresholds were preserved.
- The final SMART HTTP regression additionally passed unsupported and empty
  hierarchy criteria across direct, batch and transaction requests, including
  lenient handling. The new library regression verifies escaped hierarchy
  literals and rejects malformed separators and escapes.
- Final Clippy for server/search/PostgreSQL/storage/auth libraries and tests
  passed with warnings denied, including policy-denial tests, fixture changes
  and hierarchy changes. Formatting and diff checks passed; repository target
  remains absent.

Named-operation verification checkpoint:

- API library: 29 passed; auth library: 681 passed; server library: 360 passed.
  The added builder regression verifies both launch/environment call orders and
  replacing environment settings without losing patient/encounter identity.
- Extended SMART HTTP regression passed on disposable PostgreSQL. It covers
  read versus search permissions for `$meta`, hidden foreign/nonmatching
  resources, stored-target validation/FHIRPath reads, GET/POST `$everything`
  totals, configured links and following pages, oversized offsets, invalid
  parameters, and filtered reverse `$graph` traversal. A hot-reloaded resource
  policy denies a matching `$meta` read and removes Observation from everything.
- Metadata HTTP tests prove that read permission alone cannot add metadata,
  update permission can do so without read permission, and deleting the profile
  that grants access returns 403 with the complete stored resource, version and
  timestamp unchanged. Bootstrap development access remains unchanged.
- Final Clippy for server/auth/API libraries and tests passed with warnings
  denied. Formatting and diff checks passed; the repository target is absent.

CQL data-access verification checkpoint:

- CQL service library: 19 passed, including actual retrieves on a single-thread
  runtime, multiple pages, failure after a successful page, preservation of a
  typed host access error, next-request independence, explicit limits/no-progress,
  Patient resource context and retrieve deadlines.
- Extended SMART HTTP regression passed with GET/POST CQL, all invocation levels,
  inline library counts, filtered secondary resources, client-supplied foreign
  Patient context, missing search scope, pure computation and concurrent lab/vital
  grants. A reloaded Observation search-deny policy returns 403 from CQL without
  converting the result to an empty list.
- Clippy for server/CQL libraries and tests passed with warnings denied.
- Server library: 360 passed after the CQL integration changes. Formatting and
  diff checks passed; builds continue outside the cleaned repository target.

Named-validation verification checkpoint:

- Server library: 367 passed. Five new resolver tests cover canonical aliases,
  shared existence/body snapshots, distinct historical snapshots, definite
  missing targets and retained upstream-ignored access errors. A real R4
  targetProfile regression verifies both valid and invalid bodies carrying the
  same declared profile: the declaration cannot bypass a required birthDate.
- The warning regression covers unknown URN and HTTP declared profiles, and
  preserving the actual upstream warning stream. The mode regression covers
  absent/invalid/duplicate modes, instance-only modes and required resources.
- Final extended SMART HTTP regression passed with targetProfile checking
  enabled. It covers type/instance validation, relative/local absolute targets,
  foreign and missing filtered targets, read versus search scopes, definite
  missing targets with HTTP 200, exact historical versions and missing versions,
  constrained-history 501, policy denial, warnings and valid/error outcomes.
  Properly formed unimplemented modes return 501; missing required resource and
  invalid mode/level combinations return 400.
- Complete transaction suite: 22 passed after the shared validation adapter
  changes, including explicit-profile handling, invalid resources/references,
  rollback, concurrent transactions and single-connection search/count/includes.
- Final Clippy for server libraries/tests passed with warnings denied.
  Formatting and diff checks passed; repository target remains absent.

Metadata-conformance verification checkpoint:

- Server library: 370 passed, including new regressions for one typed meta
  parameter, duplicate set entries within one request, Coding identity ignoring
  display/version, omitted empty sets and paired primitive profile metadata.
- Extended SMART HTTP regression passed on disposable PostgreSQL. It rejects
  an available Observation profile added to Patient with 422 and malformed
  profile/tag/security datatypes with 400, retaining the complete resource,
  version and timestamp. The update-only caller receives no stored clinical
  values in validation diagnostics.
- HTTP also verifies available Patient profile deduplication, retention/removal
  of profile primitive metadata, deletion of the final security label and an
  Observation metadata edit without permission to read its unchanged Patient
  subject. Unknown declared profiles and bootstrap development rights remain
  accepted. Existing grant-escape rollback, policy, CQL, scoped validation and
  Bundle regressions continue to pass in this fixture.
- Final Clippy for server libraries/tests passed with warnings denied; formatting
  and diff checks passed. Builds stay under the external target directory and
  the repository target remains absent.

Write-validation verification checkpoint:

- Server library: 372 passed; PostgreSQL library: 27 passed (one manual benchmark
  ignored); storage library: 9 passed. New mode/resource/profile protocol tests
  cover cardinality, conflicting value choices and instance-only modes. ETag
  parsing tests cover quoted/weak versions and malformed or multiple values.
- Extended SMART HTTP regression passed. It checks valid/invalid create/update,
  ignored malformed POST ids, required/mismatched PUT ids, matched/stale/malformed
  If-Match, ignored body meta.versionId, create-on-update and missing-target
  behavior. Write-only u/d work without r when no secondary reads are needed.
  Foreign/missing patient-constrained targets are masked; missing write rights
  and a hot-reloaded update-deny policy cannot return a successful write result.
- Speculative operations preserve complete JSON, version, timestamp, history and
  resource counts. Eight concurrent validation updates alongside a real PUT
  retain exactly the real committed result and its one history version.
- SearchParameter tests reject malformed expression syntax with 422 before
  saving, both with ordinary and explicitly skipped schema validation. Validation
  reports its expression error with HTTP 200. A valid speculative SearchParameter
  neither creates a row nor installs its code into the live search registry.
- Complete transaction suite: 22 passed after the shared resource-rule and
  transaction read-lock changes. Final Clippy for server/storage/PostgreSQL
  libraries and tests passed with warnings denied. Builds remain external to
  the cleaned repository target.

The PostgreSQL suite rebuilds the process-global canonical registry for each
temporary database and serializes fixture lifetimes. Concurrent requests within
tests still run concurrently. On macOS, run the complete suite with a sufficient
file descriptor limit (`ulimit -n 4096`) to avoid EMFILE during repeated setup.

## Specification references

- [FHIR R4 RESTful API](https://hl7.org/fhir/R4/http.html)
- [FHIR search includes](https://hl7.org/fhir/search.html#revinclude)
- [FHIR R4 FHIRPath Patch](https://hl7.org/fhir/R4/fhirpatch.html)
- [FHIR R4 Bundle](https://hl7.org/fhir/R4/bundle-definitions.html)
- [FHIR R4 CompartmentDefinition](https://hl7.org/fhir/R4/compartmentdefinition-definitions.html)
- [SMART STU 2.2 scopes](https://hl7.org/fhir/smart-app-launch/STU2.2/scopes-and-launch-context.html)
- [OAuth token revocation, RFC 7009](https://www.rfc-editor.org/rfc/rfc7009.html)
- [FHIR R4 validate](https://hl7.org/fhir/R4/resource-operation-validate.html)
- [FHIR R4 SearchParameter.expression](https://hl7.org/fhir/R4/searchparameter-definitions.html#SearchParameter.expression)
- [FHIR R4 named operations](https://hl7.org/fhir/R4/operations.html)
- [FHIR R4 meta-add](https://hl7.org/fhir/R4/resource-operation-meta-add.html)
- [FHIR R4 meta-delete](https://hl7.org/fhir/R4/resource-operation-meta-delete.html)
- [FHIR R4 Meta.profile](https://hl7.org/fhir/R4/resource-definitions.html#Meta.profile)
- [FHIR R4 JSON primitives](https://hl7.org/fhir/R4/json.html#primitive)
- [FHIR R4 Patient everything](https://hl7.org/fhir/R4/patient-operation-everything.html)
- [CQL retrieve and evaluation contexts](https://cql.hl7.org/02-authorsguide.html#retrieve)
- [FHIR R4 OperationOutcome](https://hl7.org/fhir/R4/operationoutcome-definitions.html)
- [Bulk Data export](https://hl7.org/fhir/uv/bulkdata/STU2/export.html)
