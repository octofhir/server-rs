// Read-only FHIRPath benchmark: no seeding or writes to the FHIR store.
import http from 'k6/http';
import { check } from 'k6';
import { Trend } from 'k6/metrics';

const profileEvaluation = __ENV.PROFILE_EVAL === '1';
const evaluationTime = profileEvaluation ? new Trend('fhirpath_evaluation_ms', true) : null;

const expressions = {
  regex: "name.where(family.matches('^item-[0-9]+$')).count()",
  where: "name.where(use = 'official').count()",
  distinct: "(name.family | name.family).count()",
};
const scenario = __ENV.CASE || 'regex';
if (!expressions[scenario]) throw new Error(`Unknown CASE: ${scenario}`);
const expected = scenario === 'where' ? 50 : 100;
const payload = JSON.stringify({
  resourceType: 'Parameters',
  parameter: [
    { name: 'expression', valueString: expressions[scenario] },
    { name: 'resource', resource: {
      resourceType: 'Patient',
      name: Array.from({ length: 100 }, (_, i) => ({
        family: `item-${i}`, use: i % 2 === 0 ? 'official' : 'usual',
      })),
    } },
  ],
});
export const options = {
  vus: Number(__ENV.VUS || 8),
  duration: __ENV.DURATION || '15s',
  summaryTrendStats: ['avg', 'med', 'p(95)', 'p(99)', 'max'],
  thresholds: { http_req_failed: ['rate==0'], checks: ['rate==1'] },
};
const url = `${__ENV.BASE_URL || 'http://127.0.0.1:18888'}/fhir/$fhirpath`;
const headers = { 'Content-Type': 'application/fhir+json' };
if (__ENV.AUTH_TOKEN) headers.Authorization = `Bearer ${__ENV.AUTH_TOKEN}`;

function containsResult(value) {
  if (!value || typeof value !== 'object') return false;
  if (value.valueInteger === expected) return true;
  return Object.values(value).some(v => Array.isArray(v)
    ? v.some(containsResult) : containsResult(v));
}
export function setup() {
  const response = http.post(url, payload, { headers });
  if (response.status !== 200 || !containsResult(response.json())) {
    throw new Error(`FHIRPath smoke failed: ${response.status} ${response.body}`);
  }
  for (let i = 0; i < 25; i++) http.post(url, payload, { headers });
}
export default function () {
  const response = http.post(url, payload, { headers });
  if (evaluationTime && response.status === 200) {
    const metadata = response.json().parameter?.find(p => p.name === 'metadata');
    const timing = metadata?.part?.find(p => p.name === 'timing');
    const milliseconds = timing?.part?.find(p => p.name === 'evaluationTime')?.valueDecimal;
    check(milliseconds, { 'finite evaluator timing': Number.isFinite });
    if (Number.isFinite(milliseconds)) evaluationTime.add(milliseconds);
  }
  check(response, {
    'HTTP 200': r => r.status === 200,
    'correct FHIRPath result': r => r.status === 200 && containsResult(r.json()),
  });
}
