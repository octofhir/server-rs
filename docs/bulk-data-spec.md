# Bulk Data Access: what the server implements

Status: 2026-11-06. Checked against the IG source (`HL7/bulk-data`) and the published packages,
not from memory.

## Versions

| Version | Date | Status | Where |
|---|---|---|---|
| 1.0.0 (STU 1) | 2019-08-22 | trial use | the version CMS-0057-F names |
| 2.0.0 (STU 2) | 2021-11-26 | trial use | permitted by CMS-0057-F through SVAP |
| **3.0.0 (STU 3)** | 2025-12-11 | trial use, **published** | `hl7.org/fhir/uv/bulkdata/STU3` |
| **4.0.0-ballot (STU 4)** | 2026-08-07 | ballot, **not normative** | `hl7.org/fhir/uv/bulkdata/4.0.0-202609-ballot`; CI build is the same version |

`$bulk-submit` and `$bulk-submit-status` exist only in 4.0.0-ballot, at FMM 2. They add operations
and deprecate nothing in `$export`.

What CMS-0057-F requires: the Provider Access and Payer-to-Payer APIs use Bulk Data Access IG
v1.0.0 at 45 CFR 170.215(d)(1), and payers may use v2.0.0 through SVAP. Compliance dates are from
2027-01-01. The rule names neither `$bulk-submit` nor STU 3 or STU 4: `$export` is what it requires.

## Choosing a version

STU 4 removes parts of STU 3, so one endpoint cannot be exact for both.
`bulk_export.spec_version` selects the version, `stu3` by default.

| | `stu3` | `stu4` |
|---|---|---|
| Kick-off | GET and POST | POST only; GET is 405 |
| Repeated values | repeated or comma-delimited | repeated; a comma is 400 |
| `includeAssociatedData` | `code` (a Coding is accepted too) | `Coding` |
| Manifest messages | `error` | `outcome`, with `countSeverity` |
| `request` | present | absent |
| `manifestType`, `outputFormat`, `fileSize` | absent | present |

`bulk_submit` works with either. Its status manifest always has the STU 4 shape, the only one defined.

## `$export`

Yes: implemented. Part: see the note. No: not implemented. "Req" is the IG's strength.

| Requirement | Req | State | Code |
|---|---|---|---|
| Kick-off at system, Patient, Group | SHALL/SHOULD | yes | `operations/bulk/export.rs` |
| POST with Parameters | STU 3 MAY, STU 4 SHALL | yes | `export.rs` `parse_params` |
| GET | STU 3 SHALL, removed in STU 4 | yes, per version | `export.rs` `submit_export` |
| `_outputFormat` (3 NDJSON spellings) | SHALL | yes | `parse_params` |
| `_since` | SHALL | yes | `export_search_params` |
| `_until` | optional | yes | `export_search_params` |
| `_type` | optional | yes | `submit_export` |
| `_typeFilter`, repeats are OR | optional | yes | `export_search_params` |
| `_elements`, root only, mandatory kept, SUBSETTED | optional | yes | `ExportRun::project` |
| `patient` on Patient and Group, error for absent or non-member | optional | yes | `resolve_patients` |
| `includeAssociatedData` Latest, Relevant, least restrictive wins | optional | yes | `ExportRun::provenance_of` |
| Custom `_` values of `includeAssociatedData` | MAY | no, 400 | |
| Provenance of the compartment by default in Patient exports | SHALL | yes, follows exported resources | `execute_bulk_export` |
| `organizeOutputBy=Patient`, header, block in one file | optional | yes | `ExportRun::export_blocks` |
| `organizeOutputBy` other types | optional | no, 400; the IG still asks for feedback on them | `parse_params` |
| `allowPartialManifests`, `link`, 202 with files | optional | yes | `bulk/manifest.rs` |
| Unsupported input: error, `handling=lenient` continues and reports | SHOULD | yes | `parse_params`, `write_messages` |
| `separate-export-status`, `Preference-Applied`, `X-Export-Status` | MAY | yes | `submit::job_response`, `operations/router.rs` |
| 202 with absolute `Content-Location` | SHALL | yes | `router.rs` |
| In progress: 202, `X-Progress`, `Retry-After` | SHALL, MAY, SHOULD | yes | `handlers.rs` `async_job_status` |
| Too frequent polling: 429 with `Retry-After` | SHOULD | yes | `bulk::PollGate` |
| 429 when a client starts a second export | SHOULD if wanted | no, jobs queue | |
| Failure: 5XX, OperationOutcome, `application/fhir+json` | SHOULD | yes | `async_job_status` |
| `transient` issue code on poll failure | SHOULD | no | |
| Complete: 200, `application/json`, `Expires` | SHALL, SHOULD | yes | `async_job_status` |
| `transactionTime`, `requiresAccessToken`, `output` | SHALL | yes | `ManifestHeader::document` |
| `outputOrganizedBy` | SHALL when organized | yes | `document` |
| `deleted` with `_since` | SHOULD | part: system level without `_typeFilter`; history keeps no search values, and the IG lets a server omit the rest | `write_deleted` |
| DELETE: 202, then 404 with OperationOutcome | SHALL | yes | `async_job_cancel`, `async_job_status` |
| Files removed after DELETE | MAY | yes | `async_job_cancel` |
| Files: `application/fhir+ndjson`, token when `requiresAccessToken` | SHALL | yes | `bulk_export_file` |
| gzip when asked | SHALL allow | yes, with `server.compression` | `server.rs` |
| Capability URLs (`requiresAccessToken=false` on a protected server) | MAY | no | |
| CapabilityStatement, OperationDefinition | SHALL | yes | `handlers.rs`, `igs/octofhir-bulk` |
| `operation-not-supported` extension | SHOULD | no: all types and parameters work in bulk | |
| Group with `actual=false` and characteristic cohorts, Bulk Cohort API | optional | no | `operations/group.rs` |
| `_minimumFileSize`, `_maximumFileSize` | optional, async pattern only | no, 400 | |

