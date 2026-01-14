/**
 * FHIR Search Benchmark
 *
 * Tests various FHIR search operations:
 * - Simple searches (string, token, date, reference)
 * - Chained searches (patient.name, subject:Patient.identifier)
 * - _include and _revinclude
 * - Pagination
 * - Sorting
 *
 * Target metrics:
 * - Simple search p95: <50ms
 * - Chained search p95: <200ms
 * - Include search p95: <300ms
 */

import http from 'k6/http'
import { check, group, sleep } from 'k6'
import { Counter, Trend } from 'k6/metrics'

import { headers, generateUUID } from '../util.js'
import patient from '../seed/patient.js'
import observation from '../seed/observation.js'
import organization from '../seed/organization.js'
import practitioner from '../seed/practitioner.js'

// Custom metrics
const simpleSearchLatency = new Trend('simple_search_latency_ms')
const chainedSearchLatency = new Trend('chained_search_latency_ms')
const includeSearchLatency = new Trend('include_search_latency_ms')
const paginationLatency = new Trend('pagination_latency_ms')

const searchSuccess = new Counter('search_success')
const searchFailure = new Counter('search_failure')

export const options = {
  discardResponseBodies: false,
  scenarios: {
    // Seed data first
    seed: {
      executor: 'shared-iterations',
      vus: 5,
      iterations: 50,
      maxDuration: '3m',
      exec: 'seedData',
      tags: { phase: 'seed' },
    },
    // Search tests
    search: {
      executor: 'constant-vus',
      vus: 30,
      duration: '3m',
      startTime: '3m30s',
      gracefulStop: '30s',
      exec: 'searchTest',
      tags: { phase: 'search' },
    },
  },
  thresholds: {
    'simple_search_latency_ms': ['p(95)<100', 'p(99)<200'],
    'chained_search_latency_ms': ['p(95)<300', 'p(99)<500'],
    'include_search_latency_ms': ['p(95)<400', 'p(99)<800'],
  },
}

// Seed data for searches
const seedPatientFamily = `SearchTest-${Date.now()}`
const seedOrgName = `SearchOrg-${Date.now()}`

export function setup() {
  return {
    baseUrl: __ENV.BASE_URL,
    params: { headers: headers() },
    seedPatientFamily,
    seedOrgName,
  }
}

// Seed phase - create test data
export function seedData({ baseUrl, params, seedPatientFamily, seedOrgName }) {
  // Create organization
  const org = JSON.parse(JSON.stringify(organization))
  org.name = seedOrgName
  const orgRes = http.post(`${baseUrl}/Organization`, JSON.stringify(org), params)
  let orgId = null
  if (orgRes.status === 201) {
    orgId = orgRes.json().id
  }

  // Create practitioner
  const prac = JSON.parse(JSON.stringify(practitioner))
  prac.name[0].family = `SearchDoc-${Date.now()}`
  const pracRes = http.post(`${baseUrl}/Practitioner`, JSON.stringify(prac), params)
  let pracId = null
  if (pracRes.status === 201) {
    pracId = pracRes.json().id
  }

  // Create patient with specific family name for searches
  const p = JSON.parse(JSON.stringify(patient))
  p.identifier[0].value = generateUUID()
  p.name[0].family = seedPatientFamily
  p.name[0].given[0] = `John-${__VU}-${__ITER}`
  if (orgId) {
    p.managingOrganization = { reference: `Organization/${orgId}` }
  }
  if (pracId) {
    p.generalPractitioner = [{ reference: `Practitioner/${pracId}` }]
  }

  const patRes = http.post(`${baseUrl}/Patient`, JSON.stringify(p), params)
  let patId = null
  if (patRes.status === 201) {
    patId = patRes.json().id
  }

  // Create observations for the patient
  if (patId) {
    for (let i = 0; i < 5; i++) {
      const obs = JSON.parse(JSON.stringify(observation))
      obs.subject = { reference: `Patient/${patId}` }
      obs.effectiveDateTime = new Date(Date.now() - i * 86400000).toISOString()
      obs.valueQuantity.value = 60 + Math.floor(Math.random() * 40)
      http.post(`${baseUrl}/Observation`, JSON.stringify(obs), params)
    }
  }

  sleep(0.2)
}

