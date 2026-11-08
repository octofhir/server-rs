# Built-in DICOM integration

## Goal and boundaries

Provide a vendor-independent bridge between OctoFHIR's clinical resources and
external PACS through DICOMweb. PostgreSQL stores FHIR metadata, references and
integration state only. DICOM instances, pixel data, frames, segmentations and
structured-report objects remain in PACS. No DICOM payload is written to Binary,
JSONB, bytea, job bodies or database logs by the connector.

The implementation does not depend on any specific PACS product. Standards-based
capabilities are selected explicitly. A deployment can use several connections;
each connection initially owns a separate ImagingStudy representation, even when
two archives contain the same Study Instance UID. Cross-archive merging needs an
explicit patient/tenant and manifest policy, not implicit UID deduplication.

## Delivery sequence

### 1. Explicit metadata import (implemented)

- Named, disabled-by-default WADO-RS connections in configuration.
- `POST /fhir/Patient/{id}/$dicom-import` with connection and Study Instance UID.
- Bounded metadata-only HTTP requests; guarded DNS/egress, no redirects or
  environment proxy, no request-supplied target URL, environment-based credentials.
- Resolve identity against an existing readable Patient using the configured
  issuer -> FHIR identifier-system mapping. Fail on missing or different identity;
  never create/merge a patient from a name or a bare PatientID.
- Deterministic ImagingStudy/Endpoint identities, supported R4/R4B/R5 shapes,
  canonical DICOM UIDs, sorted series/instances and accurate snapshot counts.
- Endpoint and ImagingStudy changes go through the existing FHIR transaction
  pipeline: resource policies, validation, references, history and commit events.
- Preserve user metadata/security labels and clinical annotations. Do not change
  the patient on repeat import. Skip unchanged projections; use ETags for updates.
- Unit/mock HTTP tests and a PostgreSQL-backed HTTP scenario cover the public path.

Acceptance: a configured PACS study imports into an existing patient; re-import
does not duplicate resources or versions when unchanged; invalid/mixed metadata,
patient mismatches and failed writes cannot leave a half-imported resource pair.

### 2. Offline and private IG package sources (implemented)

Do this in the existing canonical-manager integration, not in the DICOM module.
Acquisition from registry, read-only package directories, local `package.tgz` and
explicitly configured private S3 objects is implemented.
A mounted directory contains an actual published FHIR package
(manifest and resource JSON), not an IG's unbuilt FSH/Git source tree. Relative
paths resolve against the configuration location, not a process-dependent cwd.

- `PackageSpec.path` supports directories and gzip tar archives, infers manifest
  identity and forwards all sources to canonical-manager local overrides.
- `packages.offline = true` disables package HTTP clients, preflight and runtime
  registry access. Explicit pinned S3 acquisition remains allowed. Iterative,
  bounded dependency closure uses configured sources,
  installed packages and exact cached archives. Missing dependencies are fatal and
  identify the requesting package.
- Acquired archive directories remain owned by the manager, including runtime
  installs and hot reload; extraction never writes to the input source.
- Directory sources must read/copy input; never mutate a read-only volume mount.
- Read manifest identity and exact version from the package and verify explicit
  configuration agrees. Bound archive expansion and reject escaping paths.
- Give all sources the same dependency-resolution/install/conversion path,
  including transitive StructureDefinitions, ValueSets and CodeSystems.
- Registry is one optional source. An offline mode never contacts it for metadata
  preflight or dependencies. Dependencies must be installed/cached or supplied by
  configured local/S3 sources; report the precise missing dependency otherwise.
- Keep package availability separate from validation policy. A deployment can
  explicitly require canonical profiles per resource type and choose strict handling
  for unavailable declared profiles. Installing an IG alone must not claim or force
  conformance for every resource. These policy controls are a subsequent step.
- For S3 package sources specify bucket/key/region/endpoint and credentials by
  reference. Pin object identity/checksum; do not persist expiring presigned URLs
  as the package identity. Preserve origins in installation diagnostics.
- Covered: read-only directories, real RuCore archive extraction, cold offline
  bootstrap into an empty database, installed-package restart without download
  archives, transitive closure, missing dependencies, release/identity mismatch,
  malformed indexes, escaping/link/duplicate/oversized/corrupt archives.