`outputPeriod`, named in the task for this work, is in neither version.

## `$bulk-submit` (4.0.0-ballot)

| Requirement | Req | State | Code |
|---|---|---|---|
| POST with Parameters, 200, errors as OperationOutcome | SHALL | yes | `bulk/submit.rs` |
| `submitter` matches an agreed value | SHALL | yes, `bulk_submit.submitters` | `Submission::authorize` |
| `submissionId` unique per submitter | SHALL | yes, table constraint | `open_submission` |
| `submissionStatus` values, default `in-progress` | SHALL | yes | `SubmitRequest::parse` |
| No requests after `completed` or `stopped` | SHALL NOT | yes, 409 | `handle_system` |
| `manifestUrl`, `fhirBaseUrl` required with it | SHALL | yes | `SubmitRequest::parse` |
| `manifestUrl` unique within a submission | SHALL | yes | `handle_system` |
| Several manifests form one submission | SHALL | yes | `bulk_submission_manifests` |
| Follow `link` of a manifest | SHALL | yes | `read_manifests` |
| `stopped`: stop retrieving, delete what was processed | SHALL | yes; created is deleted, changed returns to its version, later changes stay and are reported | `take_back` |
| `replacesManifestUrl`, with or without a successor | SHALL | yes | `execute_bulk_submit` |
| `outputFormat` | | yes, NDJSON | `SubmitRequest::parse` |
| `fileRequestHeader` | SHALL | yes; cleared from the job when it ends | `forget_file_headers` |
| `oauthMetadataUrl` (fetch with own token) | SHALL if accepted | yes, as a SMART Backend Services client signing with the server key; needs `provider_client_id` on the submitter, otherwise refused with 400 | `ProviderAuth` |
| `fileEncryptionKey` (JWE) | SHALL if accepted | no, refused with 400 | `SubmitRequest::parse` |
| `metadata`, `import` | MAY | accepted, reported as ignored: only use-case IGs define them | `execute_bulk_submit` |
| Scope `system/bulk-submit` | SHALL | yes; other tokens write under their own grants | `authorize_caller` |
| Validation | | fhirschema per resource, as a normal write; references between files of one manifest count as present | `store_batch` |
| `deleted` files of a submitted manifest | not specified | not processed, reported as a warning | `read_manifests` |
| `$bulk-submit-status`: 202 and `Content-Location` | SHALL | yes | `BulkSubmitStatusOperation` |
| 202 with partial manifest, 200 once complete | MAY, SHALL | yes | `status_response` |
| `submissionId` at the root | SHALL | yes | `status_response` |
| `outcome` with `manifestUrl`, `url`, `countSeverity` | SHALL, MAY | yes | `write_outcome` |
| `operationoutcome-sourceResource` per resource | SHOULD | yes | `RejectedLine::outcome` |
| DELETE on the status URL | SHALL | yes | `async_job_cancel` |
| 429 to delay a new submission | SHOULD if wanted | no | |

## Results

Runs of 2026-11-06 against a local server on PostgreSQL 16 through a TLS proxy, with
`scripts/inferno/run-bulk.ts`.

| Suite | Result |
|---|---|
| Inferno Bulk Data v2.0.0 Export, server (kit 0.14.0): SMART Backend Services, TLS, Group, Patient, system | 48 of 48 passed |
| Inferno Bulk Data v4.0.0-ballot Submit, Data Consumer | 15 of 18 passed |
| Inferno US Core v5.0.1, FHIR API group | 296 passed, 5 failed, 105 skipped, 1 omitted |
| Inferno US Core v6.1.0, FHIR API group | 321 passed, 6 failed, 125 skipped, 1 omitted |

The kit has no export suite for STU 3 or STU 4, so those parts rest on `tests/bulk_data.rs` and
`scripts/test-bulk-data.ts`.

The 3 submit failures expect the manifest, file and token requests during the kit's wait step,
and this server fetches right after the submission; the server log and the stored data show all
three happened.

US Core failures: `ViewDefinition` in `rest.resource` is not in the R4 ResourceType value set
while SQL on FHIR is enabled; the fixture bundle has invalid data (a `stop` element in
`effectivePeriod`, a CodeSystem version); and the optional `intent` searches compare values
across resources the fixture does not have.

Writes run through the same transaction path as `$import`, with checkpoints, so an interrupted job
resumes. URLs pass the outbound policy (`egress.rs`).

Resources keep the sender's ids, as `$import` does. A submission can therefore change a local
resource with the same id, and `stopped` restores it.
