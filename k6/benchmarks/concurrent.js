/**
 * Concurrent Users Benchmark
 *
 * Tests server performance under various concurrent user loads:
 * - 10 VUs (light load)
 * - 50 VUs (moderate load)
 * - 100 VUs (heavy load)
 * - 300 VUs (stress test)
 *
 * Target metrics:
 * - p95 read latency: <20ms at 100 VUs
 * - p95 write latency: <50ms at 100 VUs
 * - Error rate: <0.1% at 300 VUs
 * - TPS: 500+ writes at 300 VUs
 */

import http from 'k6/http'
import { check, group, sleep } from 'k6'
import { Counter, Trend, Rate } from 'k6/metrics'

import { headers, generateUUID } from '../util.js'
import patient from '../seed/patient.js'

// Custom metrics
const readLatency = new Trend('read_latency_ms')
const writeLatency = new Trend('write_latency_ms')
const readErrors = new Counter('read_errors')
const writeErrors = new Counter('write_errors')
const successRate = new Rate('success_rate')

export const options = {
  discardResponseBodies: false,
  scenarios: {
    // Ramp up test: 10 -> 50 -> 100 -> 300 VUs
    ramp_up: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '30s', target: 10 },   // Warm up
        { duration: '1m', target: 10 },    // Light load
        { duration: '30s', target: 50 },   // Ramp to moderate
        { duration: '1m', target: 50 },    // Moderate load
        { duration: '30s', target: 100 },  // Ramp to heavy
        { duration: '1m', target: 100 },   // Heavy load
        { duration: '30s', target: 300 },  // Ramp to stress
        { duration: '2m', target: 300 },   // Stress test
        { duration: '30s', target: 0 },    // Ramp down
      ],
      gracefulStop: '30s',
    },
  },
  thresholds: {
    'read_latency_ms': ['p(95)<100', 'p(99)<200'],
    'write_latency_ms': ['p(95)<200', 'p(99)<500'],
    'success_rate': ['rate>0.99'],
    'http_req_failed': ['rate<0.01'],
  },
}

// Create patient with unique data
function createPatient() {
  const p = JSON.parse(JSON.stringify(patient))
  p.identifier[0].value = generateUUID()
  p.name[0].given[0] = `Load-${Date.now()}`
  p.name[0].family = `Test-${__VU}-${__ITER}`
  return p
}

export function setup() {
  const baseUrl = __ENV.BASE_URL
  const params = { headers: headers() }

  // Create a few patients for read tests
  const seedPatients = []
  for (let i = 0; i < 10; i++) {
    const p = createPatient()
    const res = http.post(`${baseUrl}/Patient`, JSON.stringify(p), params)
    if (res.status === 201) {
      seedPatients.push(res.json().id)
    }
  }

  console.log(`Setup: Created ${seedPatients.length} seed patients`)

  return {
    baseUrl,
    params,
    seedPatients,
  }
}

export default function({ baseUrl, params, seedPatients }) {
  // Mix of read (70%) and write (30%) operations
  const isRead = Math.random() < 0.7

  if (isRead && seedPatients.length > 0) {
    group('read', () => {
      const patientId = seedPatients[Math.floor(Math.random() * seedPatients.length)]
      const start = Date.now()

      const res = http.get(
        `${baseUrl}/Patient/${patientId}`,
        { ...params, tags: { operation: 'read' } }
      )

      readLatency.add(Date.now() - start)

      const success = check(res, {
        'read status 200': (r) => r.status === 200,
        'read returns patient': (r) => {
          try {
            return r.json().resourceType === 'Patient'
          } catch { return false }
        }
      })

      successRate.add(success)
      if (!success) readErrors.add(1)
    })
  } else {
    group('write', () => {
      const p = createPatient()
      const start = Date.now()

      const res = http.post(
        `${baseUrl}/Patient`,
        JSON.stringify(p),
        { ...params, tags: { operation: 'write' } }
      )

      writeLatency.add(Date.now() - start)

      const success = check(res, {
        'write status 201': (r) => r.status === 201,
        'write returns patient': (r) => {
          try {
            return r.json().resourceType === 'Patient'
          } catch { return false }
        }
      })

      successRate.add(success)
      if (!success) writeErrors.add(1)
    })
  }

  // Small think time between operations
  sleep(Math.random() * 0.1)
}

export function teardown(data) {
  // Cleanup: delete seed patients
  const { baseUrl, params, seedPatients } = data
  for (const id of seedPatients) {
    http.del(`${baseUrl}/Patient/${id}`, null, params)
  }
  console.log(`Teardown: Deleted ${seedPatients.length} seed patients`)
}

export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'concurrent-users',
    metrics: {
      read: {
        p50: data.metrics.read_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.read_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.read_latency_ms?.values?.['p(99)'] || 0,
        errors: data.metrics.read_errors?.values?.count || 0,
      },
      write: {
        p50: data.metrics.write_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.write_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.write_latency_ms?.values?.['p(99)'] || 0,
        errors: data.metrics.write_errors?.values?.count || 0,
      },
      overall: {
        success_rate: data.metrics.success_rate?.values?.rate || 0,
        total_requests: data.metrics.http_reqs?.values?.count || 0,
        requests_per_second: data.metrics.http_reqs?.values?.rate || 0,
      }
    }
  }

  return {
    'benchmark-results/concurrent.json': JSON.stringify(summary, null, 2),
  }
}
