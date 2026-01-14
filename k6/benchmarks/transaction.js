/**
 * FHIR Transaction/Batch Benchmark
 *
 * Tests performance of FHIR transaction and batch bundles:
 * - Transaction bundles (all-or-nothing atomic operations)
 * - Batch bundles (independent operations)
 * - Various bundle sizes (10, 50, 100 entries)
 *
 * Target metrics:
 * - Transaction TPS: 50+ for 10-entry bundles
 * - Batch TPS: 100+ for 10-entry bundles
 * - p95 latency: <500ms for 10-entry bundles
 */

import http from 'k6/http'
import { check, group, sleep } from 'k6'
import { Counter, Trend } from 'k6/metrics'

import { headers, generateUUID } from '../util.js'
import patient from '../seed/patient.js'
import observation from '../seed/observation.js'
import practitioner from '../seed/practitioner.js'
import organization from '../seed/organization.js'

// Custom metrics
const transactionSuccess = new Counter('transaction_success')
const transactionFailure = new Counter('transaction_failure')
const batchSuccess = new Counter('batch_success')
const batchFailure = new Counter('batch_failure')
const transactionDuration = new Trend('transaction_duration_ms')
const batchDuration = new Trend('batch_duration_ms')

export const options = {
  discardResponseBodies: false,
  scenarios: {
    // Small bundles - high throughput test
    small_bundles: {
      executor: 'constant-vus',
      vus: 50,
      duration: '2m',
      gracefulStop: '30s',
      tags: { bundle_size: 'small' },
      env: { BUNDLE_SIZE: '10' },
    },
    // Medium bundles - balanced test
    medium_bundles: {
      executor: 'constant-vus',
      vus: 30,
      duration: '2m',
      startTime: '2m30s',
      gracefulStop: '30s',
      tags: { bundle_size: 'medium' },
      env: { BUNDLE_SIZE: '50' },
    },
    // Large bundles - stress test
    large_bundles: {
      executor: 'constant-vus',
      vus: 10,
      duration: '2m',
      startTime: '5m',
      gracefulStop: '30s',
      tags: { bundle_size: 'large' },
      env: { BUNDLE_SIZE: '100' },
    },
  },
  thresholds: {
    'transaction_duration_ms{bundle_size:small}': ['p(95)<500'],
    'transaction_duration_ms{bundle_size:medium}': ['p(95)<2000'],
    'transaction_duration_ms{bundle_size:large}': ['p(95)<5000'],
    'batch_duration_ms{bundle_size:small}': ['p(95)<400'],
    'batch_duration_ms{bundle_size:medium}': ['p(95)<1500'],
    'batch_duration_ms{bundle_size:large}': ['p(95)<4000'],
  },
}

// Create a patient resource with unique identifiers
function createPatientEntry(fullUrl) {
  const p = JSON.parse(JSON.stringify(patient))
  p.identifier[0].value = generateUUID()
  p.name[0].given[0] = `Test-${Date.now()}`
  return {
    fullUrl: fullUrl,
    resource: p,
    request: { method: 'POST', url: 'Patient' }
  }
}

// Create an observation resource referencing a patient
function createObservationEntry(patientRef) {
  const o = JSON.parse(JSON.stringify(observation))
  o.subject = { reference: patientRef }
  o.effectiveDateTime = new Date().toISOString()
  o.valueQuantity.value = Math.floor(Math.random() * 100) + 50
  return {
    resource: o,
    request: { method: 'POST', url: 'Observation' }
  }
}

// Create organization entry
function createOrganizationEntry() {
  const org = JSON.parse(JSON.stringify(organization))
  org.name = `Test Org ${Date.now()}-${Math.random().toString(36).substring(7)}`
  return {
    resource: org,
    request: { method: 'POST', url: 'Organization' }
  }
}

// Create practitioner entry
function createPractitionerEntry() {
  const prac = JSON.parse(JSON.stringify(practitioner))
  prac.name[0].given[0] = `Doc-${Math.random().toString(36).substring(7)}`
  return {
    resource: prac,
    request: { method: 'POST', url: 'Practitioner' }
  }
}

