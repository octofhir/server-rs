// Read-only HTTP benchmark for a separately provisioned LOCAL OctoFHIR fixture.
// No seed, write, cleanup, authentication flow, or changes to server settings.
import http from 'k6/http';
import { check } from 'k6';
import { Trend, Rate } from 'k6/metrics';

const base = (__ENV.BASE_URL || '').replace(/\/$/, '');
if (!/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?\/fhir$/.test(base)) {
  throw new Error('BASE_URL must name the isolated local server, e.g. http://127.0.0.1:8888/fhir');
}
const patient = __ENV.PATIENT_ID || 'p-10';
const family = __ENV.FAMILY || 'Smith';
const system = __ENV.MRN_SYSTEM || 'http://hospital.example/mrn';
const mrn = __ENV.MRN || 'mrn-19990';
const code = __ENV.CODE || 'http://loinc.org|8480-6';
const cases = [
  ['identifier_rare', `Patient?identifier=${encodeURIComponent(`${system}|${mrn}`)}`],
  ['identifier_empty', `Patient?identifier=${encodeURIComponent(`${system}|absent`)}`],
  ['family_prefix', `Patient?family=${encodeURIComponent(family)}`],
  ['birthdate', 'Patient?birthdate=ge1980-01-01&birthdate=le2000-12-31'],
  ['combined', `Patient?family=${encodeURIComponent(family)}&gender=female&birthdate=ge1980`],
  ['reference', `Observation?subject=${encodeURIComponent(`Patient/${patient}`)}`],
  ['chain', `Observation?subject:Patient.family=${encodeURIComponent(family)}`],
  ['has', `Patient?_has:Observation:subject:code=${encodeURIComponent(code)}`],
  ['sort', 'Patient?gender=female&_sort=-_lastUpdated'],
  ['deep', 'Patient?gender=female&_sort=-_lastUpdated&_offset=9000'],
  ['include', `Observation?subject=${encodeURIComponent(`Patient/${patient}`)}&_include=Observation:subject`],
  ['revinclude', `Patient?_id=${encodeURIComponent(patient)}&_revinclude=Observation:subject`],
  ['accurate', 'Patient?gender=female&_total=accurate'],
  ['none', 'Patient?gender=female&_total=none'],
];
const latency = new Trend('search_full_http_ms', true);
const failures = new Rate('search_errors');
const thresholds = { search_errors: ['rate==0'] };
for (const [name] of cases) {
  thresholds[`search_full_http_ms{case:${name}}`] = ['p(99)>=0']; // expose each case; no invented SLO
}
export const options = {
  vus: Number(__ENV.VUS || 1), duration: __ENV.DURATION || '30s',
  summaryTrendStats: ['count', 'avg', 'min', 'med', 'p(95)', 'p(99)', 'max'], thresholds,
};
export default function () {
  const [name, path] = cases[(__ITER + __VU - 1) % cases.length];
  const headers = { Accept: 'application/fhir+json' };
  if (__ENV.TOKEN) headers.Authorization = `Bearer ${__ENV.TOKEN}`;
  const start = Date.now();
  const response = http.get(`${base}/${path}&_count=10`, { headers, tags: { case: name }, timeout: '10s' });
  latency.add(Date.now() - start, { case: name });
  let bundle = null;
  try { bundle = response.json(); } catch (_) { /* recorded as failure below */ }
  const ok = check(response, {
    'HTTP 200 searchset': () => response.status === 200 && bundle?.resourceType === 'Bundle' && bundle?.type === 'searchset',
    'match entries <= 10': () => (bundle?.entry || []).filter(e => e.search?.mode === 'match').length <= 10,
  }, { case: name });
  failures.add(!ok, { case: name });
}
