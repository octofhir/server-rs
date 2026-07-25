/**
 * targetProfile conformance benchmark.
 *
 * Writes the same reference-heavy ExplanationOfBenefit repeatedly with
 * `check_target_profile` on, and reports the write-latency distribution. The
 * EOB from the performance-benchmark repo points at a Patient, a Practitioner,
 * a Claim and an Encounter, so every write exercises the whole reference
 * conformance phase.
 *
 * Run the server with:
 *   OCTOFHIR__VALIDATION__CHECK_TARGET_PROFILE=true cargo run
 *
 * Usage: bun run scripts/bench-targetprofile.ts [iterations]
 */
import EOB from "../../fhir-server-performance-benchmark/k6/seed/explanation-of-benefit.js";

const BASE_URL = "http://localhost:8888";
const ITERATIONS = Number(process.argv[2] ?? 200);
const WARMUP = 20;

const PATIENT_ID = "bf663c54-fae5-4787-8220-451dd503151b";
const PRACTITIONER_ID = "bc1a13e7-da50-4f90-bd7c-40f4a887f091";
const CLAIM_ID = "5c71dc7e-c1bb-4a39-ade2-cfc0fcf28d35";
const ENCOUNTER_ID = "eca46f85-90a6-4154-8ab4-2a7f08f1b8ae";

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

const headers = {
  "Content-Type": "application/fhir+json",
  Authorization: `Bearer ${access_token}`,
};

async function put(type: string, id: string, body: unknown) {
  const res = await fetch(`${BASE_URL}/fhir/${type}/${id}`, {
    method: "PUT",
    headers,
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    console.error(`seed ${type}/${id} failed: ${res.status} ${await res.text()}`);
    process.exit(1);
  }
}

// Referenced resources. They declare no meta.profile, which is the realistic
// plain-R4 shape: every targetProfile the base EOB declares names a base
// StructureDefinition, so the type-identity fast path is what has to carry it.
await put("Patient", PATIENT_ID, {
  resourceType: "Patient",
  id: PATIENT_ID,
  name: [{ family: "Bench", given: ["Target"] }],
  gender: "male",
  birthDate: "1980-01-01",
});
await put("Practitioner", PRACTITIONER_ID, {
  resourceType: "Practitioner",
  id: PRACTITIONER_ID,
  name: [{ family: "Provider", given: ["Bench"] }],
});
await put("Encounter", ENCOUNTER_ID, {
  resourceType: "Encounter",
  id: ENCOUNTER_ID,
  status: "finished",
  class: { system: "http://terminology.hl7.org/CodeSystem/v3-ActCode", code: "AMB" },
  subject: { reference: `Patient/${PATIENT_ID}` },
});
await put("Claim", CLAIM_ID, {
  resourceType: "Claim",
  id: CLAIM_ID,
  status: "active",
  type: { coding: [{ system: "http://terminology.hl7.org/CodeSystem/claim-type", code: "institutional" }] },
  use: "claim",
  patient: { reference: `Patient/${PATIENT_ID}` },
  created: "2017-11-04T16:51:19+03:00",
  provider: { reference: `Practitioner/${PRACTITIONER_ID}` },
  priority: { coding: [{ code: "normal" }] },
  insurance: [
    { sequence: 1, focal: true, coverage: { display: "private" } },
  ],
});

console.log("seeded referenced resources");

async function writeEob(id: string): Promise<number> {
  const body = { ...EOB, id };
  const started = performance.now();
  const res = await fetch(`${BASE_URL}/fhir/ExplanationOfBenefit/${id}`, {
    method: "PUT",
    headers,
    body: JSON.stringify(body),
  });
  const elapsed = performance.now() - started;
  if (!res.ok) {
    console.error(`EOB write failed: ${res.status} ${await res.text()}`);
    process.exit(1);
  }
  return elapsed;
}

for (let i = 0; i < WARMUP; i++) await writeEob(`bench-warmup-${i}`);

const samples: number[] = [];
for (let i = 0; i < ITERATIONS; i++) samples.push(await writeEob(`bench-eob-${i}`));

samples.sort((a, b) => a - b);
const pct = (p: number) => samples[Math.min(samples.length - 1, Math.floor((p / 100) * samples.length))];
const mean = samples.reduce((a, b) => a + b, 0) / samples.length;

console.log(`\nEOB writes: ${ITERATIONS} (serial, warmup ${WARMUP})`);
console.log(`  mean  ${mean.toFixed(2)} ms`);
console.log(`  p50   ${pct(50).toFixed(2)} ms`);
console.log(`  p95   ${pct(95).toFixed(2)} ms`);
console.log(`  p99   ${pct(99).toFixed(2)} ms`);
console.log(`  min   ${samples[0].toFixed(2)} ms   max ${samples[samples.length - 1].toFixed(2)} ms`);
