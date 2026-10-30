/**
 * Write-throughput cost of UCUM canonical quantity extraction.
 *
 * Posts transaction Bundles of N Observations, each with a UCUM valueQuantity
 * and two UCUM component quantities, and reports per-bundle latency. Run it
 * against a recreated database before and after a change to price it on the
 * real server write path. `plain` posts Patients instead, which carry no
 * quantities, to check that resources without them pay nothing.
 *
 * Usage: bun run scripts/bench-write-ucum.ts [rounds] [perBundle] [ucum|plain]
 */

const BASE_URL = process.env.OCTOFHIR_BASE_URL ?? "http://localhost:8888";
const ROUNDS = Number(process.argv[2] ?? 30);
const PER_BUNDLE = Number(process.argv[3] ?? 100);
const MODE = process.argv[4] ?? "ucum";
const WARMUP = 5;

const UCUM = "http://unitsofmeasure.org";
const UNITS = ["mg", "g", "kg", "ug", "[lb_av]"];

const tokenRes = await fetch(`${BASE_URL}/auth/token`, {
  method: "POST",
  headers: { "Content-Type": "application/x-www-form-urlencoded" },
  body: new URLSearchParams({
    grant_type: "client_credentials",
    client_id: "backend",
    client_secret: "dev-secret-2024",
    scope: "system/*.cruds",
  }).toString(),
});
if (!tokenRes.ok) {
  console.error(`token request failed: ${tokenRes.status} ${await tokenRes.text()}`);
  process.exit(1);
}
const { access_token } = (await tokenRes.json()) as { access_token: string };

function observation(id: string, i: number) {
  const unit = UNITS[i % UNITS.length];
  return {
    resourceType: "Observation",
    id,
    status: "final",
    code: { coding: [{ system: "http://loinc.org", code: "29463-7" }] },
    effectiveDateTime: "2024-01-01T10:00:00Z",
    valueQuantity: { value: 10 + (i % 90) / 10, unit, system: UCUM, code: unit },
    component: [
      {
        code: { coding: [{ system: "http://loinc.org", code: "8480-6" }] },
        valueQuantity: { value: 110 + (i % 40), unit: "mm[Hg]", system: UCUM, code: "mm[Hg]" },
      },
      {
        code: { coding: [{ system: "http://loinc.org", code: "8462-4" }] },
        valueQuantity: { value: 70 + (i % 20), unit: "mm[Hg]", system: UCUM, code: "mm[Hg]" },
      },
    ],
  };
}

function patient(id: string, i: number) {
  return {
    resourceType: "Patient",
    id,
    name: [{ family: `Bench${i}`, given: ["Ucum"] }],
    gender: i % 2 === 0 ? "male" : "female",
    birthDate: "1980-01-01",
  };
}

function bundle(round: number) {
  const type = MODE === "plain" ? "Patient" : "Observation";
  const entry = [];
  for (let i = 0; i < PER_BUNDLE; i++) {
    const id = `ucum-bench-${MODE}-${round}-${i}`;
    const resource = MODE === "plain" ? patient(id, i) : observation(id, i);
    entry.push({
      fullUrl: `${BASE_URL}/fhir/${type}/${id}`,
      resource,
      request: { method: "PUT", url: `${type}/${id}` },
    });
  }
  return { resourceType: "Bundle", type: "transaction", entry };
}

async function post(round: number): Promise<number> {
  const body = JSON.stringify(bundle(round));
  const started = performance.now();
  const res = await fetch(`${BASE_URL}/fhir`, {
    method: "POST",
    headers: {
      "Content-Type": "application/fhir+json",
      Authorization: `Bearer ${access_token}`,
    },
    body,
  });
  const elapsed = performance.now() - started;
  if (!res.ok) {
    console.error(`bundle failed: ${res.status} ${(await res.text()).slice(0, 400)}`);
    process.exit(1);
  }
  return elapsed;
}

for (let r = 0; r < WARMUP; r++) await post(-1 - r);

const samples: number[] = [];
for (let r = 0; r < ROUNDS; r++) samples.push(await post(r));

samples.sort((a, b) => a - b);
const pct = (p: number) => samples[Math.min(samples.length - 1, Math.floor((p / 100) * samples.length))];
const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
const total = ROUNDS * PER_BUNDLE;
const wall = samples.reduce((a, b) => a + b, 0);

console.log(`\npayload: ${MODE === "plain" ? "Patients (no quantities)" : "Observations (3 UCUM quantities each)"}`);
console.log(`${ROUNDS} bundles x ${PER_BUNDLE} = ${total} resources`);
console.log(`  mean  ${mean.toFixed(1)} ms/bundle`);
console.log(`  p50   ${pct(50).toFixed(1)} ms`);
console.log(`  p95   ${pct(95).toFixed(1)} ms`);
console.log(`  thrpt ${((total / wall) * 1000).toFixed(0)} resources/s`);