// Search phase
export function searchTest({ baseUrl, params, seedPatientFamily, seedOrgName }) {
  // Simple searches
  group('simple', () => {
    // String search
    group('string', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Patient?family=${seedPatientFamily}&_count=20`,
        { ...params, tags: { search_type: 'string' } }
      )
      simpleSearchLatency.add(Date.now() - start)

      const success = check(res, {
        'string search 200': (r) => r.status === 200,
        'string search returns results': (r) => {
          try {
            const bundle = r.json()
            return bundle.resourceType === 'Bundle' && bundle.total > 0
          } catch { return false }
        }
      })

      if (success) searchSuccess.add(1)
      else searchFailure.add(1)
    })

    // Token search
    group('token', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Observation?code=29463-7&_count=20`,
        { ...params, tags: { search_type: 'token' } }
      )
      simpleSearchLatency.add(Date.now() - start)

      check(res, { 'token search 200': (r) => r.status === 200 })
    })

    // Token with system
    group('token-system', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Observation?code=http://loinc.org|29463-7&_count=20`,
        { ...params, tags: { search_type: 'token-system' } }
      )
      simpleSearchLatency.add(Date.now() - start)

      check(res, { 'token-system search 200': (r) => r.status === 200 })
    })

    // Date search
    group('date', () => {
      const today = new Date().toISOString().split('T')[0]
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Observation?date=lt${today}&_count=20`,
        { ...params, tags: { search_type: 'date' } }
      )
      simpleSearchLatency.add(Date.now() - start)

      check(res, { 'date search 200': (r) => r.status === 200 })
    })

    // Reference search
    group('reference', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Observation?subject:Patient.family=${seedPatientFamily}&_count=10`,
        { ...params, tags: { search_type: 'reference' } }
      )
      simpleSearchLatency.add(Date.now() - start)

      check(res, { 'reference search 200': (r) => r.status === 200 })
    })
  })

  // Chained searches
  group('chained', () => {
    // Observation -> Patient chain
    group('observation-patient', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Observation?subject:Patient.family=${seedPatientFamily}&_count=20`,
        { ...params, tags: { search_type: 'chained' } }
      )
      chainedSearchLatency.add(Date.now() - start)

      check(res, { 'chained observation-patient 200': (r) => r.status === 200 })
    })

    // Patient -> Organization chain
    group('patient-organization', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Patient?organization.name=${seedOrgName}&_count=20`,
        { ...params, tags: { search_type: 'chained' } }
      )
      chainedSearchLatency.add(Date.now() - start)

      check(res, { 'chained patient-org 200': (r) => r.status === 200 })
    })
  })

  // _include and _revinclude
  group('include', () => {
    // _include
    group('include-patient', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Observation?code=29463-7&_include=Observation:subject&_count=10`,
        { ...params, tags: { search_type: 'include' } }
      )
      includeSearchLatency.add(Date.now() - start)

      const success = check(res, {
        'include 200': (r) => r.status === 200,
        'include has patient': (r) => {
          try {
            const bundle = r.json()
            return bundle.entry?.some(e => e.resource?.resourceType === 'Patient')
          } catch { return false }
        }
      })

      if (success) searchSuccess.add(1)
      else searchFailure.add(1)
    })

    // _revinclude
    group('revinclude-observations', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Patient?family=${seedPatientFamily}&_revinclude=Observation:subject&_count=10`,
        { ...params, tags: { search_type: 'revinclude' } }
      )
      includeSearchLatency.add(Date.now() - start)

      const success = check(res, {
        'revinclude 200': (r) => r.status === 200,
        'revinclude has observations': (r) => {
          try {
            const bundle = r.json()
            return bundle.entry?.some(e => e.resource?.resourceType === 'Observation')
          } catch { return false }
        }
      })

      if (success) searchSuccess.add(1)
      else searchFailure.add(1)
    })
  })

  // Pagination
  group('pagination', () => {
    group('first-page', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Patient?_count=10`,
        { ...params, tags: { search_type: 'pagination' } }
      )
      paginationLatency.add(Date.now() - start)

      check(res, {
        'pagination 200': (r) => r.status === 200,
        'has next link': (r) => {
          try {
            const bundle = r.json()
            return bundle.link?.some(l => l.relation === 'next')
          } catch { return false }
        }
      })
    })

    // Get second page using offset
    group('second-page', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Patient?_count=10&_offset=10`,
        { ...params, tags: { search_type: 'pagination' } }
      )
      paginationLatency.add(Date.now() - start)

      check(res, { 'second page 200': (r) => r.status === 200 })
    })
  })

  // Sorting
  group('sorting', () => {
    group('sort-date', () => {
      const start = Date.now()
      const res = http.get(
        `${baseUrl}/Observation?code=29463-7&_sort=-date&_count=20`,
        { ...params, tags: { search_type: 'sort' } }
      )
      simpleSearchLatency.add(Date.now() - start)

      check(res, { 'sort 200': (r) => r.status === 200 })
    })
  })

  sleep(0.1)
}

export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'search',
    metrics: {
      simple: {
        p50: data.metrics.simple_search_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.simple_search_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.simple_search_latency_ms?.values?.['p(99)'] || 0,
      },
      chained: {
        p50: data.metrics.chained_search_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.chained_search_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.chained_search_latency_ms?.values?.['p(99)'] || 0,
      },
      include: {
        p50: data.metrics.include_search_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.include_search_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.include_search_latency_ms?.values?.['p(99)'] || 0,
      },
      pagination: {
        p50: data.metrics.pagination_latency_ms?.values?.['p(50)'] || 0,
        p95: data.metrics.pagination_latency_ms?.values?.['p(95)'] || 0,
        p99: data.metrics.pagination_latency_ms?.values?.['p(99)'] || 0,
      },
      overall: {
        success: data.metrics.search_success?.values?.count || 0,
        failure: data.metrics.search_failure?.values?.count || 0,
      }
    }
  }

  return {
    'benchmark-results/search.json': JSON.stringify(summary, null, 2),
  }
}
