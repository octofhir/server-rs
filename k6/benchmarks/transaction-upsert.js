/**
 * FHIR Transaction Upsert Benchmark
 *
 * The existing transaction benchmark builds POST-only bundles, which have been
 * batched for a while. This one exercises the write shapes that used to cost one
 * round-trip per entry:
 *
 * - PUT `Type/id` bundles (update-as-create on the first pass, update after)
 * - DELETE `Type/id` bundles
 * - conditional create bundles (`ifNoneExist=identifier=...`)
 *
 * Each VU owns its own id space, so VUs never write the same row and the
 * measurement is not distorted by row-level lock contention.
 */

import http from 'k6/http'
import { check } from 'k6'
import { Counter, Trend } from 'k6/metrics'

import { summaryTrendStats } from '../lib/utils.js'
import { headers, generateUUID } from '../util.js'
import patient from '../seed/patient.js'
import observation from '../seed/observation.js'

const putSuccess = new Counter('upsert_put_success')
const putFailure = new Counter('upsert_put_failure')
const deleteSuccess = new Counter('upsert_delete_success')
const deleteFailure = new Counter('upsert_delete_failure')
const condSuccess = new Counter('upsert_conditional_success')
const condFailure = new Counter('upsert_conditional_failure')

const putCreateDuration = new Trend('upsert_put_create_ms')
const putUpdateDuration = new Trend('upsert_put_update_ms')
const deleteDuration = new Trend('upsert_delete_ms')
const conditionalDuration = new Trend('upsert_conditional_ms')

const BUNDLE_SIZE = Number(__ENV.BUNDLE_SIZE || '50')
// Module scope is re-evaluated per VU, so a `Date.now()` default would give
// every VU (and `setup`) a different run id. `setup` picks the id and hands it
// to the VUs through its return value.
const RUN = __ENV.RUN_ID || 'k6run'
// Conditional creates search by `identifier`, which is not in the perf
// deployment's INDEXED_PARAMS — on a multi-hundred-thousand-row table that is a
// seq scan and would dominate the whole run. Opt in with CONDITIONAL=1.
const WITH_CONDITIONAL = __ENV.CONDITIONAL === '1'

export const options = {
  discardResponseBodies: false,
  summaryTrendStats,
  scenarios: {
    upsert: {
      executor: 'constant-vus',
      vus: Number(__ENV.VUS || '10'),
      duration: __ENV.DURATION || '2m',
      gracefulStop: '30s',
    },
  },
  thresholds: {
    upsert_put_update_ms: ['p(95)<5000'],
    upsert_delete_ms: ['p(95)<5000'],
  },
}

// Ids are deterministic per (VU, iteration) so the same bundle can be replayed
// as a create, then an update, then a delete.
function idsFor(runId, vu, iter, size) {
  const ids = []
  for (let i = 0; i < size; i++) {
    ids.push(`${runId}-v${vu}-i${iter}-${i}`)
  }
  return ids
}

function putEntry(id, index, anchorId) {
  // 40% Patient, 60% Observation — roughly the mix of a real write bundle.
  if (index % 5 < 2) {
    const p = JSON.parse(JSON.stringify(patient))
    p.id = id
    p.identifier[0].value = id
    p.name[0].given[0] = `Upsert-${index}`
    return { resource: p, request: { method: 'PUT', url: `Patient/${id}` } }
  }
  const o = JSON.parse(JSON.stringify(observation))
  o.id = id
  o.subject = { reference: `Patient/${anchorId}` }
  o.effectiveDateTime = new Date().toISOString()
  o.valueQuantity.value = 50 + (index % 50)
  return { resource: o, request: { method: 'PUT', url: `Observation/${id}` } }
}

function deleteEntry(id, index) {
  const type = index % 5 < 2 ? 'Patient' : 'Observation'
  return { request: { method: 'DELETE', url: `${type}/${id}` } }
}

function bundle(entries) {
  return JSON.stringify({ resourceType: 'Bundle', type: 'transaction', entry: entries })
}

function post(body, params) {
  return http.post(`${__ENV.BASE_URL || 'http://localhost:8888/fhir'}`, body, params)
}

export function setup() {
  const params = { headers: headers() }
  const runId = `k6-${RUN}-${Date.now()}`
  // Observations reference an anchor Patient; reference validation needs it to exist.
  const anchor = JSON.parse(JSON.stringify(patient))
  anchor.id = `${runId}-anchor`
  anchor.identifier[0].value = `${runId}-anchor`
  const res = post(
    bundle([{ resource: anchor, request: { method: 'PUT', url: `Patient/${anchor.id}` } }]),
    params,
  )
  if (!check(res, { 'anchor patient created': (r) => r.status === 200 })) {
    throw new Error(`anchor setup failed: ${res.status} ${String(res.body).slice(0, 300)}`)
  }
  return { runId, anchorId: anchor.id }
}

export default function (data) {
  const params = { headers: headers() }
  const ids = idsFor(data.runId, __VU, __ITER, BUNDLE_SIZE)
  const entries = ids.map((id, i) => putEntry(id, i, data.anchorId))

  // 1. PUT bundle against ids that do not exist yet — update-as-create.
  let res = post(bundle(entries), params)
  putCreateDuration.add(res.timings.duration)
  if (check(res, { 'put-create 200': (r) => r.status === 200 })) putSuccess.add(1)
  else {
    putFailure.add(1)
    console.error(`put-create failed: ${res.status} ${String(res.body).slice(0, 200)}`)
  }

  // 2. Same bundle again — now every entry is a real update.
  res = post(bundle(entries), params)
  putUpdateDuration.add(res.timings.duration)
  if (check(res, { 'put-update 200': (r) => r.status === 200 })) putSuccess.add(1)
  else {
    putFailure.add(1)
    console.error(`put-update failed: ${res.status} ${String(res.body).slice(0, 200)}`)
  }

  // 3. Conditional create bundle — second pass must reuse, not duplicate.
  if (WITH_CONDITIONAL) {
    const condEntries = ids.slice(0, Math.max(1, Math.floor(BUNDLE_SIZE / 5))).map((id) => {
      const p = JSON.parse(JSON.stringify(patient))
      p.identifier[0].value = `cond-${id}`
      delete p.id
      return {
        fullUrl: `urn:uuid:${generateUUID()}`,
        resource: p,
        request: { method: 'POST', url: 'Patient', ifNoneExist: `identifier=cond-${id}` },
      }
    })
    res = post(bundle(condEntries), params)
    conditionalDuration.add(res.timings.duration)
    if (check(res, { 'conditional 200': (r) => r.status === 200 })) condSuccess.add(1)
    else {
      condFailure.add(1)
      console.error(`conditional failed: ${res.status} ${String(res.body).slice(0, 200)}`)
    }
  }

  // 4. DELETE bundle — clean up this iteration's rows.
  res = post(bundle(ids.map((id, i) => deleteEntry(id, i))), params)
  deleteDuration.add(res.timings.duration)
  if (check(res, { 'delete 200': (r) => r.status === 200 })) deleteSuccess.add(1)
  else {
    deleteFailure.add(1)
    console.error(`delete failed: ${res.status} ${String(res.body).slice(0, 200)}`)
  }
}
