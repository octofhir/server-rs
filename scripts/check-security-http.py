#!/usr/bin/env python3
"""Run security regressions against a real local server and disposable PostgreSQL.

Requires initdb/pg_ctl/psql and a debug server build. No existing database is used.
FHIR archives are copied from .fhir/cache; the extensions package may also be
read from the user's installed FHIR package cache. Nothing is published.
"""
import argparse
import base64
import http.server
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tarfile
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def request_json(url, body=None, token=None, form=None, basic=None):
    headers = {"Accept": "application/json", "X-Skip-Validation": "true"}
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    if form is not None:
        data = urllib.parse.urlencode(form).encode()
        headers["Content-Type"] = "application/x-www-form-urlencoded"
    if token:
        headers["Authorization"] = "Bearer " + token
    if basic:
        headers["Authorization"] = "Basic " + base64.b64encode(basic.encode()).decode()
    request = urllib.request.Request(url, data=data, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=40) as response:
            return response.status, json.load(response)
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read())


def assert_status(label, actual, expected, body):
    assert actual == expected, f"{label}: expected {expected}, got {actual}: {body}"
    print(f"{label}: HTTP {actual}", flush=True)


def packages(path):
    cache = path / "packages/cache"
    cache.mkdir(parents=True)
    for name in ["hl7.fhir.r4.core-4.0.1.tgz", "hl7.terminology.r4-7.0.1.tgz"]:
        shutil.copyfile(ROOT / ".fhir/cache" / name, cache / name)
    extension = "hl7.fhir.uv.extensions.r4-5.2.0.tgz"
    if (ROOT / ".fhir/cache" / extension).exists():
        shutil.copyfile(ROOT / ".fhir/cache" / extension, cache / extension)
    else:
        installed = Path.home() / ".fhir/packages/hl7.fhir.uv.extensions.r4#5.2.0/package"
        with tarfile.open(cache / extension, "w:gz") as archive:
            archive.add(installed, arcname="package")


