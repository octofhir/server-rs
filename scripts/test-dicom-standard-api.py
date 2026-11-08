#!/usr/bin/env python3
"""Exercise the same FHIR/DICOMweb client against an isolated R4 server.

Writes synthetic resources through standard FHIR REST only. Requires the synthetic
Secondary Capture fixture already loaded into the explicitly selected loopback PACS.
Run once per server; no OctoFHIR operations, storage tables or ownership tags are used.
"""

import argparse
import copy
import importlib.util
import json
import os
import time
import uuid
from datetime import datetime, timezone
from email import policy
from email.parser import BytesParser
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("standard_fhir_client", ROOT / "examples/dicom/standard_fhir_client.py")
CLIENT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLIENT)

STUDY = "1.2.826.0.1.3680043.10.999.1"
SERIES = "1.2.826.0.1.3680043.10.999.1.1"
INSTANCE = "1.2.826.0.1.3680043.10.999.1.1.1"
SOP_CLASS = "1.2.840.10008.5.1.4.1.1.7"


def narrative(kind):
    return {"status": "generated", "div": '<div xmlns="http://www.w3.org/1999/xhtml">Synthetic ' + kind + ' for interoperability testing. No clinical data.</div>'}


def full_url():
    return "urn:uuid:" + str(uuid.uuid4())


def wait_search(client, resource_type, predicate, **parameters):
    for _ in range(60):
        resources = list(client.search(resource_type, **parameters))
        if predicate(resources):
            return resources
        time.sleep(0.25)
    raise AssertionError("FHIR search did not converge")


