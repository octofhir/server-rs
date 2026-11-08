"""HTTP boundary checks for the portable example; no server product is required."""

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from standard_fhir_client import DicomClient, FhirClient


class TransportContract(unittest.TestCase):
    def setUp(self):
        self.requests = []
        self.pacs_requests = []
        self.servers = []
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                owner.requests.append((self.path, self.headers.get("Authorization")))
                if self.path == "/fhir/redirect":
                    self.send_response(302)
                    self.send_header("Location", owner.pacs_base + "/sink")
                    self.end_headers()
                    return
                if self.path.startswith("/fhir/ImagingStudy"):
                    if "escape=1" in self.path:
                        following = owner.pacs_base + "/sink"
                    elif "page=2" in self.path:
                        following = None
                    else:
                        following = "?page=2"
                    body = {"resourceType": "Bundle", "type": "searchset",
                            "entry": [{"resource": {"resourceType": "ImagingStudy",
                                                   "id": "second" if "page=2" in self.path else "first"}}]}
                    if following:
                        body["link"] = [{"relation": "next", "url": following}]
                else:
                    body = {"resourceType": "Endpoint", "id": "ep", "meta": {"versionId": "opaque-v2"}}
                self.send_response(200)
                self.send_header("Content-Type", "application/fhir+json")
                self.send_header("ETag", 'W/"opaque-v2"')
                self.end_headers()
                self.wfile.write(json.dumps(body).encode())

        class PacsHandler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                owner.pacs_requests.append((self.path, self.headers.get("Authorization"),
                                            self.headers.get("Accept")))
                self.send_response(200)
                self.send_header("Content-Type", 'multipart/related; boundary="test"')
                self.end_headers()
                self.wfile.write(b"--test--\r\n")

        self.fhir_base = self.start(Handler) + "/fhir"
        self.pacs_base = self.start(PacsHandler)
        self.client = FhirClient(self.fhir_base, "fhir-test-token", allow_http=True)

    def start(self, handler):
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.servers.append((server, thread))
        return "http://127.0.0.1:" + str(server.server_port)

    def tearDown(self):
        for server, thread in self.servers:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

    def test_query_relative_pagination_and_versioned_references(self):
        values = list(self.client.search("ImagingStudy", _count=1))
        self.assertEqual([value["id"] for value in values], ["first", "second"])
        resource, etag = self.client.read(self.fhir_base + "/Endpoint/ep/_history/opaque-v2")
        self.assertEqual(resource["id"], "ep")
        self.assertEqual(etag, 'W/"opaque-v2"')
        self.assertEqual(self.requests[-1][0], "/fhir/Endpoint/ep/_history/opaque-v2")
        self.assertTrue(all(auth == "Bearer fhir-test-token" for _, auth in self.requests))

    def test_cross_base_paging_and_redirects_never_forward_credentials(self):
        with self.assertRaises(ValueError):
            list(self.client.search("ImagingStudy", escape=1))
        with self.assertRaises(RuntimeError):
            self.client.http.exchange("GET", self.fhir_base + "/redirect")
        self.assertEqual(self.pacs_requests, [])

    def test_dicom_credential_and_media_type_are_independent(self):
        dicom = DicomClient(self.pacs_base, "pacs-test-token", allow_http=True)
        dicom.frame("1.2", "1.3", "1.4", 1)
        path, auth, accept = self.pacs_requests[0]
        self.assertEqual(path, "/studies/1.2/series/1.3/instances/1.4/frames/1")
        self.assertEqual(auth, "Bearer pacs-test-token")
        self.assertIn("application/octet-stream", accept)
        self.assertEqual(self.requests, [])


if __name__ == "__main__":
    unittest.main()
