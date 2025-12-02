# k6 Tests for OctoFHIR Server

Performance and functional tests for the OctoFHIR FHIR server using [k6](https://k6.io/).

## Prerequisites

1. Install k6:
   ```bash
   # macOS
   brew install k6

   # Linux (Debian/Ubuntu)
   sudo gpg -k
   sudo gpg --no-default-keyring --keyring /usr/share/keyrings/k6-archive-keyring.gpg --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D69
   echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list
   sudo apt-get update
   sudo apt-get install k6
   ```

2. Start the OctoFHIR server:
   ```bash
   cargo run --release
   ```

## Directory Structure

```
k6/
├── README.md                      # This file
├── data/
│   └── patients.js                # Test data (valid/invalid Patient resources)
├── lib/
│   ├── config.js                  # Configuration and test scenarios
│   ├── fhir.js                    # FHIR utility functions
│   ├── fixtures.js                # Resource fixtures for all types
│   └── utils.js                   # General utility functions
├── tests/
│   ├── crud-operations.js         # Comprehensive CRUD for all resource types
│   ├── search-performance.js      # Search performance tests
│   ├── patient-crud.js            # Patient-specific CRUD tests
│   ├── patient-validation.js      # Validation/negative tests
│   └── patient-performance.js     # Legacy performance tests
├── scenarios/
│   └── stress-test.js             # Stress testing with load ramping
└── results/                       # Test results output (created on run)
```

## Running Tests

### Comprehensive CRUD Operations

Test CRUD operations across all resource types:

```bash
k6 run k6/tests/crud-operations.js
```

### Search Performance Tests

Test simple, complex, and chained searches:

```bash
k6 run k6/tests/search-performance.js
```

### Stress Testing

Run stress tests with gradual load increase:

```bash
k6 run k6/scenarios/stress-test.js
```

### Patient-Specific Tests

Test Patient CRUD operations:

```bash
k6 run k6/tests/patient-crud.js
```

### Validation Tests

Test server rejection of invalid resources:

```bash
k6 run k6/tests/patient-validation.js
```

### Performance Tests

Run with different scenarios:

```bash
# Smoke test (1 VU, 5 iterations) - quick sanity check
k6 run --env SCENARIO=smoke k6/tests/patient-performance.js

# Performance baseline (10 VUs, 1 minute)
k6 run --env SCENARIO=performance k6/tests/patient-performance.js

# Load test (ramp up to 20 VUs, sustain, ramp down)
k6 run --env SCENARIO=load k6/tests/patient-performance.js

# Stress test (ramp up to 100 VUs)
k6 run --env SCENARIO=stress k6/tests/patient-performance.js

# Spike test (sudden traffic spike to 100 VUs)
k6 run --env SCENARIO=spike k6/tests/patient-performance.js
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `BASE_URL` | Server base URL | `http://localhost:8888` |
| `AUTH_TOKEN` | Bearer token for auth | `null` |
| `TIMEOUT` | Request timeout | `30s` |
| `SCENARIO` | Performance scenario | `smoke` |

Example:
```bash
k6 run --env BASE_URL=http://fhir.example.com --env SCENARIO=load k6/tests/patient-performance.js
```

### Test Scenarios

| Scenario | VUs | Duration | Description |
|----------|-----|----------|-------------|
| `smoke` | 1 | 5 iterations | Quick sanity check |
| `functional` | 1 | 1 iteration | Full functional test |
| `performance` | 10 | 1 minute | Performance baseline |
| `load` | 0→20→0 | 3 minutes | Sustained load test |
| `stress` | 0→100→0 | 7 minutes | Find breaking point |
| `spike` | 5→100→5 | ~2 minutes | Traffic spike handling |
| `soak` | 10 | 30 minutes | Long-running stability |

## Test Data

### Valid Patients

Located in `data/patients.js`:
- `validPatients` - Array of 5 realistic Patient resources
- `minimalPatient` - Minimal valid Patient (just resourceType)
- `fullPatient` - Patient with all optional fields
- `generateRandomPatient()` - Generate random valid Patient

### Invalid Patients

Located in `data/patients.js`:
- `invalidPatients.missingResourceType`
- `invalidPatients.wrongResourceType`
- `invalidPatients.invalidGender`
- `invalidPatients.invalidBirthDate`
- `invalidPatients.emptyObject`
- And more...

## Expected Results

### Functional Tests

All checks should pass (green). Example output:
```
✓ Patient created (201)
✓ Patient has Location header
✓ Patient read (200)
✓ Patient has correct resourceType
✓ Patient updated (200)
✓ Patient deleted (204 or 200)
```

### Validation Tests

Invalid resources should be rejected (400 or 422):
```
✓ Missing resourceType rejected (400 or 422)
✓ Invalid gender rejected (400 or 422)
✓ Returns OperationOutcome
```

### Performance Thresholds

Default thresholds (can be customized):
- HTTP failures: < 5%
- Create latency p95: < 1000ms
- Read latency p95: < 500ms
- Update latency p95: < 1000ms
- Delete latency p95: < 500ms
- Search latency p95: < 1000ms
- Success rates: > 95%

## Interpreting Results

### Key Metrics

| Metric | Description |
|--------|-------------|
| `http_req_duration` | Total request duration |
| `http_req_failed` | Failure rate |
| `patient_*_latency` | Operation-specific latency |
| `patient_*_success` | Operation success rate |

### Output Example

```
╔══════════════════════════════════════════════════════════════╗
║                    K6 Performance Summary                     ║
╠══════════════════════════════════════════════════════════════╣
║  Scenario: performance                                        ║
║  Total Requests: 5420                                         ║
║  Failed Rate: 0.18%                                           ║
╠══════════════════════════════════════════════════════════════╣
║  Latency (p95):                                              ║
║    Create: 245.32ms                                          ║
║    Read:   12.45ms                                           ║
║    Update: 198.67ms                                          ║
║    Delete: 15.23ms                                           ║
║    Search: 89.12ms                                           ║
╚══════════════════════════════════════════════════════════════╝
```

## CI/CD Integration

### GitHub Actions Example

```yaml
- name: Run k6 smoke tests
  run: |
    k6 run --env BASE_URL=${{ env.FHIR_SERVER_URL }} \
           --env SCENARIO=smoke \
           k6/tests/patient-performance.js

- name: Run k6 functional tests
  run: k6 run k6/tests/patient-crud.js
```

### Output to JSON

```bash
k6 run --out json=results.json k6/tests/patient-performance.js
```

## Troubleshooting

### Server Not Reachable

```
ERRO Server not reachable at http://localhost:8888
```

Ensure the OctoFHIR server is running:
```bash
cargo run --release
```

### High Failure Rate

Check server logs for errors. Common causes:
- Database connection issues
- Resource exhaustion
- Validation errors

### Slow Performance

- Check database indexes
- Monitor server resources (CPU, memory)
- Review connection pool settings