- S3 sources reference a database-backed StorageProvider plus bucket/key, an optional
  object version and mandatory SHA-256. Providers support explicit endpoint, signing
  region and path/virtual-hosted addressing. Credentials use environment references
  or encrypted stored keys (including temporary session tokens).
  Guarded SigV4 GETs stream into an atomic, verified content-addressed archive cache.
  Private endpoints require the normal egress allowlist; redirects/proxies and
  decompression are disabled. Cache hits need no credential lookup or S3 access.
- Mock HTTP tests cover paths, tokens, versions, checksum failure, redirects,
  private egress, streaming limits and cache corruption/reuse. A pinned RustFS
  1.0.4 + PostgreSQL Docker test proves signed private reads, version selection,
  cold registry-offline RuCore installation, validation and cache-only restart.
  `docker-compose.s3.yml` provides a repeatable local S3 compatibility environment.
- General clinical S3 blob storage remains a separate lower-priority stage.
  Additional credential providers and backend-specific capabilities can be added
  to package acquisition independently of clinical file storage.

Acceptance: a private IG and its terminology can boot in a container with no
registry access, using only mounted packages (or explicitly configured S3 access).

### 3. Discovery and durable reconciliation (implemented)

- Add QIDO-RS discovery separately from WADO-RS retrieval.
- Implemented change sources: bounded QIDO scans and explicit ordered notifications.
  An external replayable feed adapter can use notifications. Vendor feed protocols
  and Repository Query cursors are optional capability extensions, never assumed.
- StudyDate is acquisition time, not an ingestion/change cursor. Late imported
  priors, corrected patient metadata and post-processing series need coverage.
- Persist checkpoints/retries, coalesce events per study, re-read current metadata,
  and advance only after successful FHIR writes. Fence multi-instance workers.
- Handle deletion/rejection, partial studies, unavailable instances and patient
  reconciliation explicitly. Network failures must not become deletion events.
- Background work uses a configured service identity and the normal write path;
  never synthesize unrestricted scopes from a PACS credential.

Acceptance: restart, duplicates, reordered notifications and interrupted delivery
converge to the same representation without losing changes or reassigning patients.

### 4. Optional streaming DICOMweb gateway (implemented)

- Keep direct PACS access via Endpoint as the initial retrieval model.
- An opt-in gateway can expose WADO-RS under OctoFHIR's authorization boundary.
- Authorize exact study/series/instance membership; never expose unrestricted PACS
  searches or arbitrary BulkDataURI fetches through a shared service credential.
- Preserve multipart responses, Accept/Transfer Syntax negotiation, cancellation
  and backpressure. Bound buffers; do not persist payloads in PostgreSQL.
- The existing buffered generic proxy is not suitable for large imaging bodies.

Acceptance verified: attested membership and current primary-backed Patient/study
policies gate every request. Mock HTTP + PostgreSQL tests cover forged manifests,
narrow/anonymous denial, 32 MiB streams, cancellation and chunked limit failure.
The same synthetic DICOM fixture passes QIDO, WADO metadata, multipart object,
frame and rewritten bulk retrieval against pinned Orthanc and dcm4chee containers.
An isolated Chrome canvas viewer renders and verifies all 256 source pixels from
Orthanc through the gateway. This is a constrained retrieval implementation;
whole-study/series binary export and unrestricted archive QIDO are not advertised.
No general-purpose OHIF/Stone conformance claim is made.

### 5. UPS-RS workflow (implemented)

- Optional named UPS-RS Requestor/Watcher configuration, separate guarded HTTP and
  same-origin WebSocket endpoints, explicit backend identity/scopes and coding
  schemes. Task Performer, inference and STOW remain external.
- `POST /fhir/Task/{id}/$dicom-ups` admits submit/cancel/refresh/status against an
  existing authorized Task, Patient and attested input ImagingStudy. No caller URL,
  dataset, patient override or UID. Unknown modifiers, negated work, repetitions,
  unsupported routing and input types fail explicitly.
- One persisted workitem UID and immutable typed input projection per Task. Normal
  Task CRUD cannot change that projection or delete an active binding. Fresh source
  metadata verifies the patient and exact SOP references before external creation.
- Commands use the existing async executor, transactionally attached jobs, leases,
  execution fences and FHIR checkpoints. Ambiguous create responses recover by GET
  of the same UID. Confirmed items are never recreated after disappearance.
- Cancellation before any create attempt is distinguishable from uncertain delivery.
  Cancel acknowledgment is not CANCELED. Only a retrieved source state becomes a
  FHIR transition; terminal state regression requires manual reconciliation.
