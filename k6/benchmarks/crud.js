/**
 * FHIR CRUD Operations Benchmark
 *
 * Comprehensive benchmark for Create, Read, Update, Delete operations
 * across multiple resource types.
 *
 * Target metrics:
 * - Write TPS: 500+ (HAPI baseline: 82-3,000)
 * - Read p95 latency: <20ms (Google baseline: <200ms)
 * - Memory footprint: <500MB idle (Java baseline: 2-4GB)
 */

import http from 'k6/http'
import { check, group, sleep } from 'k6'
import { Counter, Trend, Rate } from 'k6/metrics'

import { headers, generateUUID, jsonPatch, getRef } from '../util.js'
import patient from '../seed/patient.js'
import observation from '../seed/observation.js'
import organization from '../seed/organization.js'
import practitioner from '../seed/practitioner.js'

// Custom metrics
const createLatency = new Trend('create_latency_ms')
const readLatency = new Trend('read_latency_ms')
const updateLatency = new Trend('update_latency_ms')
const deleteLatency = new Trend('delete_latency_ms')
const searchLatency = new Trend('search_latency_ms')

const createSuccess = new Counter('create_success')
const readSuccess = new Counter('read_success')
const updateSuccess = new Counter('update_success')
const deleteSuccess = new Counter('delete_success')

const operationSuccess = new Rate('operation_success_rate')

export const options = {
  discardResponseBodies: false,
  scenarios: {
    // Standard load test
    crud_load: {
      executor: 'constant-vus',
      vus: 100,
      duration: '3m',
      gracefulStop: '30s',
    },
  },
  thresholds: {
    'create_latency_ms': ['p(95)<100', 'p(99)<200'],
    'read_latency_ms': ['p(95)<20', 'p(99)<50'],
    'update_latency_ms': ['p(95)<100', 'p(99)<200'],
    'delete_latency_ms': ['p(95)<100', 'p(99)<200'],
    'operation_success_rate': ['rate>0.99'],
  },
}

// Resource generators
function createPatientResource() {
  const p = JSON.parse(JSON.stringify(patient))
  p.identifier[0].value = generateUUID()
  p.name[0].given[0] = `CRUD-${Date.now()}`
  p.name[0].family = `Test-${__VU}-${__ITER}`
  return p
}

function createObservationResource(patientRef) {
  const o = JSON.parse(JSON.stringify(observation))
  o.subject = { reference: patientRef }
  o.effectiveDateTime = new Date().toISOString()
  o.valueQuantity.value = Math.floor(Math.random() * 100) + 50
  return o
}

function createOrganizationResource() {
  const org = JSON.parse(JSON.stringify(organization))
  org.name = `CRUD Org ${Date.now()}-${Math.random().toString(36).substring(7)}`
  return org
}

function createPractitionerResource() {
  const prac = JSON.parse(JSON.stringify(practitioner))
  prac.name[0].given[0] = `Doc-${Math.random().toString(36).substring(7)}`
  return prac
}

// Modify resource for update
function modifyResource(resource) {
  const modified = JSON.parse(JSON.stringify(resource))

  switch (modified.resourceType) {
    case 'Patient':
      modified.gender = modified.gender === 'female' ? 'male' : 'female'
      break
    case 'Observation':
      modified.valueQuantity.value = Math.floor(Math.random() * 100) + 50
      modified.effectiveDateTime = new Date().toISOString()
      break
    case 'Organization':
      modified.name = `${modified.name}-updated-${Date.now()}`
      break
    case 'Practitioner':
      modified.active = !modified.active
      break
  }

  return modified
}

export function setup() {
  return {
    baseUrl: __ENV.BASE_URL,
    params: { headers: headers() },
  }
}

