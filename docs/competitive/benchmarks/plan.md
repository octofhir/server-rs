# Benchmark Plan

## Objective

Produce reproducible, apples-to-apples performance comparisons between OctoFHIR, HAPI FHIR, and Aidbox using identical datasets, scenarios, and hardware.

## Dataset

**Generator**: [Synthea](https://github.com/synthetichealth/synthea)

```bash
# Generate reproducible dataset with seed
synthea -p 1000 -s 42 --exporter.fhir.transaction_bundle true
```

**Expected output**:
- ~1,000 patients
- ~50,000-100,000 total resources (Patient, Observation, Encounter, Condition, Procedure, Medication, etc.)
- Transaction bundles for bulk loading

**Loading**:
- Convert Synthea output to FHIR transaction bundles
- Load via `POST /fhir` (transaction endpoint)
- Record load time as baseline metric

## Comparison Targets

| Server | Image | Notes |
|--------|-------|-------|
| OctoFHIR | Local build (debug for dev, release for benchmark) | Current `main` branch |
| HAPI FHIR | `hapiproject/hapi:latest` | Default JPA + PostgreSQL config |
| Aidbox | `healthsamurai/aidboxone` | Trial license, PostgreSQL backend |

## Environment

**Docker Compose setup** (all servers share identical infra):

```yaml
# All servers connect to the same PostgreSQL 16 instance
# Separate databases per server to avoid interference
# Same hardware, same network, same OS
```

**Hardware requirements**:
- Minimum: 4 CPU cores, 16 GB RAM
- Recommended: 8 CPU cores, 32 GB RAM
- SSD storage for PostgreSQL data directory

**Warm-up phase**:
- 60 seconds of warm-up traffic before measurement
- Ensures JIT compilation (JVM), cache population, connection pool warm-up

## Scenarios

### Scenario 1: Read by ID

```
GET /fhir/Patient/{id}
```
- **Purpose**: Baseline single-resource retrieval latency
- **VUs**: 10, 50, 100
- **Duration**: 60s per VU level
- **Expected**: <5ms p50, <20ms p95

### Scenario 2: Search Patient

```
GET /fhir/Patient?name=Smith&birthdate=gt1990-01-01
```
- **Purpose**: Multi-parameter search with string matching and date comparison
- **VUs**: 10, 50, 100
- **Duration**: 60s per VU level
- **Expected**: <50ms p50, <200ms p95

### Scenario 3: Search Observation

```
GET /fhir/Observation?patient={id}&code=http://loinc.org|8867-4
```
- **Purpose**: Reference search + token search (common clinical query)
- **VUs**: 10, 50, 100
- **Duration**: 60s per VU level
- **Expected**: <50ms p50, <200ms p95

### Scenario 4: Include/Revinclude

```
GET /fhir/Patient?_id={id}&_revinclude=Observation:patient
```
- **Purpose**: Multi-resource retrieval with joins (expensive operation)
- **VUs**: 10, 50
- **Duration**: 60s per VU level
- **Expected**: <100ms p50, <500ms p95

### Scenario 5: Transaction Bundle

```
POST /fhir
Content-Type: application/fhir+json
Body: Bundle with 10 resources (Patient + related resources)
```
- **Purpose**: Write throughput and transaction atomicity
- **VUs**: 10, 50
- **Duration**: 60s per VU level
- **Expected**: <200ms p50, <1000ms p95

### Scenario 6: Bulk Export

```
GET /fhir/$export
```
- **Purpose**: Bulk data export throughput
- **VUs**: 1 (serial, measure total time)
- **Metric**: Time to complete for 1000 patients, throughput in resources/sec
- **Note**: Only if supported by competitor; HAPI and Aidbox support this

## Metrics Collected

### Per-Request Metrics (k6)
| Metric | Description |
|--------|-------------|
| `http_req_duration` p50 | Median latency |
| `http_req_duration` p95 | 95th percentile latency |
| `http_req_duration` p99 | 99th percentile latency |
| `http_reqs` | Throughput (requests per second) |
| `http_req_failed` | Error rate |
| `iterations` | Completed iterations |

### System Metrics (collected via `docker stats` + Prometheus)
| Metric | Description |
|--------|-------------|
| CPU % | CPU utilization during benchmark |
| Memory RSS | Resident set size (MB) |
| DB connections | Active PostgreSQL connections |
| Disk I/O | Read/write bytes during benchmark |

### Database Metrics (pg_stat_statements)
| Metric | Description |
|--------|-------------|
| `mean_exec_time` | Average query execution time |
| `calls` | Total query invocations |
| `rows` | Rows returned per query |
| `shared_blks_hit` / `shared_blks_read` | Cache hit ratio |

## Tools

| Tool | Purpose | Location |
|------|---------|----------|
| k6 | Load generation and metrics | `k6/` (existing infrastructure) |
| pg_stat_statements | PostgreSQL query analysis | Enable in `postgresql.conf` |
| Prometheus | Server metrics scraping | OctoFHIR `/metrics` endpoint |
| `docker stats` | Container resource monitoring | CLI |
| Grafana (optional) | Dashboard for real-time visualization | Docker image |

## Execution Protocol

### 1. Setup Phase
```bash
# Start infrastructure
docker compose up -d postgres

# Start server under test
docker compose up -d <server>

# Wait for health check
curl --retry 30 --retry-delay 2 http://localhost:8888/healthz

# Load Synthea dataset
k6 run k6/seed/load-synthea.js

# Verify resource count
curl http://localhost:8888/fhir/Patient?_summary=count
```

### 2. Warm-up Phase
```bash
# 60-second warm-up with light traffic
k6 run --duration 60s --vus 10 k6/benchmarks/read.js
```

### 3. Measurement Phase
```bash
# Run each scenario sequentially
for scenario in read search-patient search-observation include transaction; do
  for vus in 10 50 100; do
    k6 run --duration 60s --vus $vus \
      --out json=results/${server}/${scenario}_${vus}vu.json \
      k6/benchmarks/${scenario}.js
  done
done
```

### 4. Cleanup Phase
```bash
# Export pg_stat_statements
psql -c "SELECT * FROM pg_stat_statements ORDER BY mean_exec_time DESC LIMIT 50"

# Export docker stats
docker stats --no-stream --format "table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}"

# Stop server
docker compose down
```

### 5. Repeat for Each Server
Run steps 1-4 for OctoFHIR, HAPI FHIR, and Aidbox.

## Reporting

### Output Format

Results published as:
1. **Markdown table** in `docs/competitive/benchmarks/results.md` (when available)
2. **CSV** for spreadsheet analysis
3. **Charts** (p50/p95/p99 bar charts per scenario)

### Table Template

| Scenario | VUs | OctoFHIR p50 | OctoFHIR p95 | HAPI p50 | HAPI p95 | Aidbox p50 | Aidbox p95 |
|----------|-----|-------------|-------------|---------|---------|-----------|-----------|
| Read by ID | 10 | — | — | — | — | — | — |
| Read by ID | 50 | — | — | — | — | — | — |
| Read by ID | 100 | — | — | — | — | — | — |
| Search Patient | 10 | — | — | — | — | — | — |
| ... | ... | ... | ... | ... | ... | ... | ... |

### Additional Comparisons

- **Cold start time**: Time from `docker start` to first successful `/healthz` response
- **Memory at idle**: RSS after startup with no traffic
- **Memory under load**: Peak RSS during 100 VU scenario
- **Docker image size**: Compressed image size comparison
