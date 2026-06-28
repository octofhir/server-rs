#!/bin/sh
# Run native octofhir-server against the benchmark's docker postgres (perf_pgdata,
# already seeded). Mirrors docker-compose octofhir env. For local search profiling.
set -e
export OCTOFHIR__SERVER__HOST=127.0.0.1
export OCTOFHIR__SERVER__PORT=8888
export OCTOFHIR__SERVER__BASE_URL=http://127.0.0.1:8888
export OCTOFHIR__SERVER__BODY_LIMIT_BYTES=104857600
export OCTOFHIR__STORAGE__POSTGRES__HOST=127.0.0.1
export OCTOFHIR__STORAGE__POSTGRES__PORT=13020
export OCTOFHIR__STORAGE__POSTGRES__USER=postgres
export OCTOFHIR__STORAGE__POSTGRES__PASSWORD=postgres
export OCTOFHIR__STORAGE__POSTGRES__DATABASE=octofhir
export OCTOFHIR__STORAGE__POSTGRES__POOL_SIZE=32
export OCTOFHIR__SEARCH__INDEXED_PARAMS='Patient.name,Patient.address,Patient.birthdate,Organization.name,Observation.date,Encounter.date,Observation.category,Observation.code,Observation.value-quantity,Observation.subject,Observation.encounter,Observation.performer,Encounter.subject,Encounter.participant,MedicationRequest.subject,MedicationRequest.encounter,MedicationRequest.requester'
export OCTOFHIR__AUTH__POLICY__ANONYMOUS_ACCESS=true
export OCTOFHIR__AUTH__POLICY__DEFAULT_DENY=false
export OCTOFHIR__TERMINOLOGY__SERVER_URL=https://tx.health-samurai.io/fhir
export OCTOFHIR__AUDIT__ENABLED=false
export OCTOFHIR__LOGGING__LEVEL=info
export RUST_LOG=info
exec "$@"
