/**
 * Bulk Data Export Benchmark
 *
 * Tests FHIR Bulk Data Access ($export) performance:
 * - System-level export (/$export)
 * - Patient-level export (/Patient/$export)
 * - Group-level export (/Group/{id}/$export)
 *
 * Target metrics:
 * - Bulk ingest: 10K resources/sec
 * - Export throughput: comparable to Google Cloud Healthcare API
 */

import http from 'k6/http'
import { check, group, sleep } from 'k6'
import { Counter, Trend, Rate } from 'k6/metrics'

import { headers, generateUUID } from '../util.js'
import patient from '../seed/patient.js'
import observation from '../seed/observation.js'

// Custom metrics
const exportInitTime = new Trend('export_init_time_ms')
const exportPollTime = new Trend('export_poll_time_ms')
const bulkIngestRate = new Trend('bulk_ingest_resources_per_sec')
const exportSuccess = new Counter('export_success')
const exportFailure = new Counter('export_failure')

export const options = {
  discardResponseBodies: false,
  scenarios: {
    // Seed data first
    seed_data: {
      executor: 'shared-iterations',
      vus: 10,
      iterations: 100,
      maxDuration: '5m',
      exec: 'seedData',
      tags: { phase: 'seed' },
    },
    // Then run export tests
    export_tests: {
      executor: 'per-vu-iterations',
      vus: 5,
      iterations: 3,
      startTime: '5m30s',
      maxDuration: '10m',
      exec: 'exportTest',
      tags: { phase: 'export' },
    },
  },
  thresholds: {
    'export_init_time_ms': ['p(95)<5000'],
    'bulk_ingest_resources_per_sec': ['avg>100'],
  },
}

// Create batch of patients with observations
function createBatchBundle(size) {
  const entries = []
  const patientCount = Math.ceil(size * 0.3)
  const observationCount = size - patientCount

  const patientUrns = []
  for (let i = 0; i < patientCount; i++) {
    const urn = `urn:uuid:${generateUUID()}`
    patientUrns.push(urn)
    const p = JSON.parse(JSON.stringify(patient))
    p.identifier[0].value = generateUUID()
    p.name[0].given[0] = `Bulk-${Date.now()}-${i}`
    entries.push({
      fullUrl: urn,
      resource: p,
      request: { method: 'POST', url: 'Patient' }
    })
  }

  for (let i = 0; i < observationCount; i++) {
    const o = JSON.parse(JSON.stringify(observation))
    o.subject = { reference: patientUrns[i % patientUrns.length] }
    o.effectiveDateTime = new Date().toISOString()
    o.valueQuantity.value = Math.floor(Math.random() * 100) + 50
    entries.push({
      resource: o,
      request: { method: 'POST', url: 'Observation' }
    })
  }

  return {
    resourceType: 'Bundle',
    type: 'transaction',
    entry: entries
  }
}

export function setup() {
  return {
    baseUrl: __ENV.BASE_URL,
    params: { headers: headers() },
  }
}

// Seed data phase - bulk insert resources
export function seedData({ baseUrl, params }) {
  const batchSize = 100
  const bundle = createBatchBundle(batchSize)

  const start = Date.now()
  const res = http.post(
    `${baseUrl}`,
    JSON.stringify(bundle),
    { ...params, tags: { operation: 'bulk-insert' } }
  )
  const duration = Date.now() - start

  const success = check(res, {
    'batch insert 200': (r) => r.status === 200,
  })

  if (success) {
    const resourcesPerSec = (batchSize / duration) * 1000
    bulkIngestRate.add(resourcesPerSec)
  }

  sleep(0.5)
}

// Export test phase
export function exportTest({ baseUrl, params }) {
  const exportParams = {
    ...params,
    headers: {
      ...params.headers,
      'Accept': 'application/fhir+json',
      'Prefer': 'respond-async',
    }
  }

  group('system-export', () => {
    const start = Date.now()

    // Initiate export
    const initRes = http.get(
      `${baseUrl}/$export?_type=Patient,Observation`,
      { ...exportParams, tags: { operation: 'export-init' } }
    )

    exportInitTime.add(Date.now() - start)

    // Check if async export started (202) or sync export completed (200)
    const initiated = check(initRes, {
      'export initiated': (r) => r.status === 202 || r.status === 200,
    })

    if (!initiated) {
      exportFailure.add(1)
      console.log(`Export init failed: ${initRes.status} - ${initRes.body?.substring(0, 200)}`)
      return
    }

    // If async (202), poll for completion
    if (initRes.status === 202) {
      const contentLocation = initRes.headers['Content-Location']
      if (!contentLocation) {
        exportFailure.add(1)
        console.log('No Content-Location header in 202 response')
        return
      }

      // Poll for completion (max 60 seconds)
      let completed = false
      const pollStart = Date.now()
      for (let i = 0; i < 30 && !completed; i++) {
        sleep(2)
        const pollRes = http.get(contentLocation, exportParams)

        if (pollRes.status === 200) {
          completed = true
          exportPollTime.add(Date.now() - pollStart)

          const body = pollRes.json()
          check(body, {
            'export has output': (b) => b.output && b.output.length > 0,
          })

          exportSuccess.add(1)
        } else if (pollRes.status !== 202) {
          // Error state
          exportFailure.add(1)
          console.log(`Export poll failed: ${pollRes.status}`)
          break
        }
      }

      if (!completed) {
        exportFailure.add(1)
        console.log('Export timed out')
      }
    } else {
      // Sync export completed immediately
      exportSuccess.add(1)
    }
  })

  group('patient-export', () => {
    const start = Date.now()

    const res = http.get(
      `${baseUrl}/Patient/$export`,
      { ...exportParams, tags: { operation: 'patient-export' } }
    )

    exportInitTime.add(Date.now() - start)

    check(res, {
      'patient export initiated': (r) => r.status === 202 || r.status === 200,
    })
  })
}

export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'bulk-export',
    metrics: {
      ingest: {
        resources_per_sec_avg: data.metrics.bulk_ingest_resources_per_sec?.values?.avg || 0,
        resources_per_sec_max: data.metrics.bulk_ingest_resources_per_sec?.values?.max || 0,
      },
      export: {
        init_time_p50: data.metrics.export_init_time_ms?.values?.['p(50)'] || 0,
        init_time_p95: data.metrics.export_init_time_ms?.values?.['p(95)'] || 0,
        poll_time_avg: data.metrics.export_poll_time_ms?.values?.avg || 0,
        success: data.metrics.export_success?.values?.count || 0,
        failure: data.metrics.export_failure?.values?.count || 0,
      }
    }
  }

  return {
    'benchmark-results/bulk.json': JSON.stringify(summary, null, 2),
  }
}
