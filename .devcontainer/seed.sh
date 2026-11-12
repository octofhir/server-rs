#!/usr/bin/env bash
# Loads one synthetic US Core patient, so the console has data to show.
set -euo pipefail

base="http://localhost:8888"
bundle="scripts/inferno/data/uscore_bundle_patient_355.json"

token=$(curl -fsS -X POST "$base/auth/token" \
  -d "grant_type=password&client_id=octofhir-ui&username=admin&password=admin123" \
  --data-urlencode "scope=user/*.cruds system/*.cruds" | jq -r .access_token)

curl -fsS -o /dev/null -X POST "$base/fhir" \
  -H "Authorization: Bearer $token" \
  -H "Content-Type: application/fhir+json" \
  --data-binary "@$bundle"

echo "OctoFHIR is ready: open port 8888, go to /ui and sign in as admin / admin123"