- Filtered persistent label namespaces, WebSocket-before-subscribe ordering and
  deletion locks protect only the source's own workitems. Events are bounded wakeups;
  reconnect and polling query current state. Terminal locks release only after the
  final FHIR checkpoint is applied. Independent databases require distinct AE titles.
- Processing parameters are compared again on retrieval, including exact DICOM DS
  normalization. FHIR SOP Class OID URIs convert to raw DICOM UIDs at the boundary.
- Read output references under Performed Procedure Sequence, verify actual output
  studies/instances and pinned patient through the shared metadata importer, then
  publish explicitly mapped DiagnosticReport/DocumentReference, Task and Provenance
  through the normal transaction pipeline. Outputs may use another Study. SR/SEG/PM
  objects stay in PACS; no clinical measurements or final report status are inferred.
- Preserve clinical annotations and inherit security labels without removing operator
  labels. Whole-object references are supported; frame/segment subset results fail
  explicitly. Missing SOP Class mappings block completion rather than guessing.

Acceptance: PostgreSQL-backed independent protocol tests cover duplicate/ambiguous
creation, missed notifications and restart, immutable inputs, publication from two
output studies, clinical/security preservation, no-op refresh, cancellation races
and deletion-lock release ordering. The pinned dcm4chee scenario exercises a separate
external Performer, coded/numeric inputs, cancellation acknowledgment, completion,
configured output publication and authenticated retrieval. Orthanc and dcm4chee both
pass the independent QIDO/WADO scenario. Only one real UPS Task Manager plus the
protocol peer have been checked: no broad UPS interoperability claim is made.

### 6. Optional general file storage driver (S3, deferred draft)

Separate from DICOM and lower priority. Add an owned `BlobStore` boundary for
Binary/Attachment payloads, with an optional S3/S3-compatible driver selected by
configuration. PACS continues to own images regardless of this setting.

Proposed configuration shape (not yet supported):

```toml
[files]
driver = "s3"

[files.s3]
bucket = "clinical-files"
region = "eu-central-1"
prefix = "octofhir/"
# endpoint = "https://s3.internal.example"  # S3-compatible deployments
# Credentials come from the environment/workload identity/secret references.
```

- Stream puts/gets, expose metadata and ranged reads, keep internal object keys
  separate from public attachment URLs and authorize every file access.
- PostgreSQL contains metadata and stable blob references, never the payload.
- Preserve Binary's external FHIR contract: `data` is not replaced by a nonstandard
  URL field; raw and FHIR-JSON representations resolve the external bytes as needed.
- Do not regex-walk arbitrary resource JSON to discover Attachment fields: use
  model/schema information and explicit supported resource paths.
- Define immutable resource-version references and retention before adding delete
  or deduplication. A failed FHIR transaction must not publish a broken reference;
  orphan object cleanup and retries are explicit.
- Keep package acquisition from S3 separate from clinical blob authorization and
  retention, even if both use the same lower-level S3 client.

## Real IG acceptance fixture

The pinned, unmodified `ru.core#1` package from the upstream `build` branch is an
R4B test fixture. Its national-profile-specific URLs appear only in the fixture,
tests and documentation. Production loading and validation have no RuCore branches.
Tests prove local-directory startup, cold offline archive startup with dependency
archives, profile slicing (positive and negative), and local ValueSet/CodeSystem
validation. Package presence alone is insufficient.

## Standards and validation

- FHIR R4/R4B/R5 ImagingStudy and Endpoint, release-specific DICOM tag mappings.
- DICOM PS3.18 Studies Service (QIDO-RS/WADO-RS/STOW-RS), Worklist Service (UPS-RS).
- IHE WIA for image access; AIW-I/AIR for task roles and imaging results.
- IHE MADO is an optional trial-use interoperability target for exported manifests,
  not a prerequisite for local indexing. A resource alone is not full IHE compliance.

Protocol and conformance tests should precede claims for each stage. No stage
advertises an operation or backend before its implementation is available.

## Open specification gaps (2026-11-08 audit)

The delivery stages above describe implemented, tested scenarios. They do not
constitute a claim of complete DICOMweb origin-server or IHE profile conformance.
The following source review compared the current gateway and UPS client with
DICOM PS3.18/PS3.4 2026d. These findings remain open; the existing PACS integration
tests do not exercise every malformed request, resource or media negotiation case.

