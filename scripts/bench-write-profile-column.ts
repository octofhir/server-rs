/**
 * Write-throughput cost of the generated `profile` column.
 *
 * Posts transaction Bundles of N Patients and reports per-bundle latency. Run
 * it twice against a recreated database — once with
 * `OCTOFHIR__STORAGE__POSTGRES__PROFILE_COLUMN=false`, once with the default —
 * to price the column and its two partial indexes on the real server write
 * path (validation, JSON, history triggers and all), not on a bare INSERT.
 *
 * `profiled` controls whether each resource declares a meta.profile: with none,
 * the column is NULL and both indexes stay empty, which is the plain-R4 case.
 *
 * Usage: bun run scripts/bench-write-profile-column.ts [rounds] [perBundle] [profiled|plain]
 */

const BASE_URL = "http://localhost:8888";
const ROUNDS = Number(process.argv[2] ?? 30);
const PER_BUNDLE = Number(process.argv[3] ?? 100);
const PROFILED = (process.argv[4] ?? "profiled") === "profiled";
const WARMUP = 5;

const PROFILES = [
  "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient",
  "http://example.org/StructureDefinition/local-patient",
];

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

function bundle(round: number) {
  const entry = [];
  for (let i = 0; i < PER_BUNDLE; i++) {
    const id = `wbench-${PROFILED ? "p" : "n"}-${round}-${i}`;
    const resource: Record<string, unknown> = {
      resourceType: "Patient",
      id,
      name: [{ family: `Bench${i}`, given: [`Round${round}`] }],
      gender: i % 2 === 0 ? "male" : "female",
      birthDate: "1980-01-01",
      address: [{ city: "Springfield", line: [`${i} Main St`], postalCode: "12345" }],
      telecom: [{ system: "phone", value: `555-${1000 + i}` }],
    };
    if (PROFILED) resource.meta = { profile: PROFILES };
    entry.push({
      fullUrl: `Patient/${id}`,
      resource,
      request: { method: "PUT", url: `Patient/${id}` },
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

console.log(`\npayload: ${PROFILED ? "profiled (2 canonicals each)" : "plain (no meta.profile)"}`);
console.log(`${ROUNDS} bundles x ${PER_BUNDLE} Patients = ${total} resources`);
console.log(`  mean  ${mean.toFixed(1)} ms/bundle`);
console.log(`  p50   ${pct(50).toFixed(1)} ms`);
console.log(`  p95   ${pct(95).toFixed(1)} ms`);
console.log(`  thrpt ${((total / wall) * 1000).toFixed(0)} resources/s`);
