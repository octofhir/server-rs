const BASE_URL = "http://localhost:8888";

async function getToken(): Promise<string> {
  const res = await fetch(`${BASE_URL}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: "grant_type=password&client_id=octofhir-ui&username=admin&password=admin123",
  });
  if (!res.ok) throw new Error(`Token failed: ${res.status} ${await res.text()}`);
  return (await res.json()).access_token;
}

function fhir(token: string) {
  const headers = {
    Authorization: `Bearer ${token}`,
    "Content-Type": "application/fhir+json",
    Accept: "application/fhir+json",
  };

  return {
    async create(resource: any) {
      const res = await fetch(`${BASE_URL}/fhir/${resource.resourceType}`, {
        method: "POST",
        headers,
        body: JSON.stringify(resource),
      });
      if (!res.ok) throw new Error(`Create ${resource.resourceType} failed: ${res.status} ${await res.text()}`);
      return res.json();
    },
    async search(query: string) {
      const res = await fetch(`${BASE_URL}/fhir/${query}`, { headers });
      if (!res.ok) throw new Error(`Search failed: ${res.status} ${await res.text()}`);
      return res.json();
    },
  };
}

// ─── Seed Data ───

async function seedData(api: ReturnType<typeof fhir>, count: number) {
  console.log(`\nSeeding ${count} patients with 5 observations each...`);
  const start = Date.now();
  let patients = 0;
  let observations = 0;

  const batch = [];
  for (let i = 0; i < count; i++) {
    batch.push(
      (async () => {
        const pat = await api.create({
          resourceType: "Patient",
          name: [{ family: `PerfTest-${i}`, given: [`Given-${i}`] }],
          birthDate: `${1950 + (i % 50)}-${String((i % 12) + 1).padStart(2, "0")}-15`,
          gender: i % 2 === 0 ? "male" : "female",
          identifier: [{ system: "http://test.org", value: `perf-${i}` }],
        });
        patients++;

        for (let j = 0; j < 5; j++) {
          await api.create({
            resourceType: "Observation",
            status: "final",
            code: { coding: [{ system: "http://loinc.org", code: "29463-7", display: "Body Weight" }] },
            subject: { reference: `Patient/${pat.id}` },
            effectiveDateTime: `2024-${String((j % 12) + 1).padStart(2, "0")}-15T10:00:00Z`,
            valueQuantity: { value: 60 + Math.random() * 40, unit: "kg", system: "http://unitsofmeasure.org", code: "kg" },
          });
          observations++;
        }
      })()
    );

    // Run in batches of 10 to avoid overwhelming the server
    if (batch.length >= 10) {
      await Promise.all(batch);
      batch.length = 0;
      process.stdout.write(`\r  Created ${patients} patients, ${observations} observations`);
    }
  }
  if (batch.length > 0) await Promise.all(batch);

  const elapsed = ((Date.now() - start) / 1000).toFixed(1);
  console.log(`\r  Created ${patients} patients, ${observations} observations in ${elapsed}s`);
  return { patients, observations };
}

// ─── Search Tests ───

interface SearchResult {
  query: string;
  total: number;
  latencyMs: number;
  ok: boolean;
}

async function runSearch(api: ReturnType<typeof fhir>, query: string): Promise<SearchResult> {
  const start = Date.now();
  const bundle = await api.search(query);
  const latencyMs = Date.now() - start;
  return {
    query,
    total: bundle.total ?? bundle.entry?.length ?? 0,
    latencyMs,
    ok: bundle.resourceType === "Bundle",
  };
}

async function runSearchBenchmark(api: ReturnType<typeof fhir>) {
  console.log("\n=== Search Benchmark ===\n");

  const searches = [
    // Simple string
    { name: "String (family)", query: "Patient?family=PerfTest-0&_count=10" },
    { name: "String (name)", query: "Patient?name=Given&_count=10" },

    // Token
    { name: "Token (code)", query: "Observation?code=29463-7&_count=10" },
    { name: "Token (system|code)", query: "Observation?code=http://loinc.org|29463-7&_count=10" },
    { name: "Token (gender)", query: "Patient?gender=male&_count=10" },

    // Date
    { name: "Date (eq)", query: "Patient?birthdate=1970-01-15&_count=10" },
    { name: "Date (gt)", query: "Patient?birthdate=gt1980-01-01&_count=10" },
    { name: "Date (lt)", query: "Observation?date=lt2024-06-01&_count=10" },

    // Reference
    { name: "Reference (subject)", query: "Observation?subject=Patient/nonexistent&_count=10" },

    // No filters (scan)
    { name: "Scan (Patient)", query: "Patient?_count=50" },
    { name: "Scan (Observation)", query: "Observation?_count=50" },

    // Sorting
    { name: "Sort (date)", query: "Observation?code=29463-7&_sort=-date&_count=20" },
    { name: "Sort (lastUpdated)", query: "Patient?_sort=-_lastUpdated&_count=20" },

    // Include/Revinclude
    { name: "Include", query: "Observation?code=29463-7&_include=Observation:subject&_count=10" },
    { name: "Revinclude", query: "Patient?family=PerfTest-0&_revinclude=Observation:subject&_count=10" },
  ];

  // Warmup
  console.log("Warming up...");
  for (const s of searches) {
    try {
      await runSearch(api, s.query);
    } catch {}
  }

  // Run each search 10 times
  const iterations = 10;
  console.log(`Running ${searches.length} searches x ${iterations} iterations\n`);

  for (const s of searches) {
    const latencies: number[] = [];
    let total = 0;
    let errors = 0;

    for (let i = 0; i < iterations; i++) {
      try {
        const result = await runSearch(api, s.query);
        latencies.push(result.latencyMs);
        total = result.total;
      } catch (e) {
        errors++;
      }
    }

    if (latencies.length === 0) {
      console.log(`  ${s.name.padEnd(25)} ERROR (${errors} failures)`);
      continue;
    }

    latencies.sort((a, b) => a - b);
    const p50 = latencies[Math.floor(latencies.length * 0.5)];
    const p95 = latencies[Math.floor(latencies.length * 0.95)];
    const avg = Math.round(latencies.reduce((a, b) => a + b, 0) / latencies.length);

    const totalStr = String(total).padStart(5);
    console.log(
      `  ${s.name.padEnd(25)} total=${totalStr}  avg=${String(avg).padStart(4)}ms  p50=${String(p50).padStart(4)}ms  p95=${String(p95).padStart(4)}ms`
    );
  }
}

// ─── Check Index Tables ───

async function checkIndexes() {
  console.log("\n=== Index Table Stats ===\n");

  const { execSync } = require("child_process");
  const psql = (sql: string) =>
    execSync(
      `PGPASSWORD=postgres psql -h localhost -p 5450 -U postgres -d octofhir -t -A -c "${sql}"`,
      { encoding: "utf8" }
    ).trim();

  const refCount = psql("SELECT count(*) FROM search_idx_reference");
  const dateCount = psql("SELECT count(*) FROM search_idx_date");
  const stringCount = psql("SELECT count(*) FROM search_idx_string");
  const patientCount = psql("SELECT count(*) FROM patient");
  const obsCount = psql("SELECT count(*) FROM observation");

  console.log(`  Resources:  ${patientCount} patients, ${obsCount} observations`);
  console.log(`  Indexes:    ${refCount} references, ${dateCount} dates, ${stringCount} strings`);

  // Sample some index data
  console.log("\n  Sample reference index:");
  const refSample = psql(
    "SELECT resource_type || '/' || resource_id || ' -> ' || param_code || ' -> ' || coalesce(target_type,'') || '/' || coalesce(target_id,'') FROM search_idx_reference LIMIT 5"
  );
  for (const line of refSample.split("\n")) {
    console.log(`    ${line}`);
  }

  console.log("\n  Sample string index:");
  const strSample = psql(
    "SELECT resource_type || '/' || resource_id || ' -> ' || param_code || ' = ' || value_normalized FROM search_idx_string LIMIT 5"
  );
  for (const line of strSample.split("\n")) {
    console.log(`    ${line}`);
  }
}

// ─── Main ───

async function main() {
  console.log("=== FHIR Search Performance Test ===\n");

  const token = await getToken();
  console.log("Token obtained ✓");

  const api = fhir(token);

  // Seed data
  await seedData(api, 100);

  // Check indexes
  await checkIndexes();

  // Run benchmark
  await runSearchBenchmark(api);

  console.log("\nDone!");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
