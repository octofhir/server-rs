# OctoFHIR Performance Benchmarks

This document describes the performance benchmarks for OctoFHIR and provides comparison with other FHIR servers.

## Target Metrics

| Metric | Target | Competitor Baseline |
|--------|--------|---------------------|
| Write TPS | 500+ | HAPI: 82-3,000 |
| Read p95 latency | <20ms | Google Healthcare API: <200ms |
| Memory footprint | <500MB idle | Java-based: 2-4GB |
| Bulk ingest | 10K res/sec | Comparable to Google |

## Running Benchmarks

### Prerequisites

- [k6](https://k6.io/docs/get-started/installation/) installed
- OctoFHIR server running locally
- PostgreSQL database running

### Quick Start

```bash
# Start dependencies
just db-up

# Start server (in another terminal)
cargo run --release

# Run all benchmarks
just bench-all

# Or run individual benchmarks
just bench-crud
just bench-search
just bench-transaction
just bench-concurrent
```

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `BASE_URL` | Yes | FHIR server base URL (e.g., `http://localhost:8888/fhir`) |
| `AUTH_USER` | Yes | Username for authentication |
| `AUTH_PASSWORD` | Yes | Password for authentication |
| `CLIENT_ID` | No | OAuth client ID (default: `k6-test`) |
| `CLIENT_SECRET` | No | OAuth client secret |

## Benchmark Scenarios

### 1. CRUD Operations (`k6/benchmarks/crud.js`)

Tests Create, Read, Update, Delete operations across multiple resource types (Patient, Observation, Organization, Practitioner).

**Configuration:**
- VUs: 100
- Duration: 3 minutes

**Thresholds:**
- Create p95: <100ms
- Read p95: <20ms
- Update p95: <100ms
- Delete p95: <100ms
- Success rate: >99%

```bash
just bench-crud
```

### 2. Search Operations (`k6/benchmarks/search.js`)

Tests various FHIR search operations:
- Simple searches (string, token, date, reference)
- Chained searches
- `_include` and `_revinclude`
- Pagination and sorting

**Configuration:**
- VUs: 30
- Duration: 3 minutes (after 3 min seed phase)

**Thresholds:**
- Simple search p95: <100ms
- Chained search p95: <300ms
- Include search p95: <400ms

```bash
just bench-search
```

### 3. Transaction/Batch (`k6/benchmarks/transaction.js`)

Tests FHIR transaction and batch bundle processing with various bundle sizes (10, 50, 100 entries).

**Configuration:**
- Small bundles (10): 50 VUs, 2 min
- Medium bundles (50): 30 VUs, 2 min
- Large bundles (100): 10 VUs, 2 min

**Thresholds:**
- Small bundle p95: <500ms
- Medium bundle p95: <2000ms
- Large bundle p95: <5000ms

```bash
just bench-transaction
```

### 4. Concurrent Users (`k6/benchmarks/concurrent.js`)

Tests server performance under various concurrent user loads with ramping VUs: 10 → 50 → 100 → 300.

**Configuration:**
- Total duration: ~8 minutes
- 70% read, 30% write mix

**Thresholds:**
- Read p95: <100ms
- Write p95: <200ms
- Success rate: >99%

```bash
just bench-concurrent
```

### 5. Bulk Data Export (`k6/benchmarks/bulk.js`)

Tests FHIR Bulk Data Access ($export) operations:
- System-level export
- Patient-level export
- Bulk data ingest rate

**Configuration:**
- Seed phase: 10 VUs, 100 iterations
- Export phase: 5 VUs, 3 iterations each

**Thresholds:**
- Ingest rate: >100 res/sec
- Export init p95: <5000ms

```bash
just bench-bulk
```

## Competitor Comparison

### HAPI FHIR

| Metric | HAPI JPA | OctoFHIR Target |
|--------|----------|-----------------|
| Write TPS | 82-3,000 (config dependent) | 500+ |
| Read latency | ~50-100ms | <20ms p95 |
| Memory (idle) | 2-4 GB (JVM heap) | <500 MB |
| Cold start | 30-60 sec | <5 sec |

Source: [HAPI FHIR Performance](https://hapifhir.io/hapi-fhir/docs/server_jpa/performance.html)

### Google Cloud Healthcare API

| Metric | Google | OctoFHIR Target |
|--------|--------|-----------------|
| Read latency SLA | <200ms | <20ms p95 |
| Write latency | Variable | <100ms p95 |
| Bulk import | ~10K res/sec | 10K res/sec |

### Azure API for FHIR

| Metric | Azure | OctoFHIR Target |
|--------|-------|-----------------|
| Read latency | ~100ms typical | <20ms p95 |
| Throughput | ~1000 req/sec | 500+ TPS |

## Hardware Requirements

### Minimum (Development)
- CPU: 4 cores
- Memory: 8 GB
- Storage: SSD recommended

### Recommended (Benchmarks)
- CPU: 8 cores
- Memory: 16 GB
- Storage: NVMe SSD

### CI Environment
- GitHub Actions runner (4 cores, 16 GB)

## CI/CD Integration

Benchmarks run automatically:
- **Nightly**: Full benchmark suite at 2 AM UTC
- **On release**: Full benchmark suite
- **Manual**: Via workflow dispatch

Results are stored as GitHub artifacts for 90 days.

### Performance Regression Detection

The CI pipeline checks for performance regressions:
- >10% increase in p95 latency triggers a warning
- >25% increase fails the build (configurable)

## Results Format

Benchmark results are saved as JSON files in `benchmark-results/`:

```json
{
  "timestamp": "2024-01-15T10:30:00Z",
  "test": "crud",
  "metrics": {
    "create": {
      "count": 15000,
      "p50": 12.5,
      "p95": 45.2,
      "p99": 89.1
    },
    "read": {
      "count": 15000,
      "p50": 3.2,
      "p95": 12.8,
      "p99": 28.5
    }
  }
}
```

## Profiling

For detailed performance analysis:

```bash
# Generate flamegraph
just flame

# Memory profiling (requires heaptrack)
heaptrack ./target/release/octofhir-server

# CPU profiling with perf
perf record -g ./target/release/octofhir-server
perf report
```

## Contributing

When adding new benchmarks:

1. Create test file in `k6/benchmarks/`
2. Add thresholds to `k6/config/benchmark.json`
3. Add command to `justfile`
4. Update this documentation
5. Add to CI workflow if needed

## References

- [k6 Documentation](https://k6.io/docs/)
- [HAPI FHIR Performance](https://hapifhir.io/hapi-fhir/docs/server_jpa/performance.html)
- [Smile CDR Benchmarks](https://www.smiledigitalhealth.com/our-blog/performance-scalability-benchmarks/)
- [FHIR Bulk Data Access](https://hl7.org/fhir/uv/bulkdata/)
