// Read-only $validate benchmark. Setup verifies that pat-1 actually executes.
import http from 'k6/http';
import { check } from 'k6';

const nodes = Number(__ENV.NODES || 100);
if (!Number.isSafeInteger(nodes) || nodes < 1) throw new Error('NODES must be positive');
const url = `${__ENV.BASE_URL || 'http://127.0.0.1:18888'}/fhir/Patient/$validate`;
const headers = { 'Content-Type': 'application/fhir+json' };
if (__ENV.AUTH_TOKEN) headers.Authorization = `Bearer ${__ENV.AUTH_TOKEN}`;

function parameters(resource) {
  return JSON.stringify({
    resourceType: 'Parameters',
    parameter: [{ name: 'resource', resource }],
  });
}
const payload = parameters({
  resourceType: 'Patient',
  id: 'validation-benchmark',
  contact: Array.from({ length: nodes }, (_, i) => ({
    name: { family: `Family-${i}`, given: ['First', 'Second'] },
  })),
});

export const options = {
  vus: Number(__ENV.VUS || 8),
  duration: __ENV.DURATION || '15s',
  summaryTrendStats: ['avg', 'med', 'p(95)', 'p(99)', 'max'],
  thresholds: { http_req_failed: ['rate==0'], checks: ['rate==1'] },
};

function succeeds(response) {
  if (response.status !== 200) return false;
  const body = response.json();
  return body.resourceType === 'OperationOutcome'
    && Array.isArray(body.issue)
    && body.issue.every(issue => !['fatal', 'error'].includes(issue.severity));
}

export function setup() {
  const valid = http.post(url, payload, { headers });
  if (!succeeds(valid)) throw new Error(`Validation smoke failed: ${valid.status} ${valid.body}`);
  const invalid = http.post(url, parameters({
    resourceType: 'Patient', contact: [{}],
  }), { headers });
  if (invalid.status !== 200 || !invalid.json().issue?.some(issue =>
    issue.severity === 'error' && issue.diagnostics?.includes('pat-1'))) {
    throw new Error(`FHIRPath invariant was not enforced: ${invalid.status} ${invalid.body}`);
  }
  for (let i = 0; i < 25; i++) http.post(url, payload, { headers });
}

export default function () {
  const response = http.post(url, payload, { headers });
  check(response, {
    'HTTP 200': r => r.status === 200,
    'valid resource accepted': succeeds,
  });
}