def scenario(binary):
    with tempfile.TemporaryDirectory(prefix="octofhir-http-security-") as tmp:
        path = Path(tmp)
        data = path / "postgres"
        pg_port, server_port = free_port(), free_port()
        server = None
        subprocess.run(["initdb", "-D", str(data), "-A", "trust", "-U", "audit", "--no-locale", "--encoding=UTF8"], check=True, capture_output=True)
        try:
            subprocess.run(["pg_ctl", "-D", str(data), "-l", str(path / "postgres.log"), "-o", f"-h 127.0.0.1 -p {pg_port} -k {path}", "-w", "start"], check=True, capture_output=True)
            pg = ["psql", "-h", "127.0.0.1", "-p", str(pg_port), "-U", "audit", "-d", "postgres", "-X", "-v", "ON_ERROR_STOP=1"]
            subprocess.run(pg + ["-c", "CREATE ROLE audit_runtime LOGIN; ALTER DATABASE postgres OWNER TO audit_runtime; GRANT ALL ON SCHEMA public TO audit_runtime"], check=True, capture_output=True)
            packages(path)
            config = path / "octofhir.toml"
            config.write_text(f"""[server]
host="127.0.0.1"
port={server_port}
allow_private_outbound=false
[storage.postgres]
url="postgres://audit_runtime@127.0.0.1:{pg_port}/postgres"
[packages]
path="{path}/packages"
load=[]
offline=true
[auth]
issuer="http://127.0.0.1:{server_port}"
[bootstrap.admin_user]
username="audit-admin"
password="isolated-admin-password"
[bootstrap.backend_client]
client_id="audit-backend"
client_secret="isolated-audit-secret"
scopes=["system/*.cruds","system/Patient.r"]
[db_console]
enabled=true
sql_mode="readwrite"
[validation]
allow_skip_validation=true
[otel]
enabled=false
""")
            env = {key: value for key, value in os.environ.items() if not key.startswith(("OCTOFHIR", "OTEL_"))}
            env["OCTOFHIR_CONFIG"] = str(config)
            env["RUST_LOG"] = "warn"
            log_path = path / "server.log"
            with log_path.open("w") as log:
                server = subprocess.Popen([str(binary)], cwd=path, env=env, stdout=log, stderr=log)
                base = f"http://127.0.0.1:{server_port}"
                # Current core archives contain three non-convertible legacy definitions.
                # After offline acquisition, ordinary startup reuses the test DB's packages.
                for _ in range(360):
                    if server.poll() is not None:
                        content = log_path.read_text()
                        if "unconverted StructureDefinitions" in content and "offline=true" in config.read_text():
                            config.write_text(config.read_text().replace("offline=true", "offline=false"))
                            server = subprocess.Popen([str(binary)], cwd=path, env=env, stdout=log, stderr=log)
                            continue
                        raise RuntimeError(content[-5000:])
                    try:
                        if request_json(base + "/healthz")[0] == 200:
                            break
                    except (OSError, ValueError):
                        time.sleep(0.5)
                else:
                    raise RuntimeError("Server readiness timeout: " + log_path.read_text()[-5000:])

                def token(scope="system/*.cruds", admin=False):
                    form = {"grant_type": "client_credentials", "scope": scope}
                    basic = "audit-backend:isolated-audit-secret"
                    if admin:
                        form = {"grant_type": "password", "client_id": "octofhir-ui", "username": "audit-admin", "password": "isolated-admin-password", "scope": "openid offline_access user/*.cruds system/*.cruds"}
                        basic = None
                    status, body = request_json(base + "/auth/token", form=form, basic=basic)
                    assert_status("OAuth admin" if admin else "OAuth backend", status, 200, body)
                    return body["access_token"]

                backend, admin = token(), token(admin=True)
                copy = {"query": "COPY (SELECT 'audit-marker') TO PROGRAM 'true'"}
                for label, query, credential, expected in [
                    ("Unauthenticated command", copy, None, 401),
                    ("Backend cannot use console", {"query": "SELECT 42"}, backend, 403),
                    ("Admin cannot execute PROGRAM", copy, admin, 403),
                    ("Admin cannot bypass with SELECT INTO", {"query": "SELECT 42 AS marker INTO audit_probe"}, admin, 403),
                    ("Ordinary SELECT still works", {"query": "SELECT 42 AS marker"}, admin, 200),
                    ("Multiple statements rejected", {"query": "SELECT 1; SELECT 2"}, admin, 403),
                ]:
                    status, body = request_json(base + "/api/$sql", query, credential)
                    assert_status(label, status, expected, body)
                    if expected == 200:
                        assert body["rows"] == [[42]], body

                subprocess.run(pg + ["-c", "ALTER ROLE audit_runtime SUPERUSER"], check=True, capture_output=True)
                status, body = request_json(base + "/api/$sql", {"query": "SELECT 42"}, admin)
                assert_status("Privileged DB connections refused", status, 403, body)
                subprocess.run(pg + ["-c", "ALTER ROLE audit_runtime NOSUPERUSER"], check=True, capture_output=True)

                received = []
                class Receiver(http.server.BaseHTTPRequestHandler):
                    def do_GET(self):
                        received.append(self.headers.get("Authorization"))
                        self.send_response(200)
                        self.send_header("Content-Type", "application/json")
                        self.end_headers()
                        self.wfile.write(b'{"audit":true}')
                    def log_message(self, *_args):
                        pass
                with http.server.HTTPServer(("127.0.0.1", 0), Receiver) as receiver:
                    receiver.timeout = 3
                    thread = threading.Thread(target=receiver.handle_request, daemon=True)
                    thread.start()
                    notebook = {"resourceType": "Notebook", "nbformat": 1, "title": "Security regression", "cells": [{"id": "rest", "type": "rest", "source": {"method": "GET", "url": f"http://127.0.0.1:{receiver.server_port}/audit"}}]}
                    status, body = request_json(base + "/fhir/Notebook", notebook, backend)
                    assert_status("Create test Notebook", status, 201, body)
                    notebook_id = body["id"]
                    status, body = request_json(base + f"/api/notebooks/{notebook_id}/run", {}, backend)
                    assert_status("Run test Notebook", status, 200, body)
                    assert body["cells"][0]["outputs"][0]["kind"] == "error", body
                    thread.join(timeout=5)
                    assert not received, "External cell received caller credentials"
                    print("Notebook SSRF and Authorization forwarding blocked", flush=True)
                    restricted = token("system/Patient.r")
                    for endpoint in [f"/api/notebooks/{notebook_id}/run", f"/api/notebooks/{notebook_id}/export"]:
                        status, body = request_json(base + endpoint, {} if endpoint.endswith("/run") else None, restricted)
                        assert_status("Notebook read grant enforced", status, 403, body)
                print("All HTTP security regressions passed", flush=True)
        finally:
            if server:
                server.terminate()
                try:
                    server.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()
            subprocess.run(["pg_ctl", "-D", str(data), "-m", "immediate", "-w", "stop"], capture_output=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/octofhir-server")
    arguments = parser.parse_args()
    scenario(arguments.binary.resolve())
