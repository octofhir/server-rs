/**
 * k6 Utilities for OctoFHIR Benchmarks
 * Supports both public (octofhir-ui) and confidential (k6-test) clients
 */

import http from 'k6/http'
import { check } from 'k6'

export function jsonPatch(obj, path, value) {
  let pt = obj;
  const ks = path.split('.');
  while (ks.length > 1) pt = pt[ks.shift()];
  pt[ks.shift()] = value;
  return obj;
}

export function is200(url, params) {
  const res = http.get(url, params)
  return check(res, { 'Status 200': ({ status }) => status === 200 })
}

// OctoFHIR password grant flow - supports both public and confidential clients
const passwordGrant = () => {
  const user = __ENV.AUTH_USER || __ENV.AUTH_USERNAME || __ENV.OAUTH2_USER
  const pass = __ENV.AUTH_PASSWORD || __ENV.OAUTH2_PASSWORD
  const baseUrl = __ENV.BASE_URL || 'http://localhost:8888/fhir'
  const serverRoot = baseUrl.replace(/\/fhir\/?$/, '')
  const tokenURL = __ENV.TOKEN_URL || `${serverRoot}/auth/token`

  // Support both k6-test (confidential) and octofhir-ui (public) clients
  const clientId = __ENV.CLIENT_ID || 'octofhir-ui'
  const clientSecret = __ENV.CLIENT_SECRET || ''

  if (!user || !pass) {
    console.log('No credentials provided (AUTH_USER/AUTH_PASSWORD)')
    return null
  }

  let body = `grant_type=password&username=${user}&password=${pass}&client_id=${clientId}`

  // Add client_secret only for confidential clients
  if (clientSecret) {
    body += `&client_secret=${clientSecret}`
  }

  const token = http.post(
    tokenURL,
    body,
    { headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, responseType: 'text' }
  )

  if (!check(token, { 'Password grant token': ({ status }) => status === 200 })) {
    console.log('Password grant failed:', token.body)
    return null
  }

  return { "Authorization": `Bearer ${token.json('access_token')}` }
}

export function headers() {
  const auth = passwordGrant()
  return {
    ...auth,
    "Accept-Encoding": "gzip",
    "Accept": "application/fhir+json, application/json",
    "Content-Type": "application/fhir+json",
  }
}

// Generate UUID v4
export function generateUUID() {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = Math.random() * 16 | 0
    const v = c === 'x' ? r : (r & 0x3 | 0x8)
    return v.toString(16)
  })
}

// Get resource reference string
export function getRef(data, rt) {
  return `${data[rt].resourceType}/${data[rt].id}`
}