def scenario(base, token, dicom_base):
    for url in [base, dicom_base]:
        assert urlsplit(url).hostname in ("127.0.0.1", "localhost"), "Use isolated loopback servers"
    client = CLIENT.FhirClient(base, token=token, allow_http=True)
    dicom = CLIENT.DicomClient(dicom_base, allow_http=True)
    capability = client.capability()
    assert capability["fhirVersion"].startswith("4."), "This synthetic workflow fixture is FHIR R4"
    assert any(
        item.get("code") == "transaction"
        for rest in capability.get("rest", [])
        for item in rest.get("interaction", [])
    ), "FHIR transactions are required"

    patient_url, endpoint_url, device_url, study_url, task_url = [full_url() for _ in range(5)]
    marker = str(uuid.uuid4())
    resources = [
        (patient_url, {"resourceType": "Patient", "active": True,
                       "identifier": [{"system": "urn:example:dicom-interop", "value": marker},
                                      {"system": "urn:example:hospital:mrn", "value": "OCTOFHIR-DICOM-TEST"}]}),
        (endpoint_url, {"resourceType": "Endpoint", "status": "active",
                        "connectionType": {"system": "http://terminology.hl7.org/CodeSystem/endpoint-connection-type",
                                           "code": "dicom-wado-rs"},
                        "name": "Synthetic test PACS", "payloadType": [{"text": "DICOM instances"}],
                        "address": dicom_base}),
        (device_url, {"resourceType": "Device", "status": "active",
                      "deviceName": [{"name": "Portable DICOM test worker", "type": "user-friendly-name"}]}),
        (study_url, {"resourceType": "ImagingStudy", "status": "available",
                     "identifier": [{"system": "urn:dicom:uid", "value": "urn:oid:" + STUDY}],
                     "subject": {"reference": patient_url}, "endpoint": [{"reference": endpoint_url}],
                     "numberOfSeries": 1, "numberOfInstances": 1,
                     "modality": [{"system": "http://dicom.nema.org/resources/ontology/DCM", "code": "OT"}],
                     "series": [{"uid": SERIES,
                                 "modality": {"system": "http://dicom.nema.org/resources/ontology/DCM", "code": "OT"},
                                 "numberOfInstances": 1,
                                 "instance": [{"uid": INSTANCE,
                                               "sopClass": {"system": "urn:ietf:rfc:3986",
                                                            "code": "urn:oid:" + SOP_CLASS}}]}]}),
        (task_url, {"resourceType": "Task", "status": "requested", "intent": "order",
                    "code": {"text": "Review the synthetic study"},
                    "for": {"reference": patient_url}, "focus": {"reference": study_url},
                    "owner": {"reference": device_url}}),
    ]
    for _, resource in resources:
        resource["text"] = narrative(resource["resourceType"])
    bundle = client.transaction([
        {"fullUrl": url, "resource": resource,
         "request": {"method": "POST", "url": resource["resourceType"]}}
        for url, resource in resources
    ])
    references = {
        resource["resourceType"]: client.reference(entry["response"]["location"])
        for (_, resource), entry in zip(resources, bundle["entry"], strict=True)
    }
    patient_id = references["Patient"].split("/")[1]
    studies = wait_search(client, "ImagingStudy",
                          lambda values: any(item["id"] == references["ImagingStudy"].split("/")[1] for item in values),
                          patient=references["Patient"], _count=1)
    assert all(client.reference(item["subject"]["reference"]) == references["Patient"] for item in studies)
    tasks = wait_search(client, "Task",
                        lambda values: any(item["id"] == references["Task"].split("/")[1] for item in values),
                        patient="Patient/" + patient_id, status="requested", _count=1)
    assert any(client.reference(item["focus"]["reference"]) == references["ImagingStudy"] for item in tasks)

    task, etag = client.read(references["Task"])
    assert client.reference(task["owner"]["reference"]) == references["Device"]
    for state in ["accepted", "in-progress"]:
        task["status"] = state
        client.update(task, etag)
        task, etag = client.read(references["Task"])
        assert task["status"] == state

    frame, content_type = client.frame(references["ImagingStudy"].split("/")[1], SERIES, INSTANCE, 1, dicom)
    message = BytesParser(policy=policy.default).parsebytes(
        ("MIME-Version: 1.0\r\nContent-Type: " + content_type + "\r\n\r\n").encode() + frame
    )
    assert message.is_multipart(), "Expected WADO-RS multipart frame response"
    parts = list(message.iter_parts())
    assert len(parts) == 1 and parts[0].get_payload(decode=True) == bytes(range(256))

    document_url, provenance_url = full_url(), full_url()
    document = {
        "resourceType": "DocumentReference", "status": "current", "text": narrative("DocumentReference"),
        "subject": {"reference": references["Patient"]},
        "context": {"related": [{"reference": references["ImagingStudy"]}]},
        "content": [{"attachment": {
            "contentType": "application/dicom",
            "url": f"{dicom_base}/studies/{STUDY}/series/{SERIES}/instances/{INSTANCE}",
            "title": "Synthetic Secondary Capture",
        }}],
    }
    completed = copy.deepcopy(task)
    completed["status"] = "completed"
    completed["output"] = [{"type": {"text": "DICOM result"}, "valueReference": {"reference": document_url}}]
    completed.pop("meta", None)
    completed_bundle = client.transaction([
        {"fullUrl": document_url, "resource": document, "request": {"method": "POST", "url": "DocumentReference"}},
        {"fullUrl": client.http.base + "/" + references["Task"], "resource": completed,
         "request": {"method": "PUT", "url": references["Task"], "ifMatch": etag}},
        {"fullUrl": provenance_url, "resource": {
            "resourceType": "Provenance", "text": narrative("Provenance"), "recorded": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "target": [{"reference": document_url}, {"reference": references["Task"]}],
            "agent": [{"who": {"reference": references["Device"]}}],
        }, "request": {"method": "POST", "url": "Provenance"}},
    ])
    document_ref = client.reference(completed_bundle["entry"][0]["response"]["location"])
    result_task, _ = client.read(references["Task"])
    assert result_task["status"] == "completed"
    assert client.reference(result_task["output"][0]["valueReference"]["reference"]) == document_ref
    stored_document, _ = client.read(document_ref)
    assert stored_document["content"][0]["attachment"]["url"].startswith(dicom_base + "/studies/")
    assert "data" not in stored_document["content"][0]["attachment"], "DICOM bytes must stay in PACS"
    print(json.dumps({
        "fhirVersion": capability["fhirVersion"],
        "software": capability.get("software", {}).get("name"),
        "checks": ["transaction references", "standard patient/study/task search", "ETag updates",
                   "Endpoint to WADO-RS frame bytes", "Task result + DocumentReference + Provenance transaction"],
        "customOperations": False,
    }))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fhir-base", required=True)
    parser.add_argument("--fhir-token-env")
    parser.add_argument("--dicom-base", required=True)
    arguments = parser.parse_args()
    scenario(arguments.fhir_base, CLIENT.token_from_env(arguments.fhir_token_env), arguments.dicom_base)