// Build a transaction bundle with inter-resource references
function buildTransactionBundle(size) {
  const entries = []
  const patientCount = Math.max(1, Math.ceil(size * 0.2))
  const observationCount = Math.ceil(size * 0.6)
  const orgCount = Math.ceil(size * 0.1)
  const pracCount = Math.max(0, size - patientCount - observationCount - orgCount)

  const patientUrns = []
  for (let i = 0; i < patientCount; i++) {
    const urn = `urn:uuid:${generateUUID()}`
    patientUrns.push(urn)
    entries.push(createPatientEntry(urn))
  }

  for (let i = 0; i < observationCount; i++) {
    const patientRef = patientUrns[i % patientUrns.length]
    entries.push(createObservationEntry(patientRef))
  }

  for (let i = 0; i < orgCount; i++) {
    entries.push(createOrganizationEntry())
  }

  for (let i = 0; i < pracCount; i++) {
    entries.push(createPractitionerEntry())
  }

  return {
    resourceType: 'Bundle',
    type: 'transaction',
    entry: entries
  }
}

// Build a batch bundle (no inter-resource references)
function buildBatchBundle(size) {
  const entries = []
  const patientCount = Math.ceil(size * 0.3)
  const orgCount = Math.ceil(size * 0.3)
  const pracCount = Math.max(0, size - patientCount - orgCount)

  for (let i = 0; i < patientCount; i++) {
    const entry = createPatientEntry(`urn:uuid:${generateUUID()}`)
    delete entry.fullUrl
    entries.push(entry)
  }

  for (let i = 0; i < orgCount; i++) {
    entries.push(createOrganizationEntry())
  }

  for (let i = 0; i < pracCount; i++) {
    entries.push(createPractitionerEntry())
  }

  return {
    resourceType: 'Bundle',
    type: 'batch',
    entry: entries
  }
}

export function setup() {
  return {
    baseUrl: __ENV.BASE_URL,
    params: { headers: headers() },
  }
}

export default function({ baseUrl, params }) {
  const bundleSize = parseInt(__ENV.BUNDLE_SIZE || '10')
  const sizeTag = bundleSize < 30 ? 'small' : bundleSize < 80 ? 'medium' : 'large'

  group('transaction', () => {
    const bundle = buildTransactionBundle(bundleSize)
    const start = Date.now()

    const res = http.post(
      `${baseUrl}`,
      JSON.stringify(bundle),
      { ...params, tags: { name: `transaction-${bundleSize}` } }
    )

    transactionDuration.add(Date.now() - start, { bundle_size: sizeTag })

    const success = check(res, {
      'transaction status 200': (r) => r.status === 200,
      'transaction returns bundle': (r) => {
        try {
          const body = r.json()
          return body.resourceType === 'Bundle' && body.type === 'transaction-response'
        } catch { return false }
      }
    })

    if (success) {
      transactionSuccess.add(1)
    } else {
      transactionFailure.add(1)
      if (res.status !== 200) {
        console.log(`Transaction failed: ${res.status}`)
      }
    }
  })

  sleep(0.1)

  group('batch', () => {
    const bundle = buildBatchBundle(bundleSize)
    const start = Date.now()

    const res = http.post(
      `${baseUrl}`,
      JSON.stringify(bundle),
      { ...params, tags: { name: `batch-${bundleSize}` } }
    )

    batchDuration.add(Date.now() - start, { bundle_size: sizeTag })

    const success = check(res, {
      'batch status 200': (r) => r.status === 200,
      'batch returns bundle': (r) => {
        try {
          const body = r.json()
          return body.resourceType === 'Bundle' && body.type === 'batch-response'
        } catch { return false }
      }
    })

    if (success) {
      batchSuccess.add(1)
    } else {
      batchFailure.add(1)
      if (res.status !== 200) {
        console.log(`Batch failed: ${res.status}`)
      }
    }
  })
}

export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'transaction-batch',
    metrics: {
      transaction: {
        success: data.metrics.transaction_success?.values?.count || 0,
        failure: data.metrics.transaction_failure?.values?.count || 0,
        duration_p50: data.metrics.transaction_duration_ms?.values?.['p(50)'] || 0,
        duration_p95: data.metrics.transaction_duration_ms?.values?.['p(95)'] || 0,
        duration_p99: data.metrics.transaction_duration_ms?.values?.['p(99)'] || 0,
      },
      batch: {
        success: data.metrics.batch_success?.values?.count || 0,
        failure: data.metrics.batch_failure?.values?.count || 0,
        duration_p50: data.metrics.batch_duration_ms?.values?.['p(50)'] || 0,
        duration_p95: data.metrics.batch_duration_ms?.values?.['p(95)'] || 0,
        duration_p99: data.metrics.batch_duration_ms?.values?.['p(99)'] || 0,
      }
    }
  }

  return {
    'benchmark-results/transaction.json': JSON.stringify(summary, null, 2),
  }
}