export default function({ baseUrl, params }) {
  const resources = {}

  // CREATE phase
  group('create', () => {
    // Patient
    group('Patient', () => {
      const p = createPatientResource()
      const start = Date.now()

      const res = http.post(
        `${baseUrl}/Patient`,
        JSON.stringify(p),
        { ...params, tags: { operation: 'create', resource: 'Patient' } }
      )

      createLatency.add(Date.now() - start)

      const success = check(res, {
        'Patient created': (r) => r.status === 201,
      })

      operationSuccess.add(success)
      if (success) {
        createSuccess.add(1)
        resources.Patient = res.json()
      }
    })

    // Organization
    group('Organization', () => {
      const org = createOrganizationResource()
      const start = Date.now()

      const res = http.post(
        `${baseUrl}/Organization`,
        JSON.stringify(org),
        { ...params, tags: { operation: 'create', resource: 'Organization' } }
      )

      createLatency.add(Date.now() - start)

      const success = check(res, {
        'Organization created': (r) => r.status === 201,
      })

      operationSuccess.add(success)
      if (success) {
        createSuccess.add(1)
        resources.Organization = res.json()
      }
    })

    // Practitioner
    group('Practitioner', () => {
      const prac = createPractitionerResource()
      const start = Date.now()

      const res = http.post(
        `${baseUrl}/Practitioner`,
        JSON.stringify(prac),
        { ...params, tags: { operation: 'create', resource: 'Practitioner' } }
      )

      createLatency.add(Date.now() - start)

      const success = check(res, {
        'Practitioner created': (r) => r.status === 201,
      })

      operationSuccess.add(success)
      if (success) {
        createSuccess.add(1)
        resources.Practitioner = res.json()
      }
    })

    // Observation (depends on Patient)
    if (resources.Patient) {
      group('Observation', () => {
        const obs = createObservationResource(`Patient/${resources.Patient.id}`)
        const start = Date.now()

        const res = http.post(
          `${baseUrl}/Observation`,
          JSON.stringify(obs),
          { ...params, tags: { operation: 'create', resource: 'Observation' } }
        )

        createLatency.add(Date.now() - start)

        const success = check(res, {
          'Observation created': (r) => r.status === 201,
        })

        operationSuccess.add(success)
        if (success) {
          createSuccess.add(1)
          resources.Observation = res.json()
        }
      })
    }
  })

  // READ phase
  group('read', () => {
    Object.entries(resources).forEach(([rt, resource]) => {
      group(rt, () => {
        const start = Date.now()

        const res = http.get(
          `${baseUrl}/${rt}/${resource.id}`,
          { ...params, tags: { operation: 'read', resource: rt } }
        )

        readLatency.add(Date.now() - start)

        const success = check(res, {
          [`${rt} read`]: (r) => r.status === 200,
        })

        operationSuccess.add(success)
        if (success) readSuccess.add(1)
      })
    })
  })

  // SEARCH phase
  group('search', () => {
    if (resources.Patient) {
      group('Patient by name', () => {
        const start = Date.now()

        const res = http.get(
          `${baseUrl}/Patient?family=${resources.Patient.name[0].family}&_count=10`,
          { ...params, tags: { operation: 'search', resource: 'Patient' } }
        )

        searchLatency.add(Date.now() - start)

        check(res, {
          'Patient search 200': (r) => r.status === 200,
          'Patient search returns bundle': (r) => {
            try {
              return r.json().resourceType === 'Bundle'
            } catch { return false }
          }
        })
      })
    }
  })

  // UPDATE phase
  group('update', () => {
    Object.entries(resources).forEach(([rt, resource]) => {
      group(rt, () => {
        const modified = modifyResource(resource)
        const start = Date.now()

        const res = http.put(
          `${baseUrl}/${rt}/${resource.id}`,
          JSON.stringify(modified),
          { ...params, tags: { operation: 'update', resource: rt } }
        )

        updateLatency.add(Date.now() - start)

        const success = check(res, {
          [`${rt} updated`]: (r) => r.status === 200,
        })

        operationSuccess.add(success)
        if (success) updateSuccess.add(1)
      })
    })
  })

  // DELETE phase (reverse order due to references)
  group('delete', () => {
    const deleteOrder = ['Observation', 'Practitioner', 'Organization', 'Patient']

    deleteOrder.forEach(rt => {
      if (resources[rt]) {
        group(rt, () => {
          const start = Date.now()

          const res = http.del(
            `${baseUrl}/${rt}/${resources[rt].id}`,
            null,
            { ...params, tags: { operation: 'delete', resource: rt } }
          )

          deleteLatency.add(Date.now() - start)

          const success = check(res, {
            [`${rt} deleted`]: (r) => r.status === 200 || r.status === 204,
          })

          operationSuccess.add(success)
          if (success) deleteSuccess.add(1)
        })
      }
    })
  })

  sleep(0.05) // Small pause between iterations
}

export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'crud',
    metrics: {
      create: {
        count: data.metrics.create_success?.values?.count || 0,
        p50: data.metrics.create_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.create_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.create_latency_ms?.values?.['p(99)'] || 0,
      },
      read: {
        count: data.metrics.read_success?.values?.count || 0,
        p50: data.metrics.read_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.read_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.read_latency_ms?.values?.['p(99)'] || 0,
      },
      update: {
        count: data.metrics.update_success?.values?.count || 0,
        p50: data.metrics.update_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.update_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.update_latency_ms?.values?.['p(99)'] || 0,
      },
      delete: {
        count: data.metrics.delete_success?.values?.count || 0,
        p50: data.metrics.delete_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.delete_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.delete_latency_ms?.values?.['p(99)'] || 0,
      },
      search: {
        p50: data.metrics.search_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.search_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.search_latency_ms?.values?.['p(99)'] || 0,
      },
      overall: {
        success_rate: data.metrics.operation_success_rate?.values?.rate || 0,
        requests_per_second: data.metrics.http_reqs?.values?.rate || 0,
      }
    }
  }

  return {
    'benchmark-results/crud.json': JSON.stringify(summary, null, 2),
  }
}
