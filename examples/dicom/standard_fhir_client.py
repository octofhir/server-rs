#!/usr/bin/env python3
"""Vendor-neutral FHIR REST client and pinned DICOMweb frame reader.

Uses ordinary resources, search, versioned PUT and transaction Bundles.
FHIR and PACS credentials are separate; redirects and cross-base links are refused.
This is a small interoperable example, not a PACS/UPS workflow engine.
"""

import argparse
import json
import os
import re
import sys
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import unquote, urlencode, urljoin, urlsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener

MAX_BYTES = 64 * 1024 * 1024


class NoRedirects(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise RuntimeError("HTTP redirects require an explicitly configured base URL")


def base_url(value, allow_http=False):
    parsed = urlsplit(value)
    if (
        parsed.scheme not in ("https", "http")
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or (parsed.scheme == "http" and not allow_http)
        or any(segment in (".", "..") for segment in unquote(parsed.path).replace("\\", "/").split("/"))
        or any(char.isspace() for char in value)
    ):
        raise ValueError("Use an HTTP(S) base URL without credentials/query; HTTP needs --allow-http")
    return value.rstrip("/")


def resource_reference(value):
    parts = value.split("/")
    if (
        len(parts) != 2
        or re.fullmatch(r"[A-Z][A-Za-z0-9]*", parts[0]) is None
        or re.fullmatch(r"[A-Za-z0-9.-]{1,64}", parts[1]) is None
    ):
        raise ValueError("Expected a relative ResourceType/id reference")
    return value


def uid(value):
    if (
        not isinstance(value, str)
        or len(value) > 64
        or re.fullmatch(r"[0-9]+(?:\.[0-9]+)*", value) is None
        or any(len(part) > 1 and part.startswith("0") for part in value.split("."))
    ):
        raise ValueError("Expected a raw DICOM UID")
    return value


class Transport:
    def __init__(self, base, token=None, allow_http=False):
        self.base = base_url(base, allow_http)
        if token is not None and re.fullmatch(r"[A-Za-z0-9._~+/=-]+", token) is None:
            raise ValueError("Invalid bearer token")
        self.token = token
        self.opener = build_opener(ProxyHandler({}), NoRedirects())

    def exchange(self, method, url, body=None, headers=None, accept="application/fhir+json"):
        parsed = urlsplit(url)
        if (
            not (url == self.base or url.startswith(self.base + "/") or url.startswith(self.base + "?"))
            or parsed.fragment
            or any(segment in (".", "..") for segment in unquote(parsed.path).replace("\\", "/").split("/"))
        ):
            raise ValueError("Refusing a link outside the configured base URL")
        request_headers = {"Accept": accept}
        if self.token:
            request_headers["Authorization"] = "Bearer " + self.token
        request_headers.update(headers or {})
        if body is not None:
            body = json.dumps(body, separators=(",", ":")).encode()
            if len(body) > MAX_BYTES:
                raise ValueError("FHIR payload exceeds the example client's size bound")
            request_headers["Content-Type"] = "application/fhir+json"
        try:
            with self.opener.open(Request(url, data=body, headers=request_headers, method=method), timeout=30) as response:
                data = response.read(MAX_BYTES + 1)
                if len(data) > MAX_BYTES:
                    raise ValueError("Response exceeds the example client's size bound")
                return data, response.headers
        except HTTPError as error:
            # Do not echo server bodies or request credentials into console logs.
            raise RuntimeError(f"Remote HTTP request failed with status {error.code}") from None


class FhirClient:
    def __init__(self, base, token=None, allow_http=False):
        self.http = Transport(base, token, allow_http)

    def _json(self, method, path="", body=None, headers=None):
        url = self.http.base + ("/" + path if path else "")
        data, response_headers = self.http.exchange(method, url, body, headers)
        if not data and method == "PUT":
            return self._json("GET", path)
        resource = json.loads(data)
        if not isinstance(resource, dict) or "resourceType" not in resource:
            raise ValueError("Expected a FHIR resource response")
        return resource, response_headers

    def capability(self):
        resource, _ = self._json("GET", "metadata")
        if resource["resourceType"] != "CapabilityStatement":
            raise ValueError("Expected CapabilityStatement")
        return resource

    def _reference_path(self, location):
        if urlsplit(location).scheme or location.startswith("/"):
            absolute = urljoin(self.http.base + "/", location)
            if not absolute.startswith(self.http.base + "/"):
                raise ValueError("Refusing a resource reference outside the configured FHIR base")
            location = absolute[len(self.http.base) + 1:]
        parts = location.split("/")
        if len(parts) == 4 and parts[2] == "_history":
            resource_reference("Version/" + parts[3])
            resource_reference("/".join(parts[:2]))
            return location
        return resource_reference(location)

    def reference(self, location):
        return "/".join(self._reference_path(location).split("/")[:2])

    def read(self, reference):
        resource, headers = self._json("GET", self._reference_path(reference))
        return resource, headers.get("ETag")

    def update(self, resource, etag):
        if not etag:
            raise ValueError("A current ETag is required for versioned updates")
        reference = resource_reference(resource["resourceType"] + "/" + resource["id"])
        return self._json("PUT", reference, resource, {"If-Match": etag, "Prefer": "return=representation"})[0]

    def transaction(self, entries):
        if not isinstance(entries, list) or not 1 <= len(entries) <= 100:
            raise ValueError("Expected 1..100 transaction entries")
        bundle, _ = self._json("POST", body={"resourceType": "Bundle", "type": "transaction", "entry": entries})
        if bundle.get("type") != "transaction-response":
            raise ValueError("Expected a FHIR transaction-response Bundle")
        if len(bundle.get("entry", [])) != len(entries):
            raise ValueError("Transaction response entry count differs from the request")
        for entry in bundle.get("entry", []):
            if not str(entry.get("response", {}).get("status", "")).startswith("2"):
                raise RuntimeError("A transaction entry failed")
        return bundle

    def search(self, resource_type, **parameters):
        resource_reference(resource_type + "/validation")
        url = self.http.base + "/" + resource_type + "?" + urlencode(parameters)
        for _ in range(100):
            data, _ = self.http.exchange("GET", url)
            bundle = json.loads(data)
            if bundle.get("resourceType") != "Bundle" or bundle.get("type") != "searchset":
                raise ValueError("Expected a FHIR searchset Bundle")
            for entry in bundle.get("entry", []):
                resource = entry.get("resource")
                if resource and entry.get("search", {}).get("mode", "match") == "match":
                    yield resource
            following = next((link["url"] for link in bundle.get("link", []) if link.get("relation") == "next"), None)
            if not following:
                return
            # Resolve standard URI references; Transport still pins the FHIR base.
            url = urljoin(url, following)
        raise RuntimeError("FHIR search exceeded 100 pages")

    def frame(self, study_id, series_uid, instance_uid, frame_number, dicom):
        study, _ = self.read("ImagingStudy/" + study_id)
        study_uids = {
            item["value"].removeprefix("urn:oid:")
            for item in study.get("identifier", [])
            if item.get("system") == "urn:dicom:uid" and isinstance(item.get("value"), str)
        }
        if len(study_uids) != 1:
            raise ValueError("ImagingStudy must identify exactly one DICOM study")
        series_uid, instance_uid = uid(series_uid), uid(instance_uid)
        series = next((item for item in study.get("series", []) if item.get("uid") == series_uid), None)
        if not series or not any(item.get("uid") == instance_uid for item in series.get("instance", [])):
            raise ValueError("Select an instance listed in the authorized ImagingStudy")
        for reference in series.get("endpoint") or study.get("endpoint", []):
            endpoint, _ = self.read(reference["reference"])
            connections = endpoint.get("connectionType", [])
            if isinstance(connections, dict):  # R4/R4B; R5 uses repetitions.
                connections = [connections]
            wado = any(item.get("code") == "dicom-wado-rs" for item in connections)
            if wado and endpoint.get("address", "").rstrip("/") == dicom.http.base:
                return dicom.frame(next(iter(study_uids)), series_uid, instance_uid, frame_number)
        raise ValueError("No WADO-RS Endpoint matches the explicitly pinned DICOM base URL")


class DicomClient:
    def __init__(self, base, token=None, allow_http=False):
        self.http = Transport(base, token, allow_http)

    def frame(self, study, series, instance, number):
        if not isinstance(number, int) or not 1 <= number <= 2147483647:
            raise ValueError("Frame numbers are positive and one-based")
        path = f"/studies/{uid(study)}/series/{uid(series)}/instances/{uid(instance)}/frames/{number}"
        data, headers = self.http.exchange(
            "GET", self.http.base + path,
            accept='multipart/related; type="application/octet-stream"',
        )
        return data, headers.get("Content-Type", "")


def token_from_env(name):
    if name is None:
        return None
    value = os.environ.get(name)
    if not value:
        raise ValueError("The configured token environment variable is empty")
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fhir-base", required=True)
    parser.add_argument("--fhir-token-env")
    parser.add_argument("--allow-http", action="store_true")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("capability")
    publish = commands.add_parser("publish-bundle")
    publish.add_argument("file", type=Path)
    studies = commands.add_parser("studies")
    studies.add_argument("patient_id")
    task = commands.add_parser("task")
    task.add_argument("task_id")
    frame = commands.add_parser("frame")
    frame.add_argument("study_id")
    frame.add_argument("--series", required=True)
    frame.add_argument("--instance", required=True)
    frame.add_argument("--number", type=int, default=1)
    frame.add_argument("--dicom-base", required=True)
    frame.add_argument("--dicom-token-env")
    frame.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    client = FhirClient(args.fhir_base, token_from_env(args.fhir_token_env), args.allow_http)
    if args.command == "capability":
        result = client.capability()
    elif args.command == "publish-bundle":
        if args.file.stat().st_size > MAX_BYTES:
            raise ValueError("Input Bundle exceeds the size bound")
        bundle = json.loads(args.file.read_text())
        if bundle.get("resourceType") != "Bundle" or bundle.get("type") != "transaction":
            raise ValueError("Expected a transaction Bundle")
        result = client.transaction(bundle.get("entry"))
    elif args.command == "studies":
        resource_reference("Patient/" + args.patient_id)
        result = list(client.search("ImagingStudy", patient="Patient/" + args.patient_id, _count=100))
    elif args.command == "task":
        result = client.read("Task/" + args.task_id)[0]
    else:
        dicom = DicomClient(args.dicom_base, token_from_env(args.dicom_token_env), args.allow_http)
        data, content_type = client.frame(args.study_id, args.series, args.instance, args.number, dicom)
        descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as output:
            output.write(data)
        result = {"file": str(args.output), "contentType": content_type, "bytes": len(data)}
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, ValueError, OSError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
