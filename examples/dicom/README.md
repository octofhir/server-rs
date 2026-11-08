# Standard FHIR + DICOMweb client

This example uses Python 3.11+ and its standard library. Its HTTP contract is
FHIR REST and DICOMweb, independent of a server product. The built-in OctoFHIR
operations remain available as an additional automation interface.

The FHIR server must support the resources/interactions used by the workflow:
Patient, Endpoint, Device, ImagingStudy, Task, DocumentReference, Provenance,
search, versioned updates and transaction Bundles. Supply the server's canonical
FHIR base URL and an appropriately scoped bearer token when authentication is
required. Obtain/refresh tokens through the deployment's authentication service.

## Read and publish ordinary FHIR resources

~~~bash
python3 examples/dicom/standard_fhir_client.py \
  --fhir-base https://fhir.example.org/fhir \
  --fhir-token-env FHIR_TOKEN capability

python3 examples/dicom/standard_fhir_client.py \
  --fhir-base https://fhir.example.org/fhir \
  --fhir-token-env FHIR_TOKEN studies patient-id

python3 examples/dicom/standard_fhir_client.py \
  --fhir-base https://fhir.example.org/fhir \
  --fhir-token-env FHIR_TOKEN publish-bundle resources.transaction.json

python3 examples/dicom/standard_fhir_client.py \
  --fhir-base https://fhir.example.org/fhir \
  --fhir-token-env FHIR_TOKEN task task-id
~~~

**publish-bundle** accepts a standard Bundle with type **transaction**. Its
producer supplies normal resource contents, full URLs and references; the client
does not implement a second DICOM-to-FHIR mapper. An external processor can
publish Endpoint/ImagingStudy metadata, create a requested Task, then write its
result references and Provenance through the same standard API.

The Python FhirClient also provides read, search, update and transaction methods.
Search follows same-base pagination links. Updates require an ETag and send
If-Match, requesting a representation and falling back to a read for an empty
successful update response. HTTP errors remain failures rather than successful
empty results. The example is bounded to 100 search pages, 100 transaction entries
and 64 MiB per HTTP payload.

## Retrieve through the resource's DICOMweb Endpoint

~~~bash
python3 examples/dicom/standard_fhir_client.py \
  --fhir-base https://fhir.example.org/fhir \
  --fhir-token-env FHIR_TOKEN \
  frame imaging-study-id \
  --series 1.2.3.4 --instance 1.2.3.4.5 --number 1 \
  --dicom-base https://pacs.example.org/dicom-web \
  --dicom-token-env PACS_TOKEN \
  --output frame.multipart
~~~

The client reads ImagingStudy and its Endpoint through FHIR, selects an instance
listed in the study and issues WADO-RS Retrieve Frames. It uses raw DICOM UIDs in
the DICOMweb path. The output is the original multipart HTTP representation,
including its MIME boundary. An existing output file is never overwritten; newly created frame files use private permissions (0600).

The PACS base URL is pinned explicitly and must match the selected WADO-RS
Endpoint. FHIR and PACS tokens are configured separately. The client refuses
redirects and links outside the configured base, verifies TLS normally, and
requires **--allow-http** for an HTTP deployment. A gateway can be used when its
advertised Endpoint and credential requirements match those settings.

## Work ownership

Creating a FHIR Task records a request; FHIR does not require every server to
execute it or to submit it to UPS. An authorized worker owns processing and
publishes its actual state/results. The test worker below retrieves a synthetic
frame and completes the Task. A clinical implementation supplies its real
processing policy and result mapping.

OctoFHIR's $dicom-import, $dicom-discover, $dicom-notify, $dicom-sync-status and
$dicom-ups continue to provide built-in automation. The standard client and test
scenario require none of these operations, private database tables, connector
ownership tags or StorageProvider resources.

## Check the HTTP boundary locally

~~~bash
python3 -m unittest discover -s examples/dicom -v
~~~

These tests exercise relative pagination, versioned references, redirect/cross-base
credential protection and independent FHIR/PACS authentication over local HTTP.

## Reproduce the same contract against two FHIR servers

Use isolated loopback servers: the scenario writes synthetic test resources.
The checked-in PACS fixture contains no clinical data. The dedicated interoperability compose file
uses a digest-pinned official image, an ephemeral H2 database and enabled request
validation.

~~~bash
docker compose -p octofhir-dicom-standard -f docker-compose.fhir-interop.yml \
  up -d

# Wait for both services, then upload the synthetic fixture to the test PACS.
curl --fail http://127.0.0.1:8042/system
curl --fail http://127.0.0.1:18080/fhir/metadata
curl --fail -X POST http://127.0.0.1:8042/instances \
  -H 'Content-Type: application/dicom' \
  --data-binary @crates/octofhir-server/tests/fixtures/dicom/synthetic-secondary-capture.dcm

python3 scripts/test-dicom-standard-api.py \
  --fhir-base http://127.0.0.1:18080/fhir \
  --dicom-base http://127.0.0.1:8042/dicom-web

# Run the identical scenario on an isolated OctoFHIR R4 deployment.
# Configure FHIR_TOKEN through that deployment's authentication service.
python3 scripts/test-dicom-standard-api.py \
  --fhir-base http://127.0.0.1:8888/fhir --fhir-token-env FHIR_TOKEN \
  --dicom-base http://127.0.0.1:8042/dicom-web

docker compose -p octofhir-dicom-standard -f docker-compose.fhir-interop.yml \
  down --volumes
~~~

Use ORTHANC_HTTP_PORT and HAPI_HTTP_PORT to change test ports. The same unchanged
client/scenario exercises transaction reference resolution, ordinary
Patient/ImagingStudy/Task links and search, ETag updates, actual WADO-RS frame
bytes, and a transaction containing Task output, DocumentReference and Provenance.
The synthetic payload is R4; another FHIR release must be checked against that
release's resource definitions and the target's CapabilityStatement.

References: [FHIR REST](https://hl7.org/fhir/R4/http.html),
[ImagingStudy](https://hl7.org/fhir/R4/imagingstudy.html),
[DICOM Studies Service](https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_10.html).