HTTP probes against the exact v0.0.5 Docker candidate and isolated pinned Orthanc
confirmed three of these gaps: an unsupported metadata `Accept` still receives
200 JSON, the valid `accept=application/dicom+json` query receives 400, and study
QIDO with `includefield=all` returns only tags 0020000D and 00201208. Baseline
metadata retrieval and authenticated import succeeded in the same instance.

### Existing gateway behavior to correct first

- [ ] Negotiate the metadata and projected QIDO response media type. Both branches
      currently call `json_response` without checking the incoming `Accept` header.
      Unsupported requested representations must not silently receive a successful
      JSON representation. Support the standard `accept` query parameter and
      metadata `charset` negotiation, with explicit rejection of unsupported values.
      The current query whitelist rejects both parameter names. Verify conflicting
      header/query preferences and unsupported representations in HTTP tests.
      See [PS3.18 10.4](https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html).
- [ ] Complete the authorized-study QIDO response contract, or stop presenting
      the projection as a conformant QIDO Search endpoint. `includefield` currently
      passes validation but is ignored. The study projection only contains Study
      Instance UID and Number of Study Related Instances, omitting the required
      response attributes in Table 10.6.3-3. Resolve available attributes from
      authorized metadata, implement the supported inclusion rules and report
      truncated search results correctly. Never broaden the authorized membership
      to satisfy a query. Test study/series/instance responses and inclusion rules.
      See [PS3.18 10.6.3.3](https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.6.3.3.html).
- [ ] Validate ascending frame lists. `gateway/path.rs::frames` checks positive
      bounded integers but currently accepts `2,1`. Define and test the handling
      of duplicate entries as well as decreasing, empty and oversized lists.
      See the `{frames}` definition in
      [PS3.18 10.1.1](https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_10.html).

### Requirements before a full origin-server claim

- [ ] Implement DICOM Retrieve Capabilities for any advertised origin-server
      Studies Service and list exact resources, media types and limitations.
      FHIR `/metadata` is a different capability mechanism. See
      [PS3.18 10.2](https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.2.html).
- [ ] Decide and implement a safe whole-study/series retrieval contract before
      claiming full WADO-RS origin-server support. Those binary routes are currently
      deliberately denied: a live PACS response could contain new, unattested
      instances. Supporting them must preserve the existing authorization boundary.
- [ ] Resolve partial thumbnail support. Instance/frame thumbnail routes exist;
      study/series thumbnail routes do not. An origin server supporting any
      thumbnail resource must support all of them. See
      [PS3.18 10.4.1.1.4](https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html).
- [ ] Publish a DICOM Conformance Statement covering the actual user-agent and
      gateway roles, supported transactions, SOP classes/transfer syntaxes,
      configuration, failure behavior and test evidence. UPS user agents also
      have conformance-statement requirements. See
      [PS3.18 11.2](https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_11.2.html).

### Additional scope, not implicit requirements of the chosen roles

- General UPS Search, Update and Performer Change State are absent from the
  production client. Create/Retrieve/Request Cancellation/Subscribe/Unsubscribe
  do not amount to UPS Pull. Search is optional for the Watch SCU; Performer
  ownership is intentionally external. See the
  [PS3.4 CC.2 role matrix](https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_CC.2.html).
- Permanent filtered-subscription retirement is an operator drain/removal
  procedure, not an automated Suspend/Unsubscribe workflow. Disconnecting the
  WebSocket is insufficient to release persistent deletion locks.
- STOW-RS upload, DIMSE services, SR-to-Observation interpretation, frame/segment
  subset output references and a general-purpose viewer are outside this delivery.
  Ordinary FHIR Task creation on an arbitrary server does not itself execute UPS;
  a worker supplies that behavior. The portable client demonstrates the standard
  FHIR/DICOMweb boundary and does not implement an autonomous external bridge.
- Broader CT/MR, enhanced multiframe, compressed transfer-syntax and viewer testing
  remains necessary before broader compatibility claims. Current real-PACS tests
  use a synthetic single-frame Secondary Capture object; real UPS coverage is one
  Task Manager plus an independent protocol peer. The unchanged portable FHIR R4
  scenario passes OctoFHIR and HAPI, not a two-server R4B/R5 conformance matrix.
- General Binary/Attachment S3 storage remains the explicitly deferred stage 6;
  it is separate from DICOM protocol conformance and PACS ownership of images.
