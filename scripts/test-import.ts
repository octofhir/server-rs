const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const CLIENT_ID = process.env.CLIENT_ID || "backend";
const CLIENT_SECRET = process.env.CLIENT_SECRET || "dev-secret-2024";

// NDJSON data for import test
const PATIENT_NDJSON = [
  JSON.stringify({
    resourceType: "Patient",
    id: "import-test-p1",
    name: [{ family: "ImportTest", given: ["Alice"] }],
    birthDate: "1985-03-20",
  }),
  JSON.stringify({
    resourceType: "Patient",
    id: "import-test-p2",
    name: [{ family: "ImportTest", given: ["Bob"] }],
    birthDate: "1990-07-15",
  }),
  JSON.stringify({
    resourceType: "Patient",
    id: "import-test-p3",
    name: [{ family: "ImportTest", given: ["Charlie"] }],
    birthDate: "1978-11-02",
  }),
].join("\n");

const OBS_NDJSON = [
  JSON.stringify({
    resourceType: "Observation",
    id: "import-test-obs1",
    status: "final",
    code: { coding: [{ system: "http://loinc.org", code: "8867-4" }] },
    subject: { reference: "Patient/import-test-p1" },
    effectiveDateTime: "2024-06-01T10:00:00Z",
    valueQuantity: { value: 72, unit: "bpm" },
  }),
  JSON.stringify({
    resourceType: "Observation",
    id: "import-test-obs2",
    status: "final",
    code: { coding: [{ system: "http://loinc.org", code: "8867-4" }] },
    subject: { reference: "Patient/import-test-p2" },
    effectiveDateTime: "2024-06-02T10:00:00Z",
    valueQuantity: { value: 80, unit: "bpm" },
  }),
].join("\n");

// Start a tiny HTTP server to serve NDJSON files
let ndjsonServer: ReturnType<typeof Bun.serve> | null = null;

function startNdjsonServer(): number {
  ndjsonServer = Bun.serve({
    port: 0, // random port
    fetch(req) {
      const url = new URL(req.url);
      if (url.pathname === "/patients.ndjson") {
        return new Response(PATIENT_NDJSON, {
          headers: { "Content-Type": "application/fhir+ndjson" },
        });
      }
      if (url.pathname === "/observations.ndjson") {
        return new Response(OBS_NDJSON, {
          headers: { "Content-Type": "application/fhir+ndjson" },
        });
      }
      return new Response("Not found", { status: 404 });
    },
  });
  return ndjsonServer.port;
}

async function getToken(): Promise<string> {
  const body = new URLSearchParams({
    grant_type: "client_credentials",
    client_id: CLIENT_ID,
    client_secret: CLIENT_SECRET,
    scope: "system/*.cruds",
  });

  const res = await fetch(`${BASE_URL}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });

  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Token request failed: ${res.status} ${text}`);
  }

  const data = (await res.json()) as { access_token: string };
  return data.access_token;
}

async function fhir(
  token: string,
  method: string,
  path: string,
  body?: object,
): Promise<{ status: number; data: any }> {
  const res = await fetch(`${BASE_URL}/fhir/${path}`, {
    method,
    headers: {
      "Content-Type": "application/fhir+json",
      Accept: "application/fhir+json",
      Authorization: `Bearer ${token}`,
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  let data: any;
  try {
    data = JSON.parse(text);
  } catch {
    data = text;
  }
  return { status: res.status, data };
}

async function pollJobStatus(
  token: string,
  jobId: string,
  maxWaitMs = 30_000,
): Promise<any> {
  const start = Date.now();
  while (Date.now() - start < maxWaitMs) {
    const res = await fetch(`${BASE_URL}/fhir/_async-status/${jobId}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const data = (await res.json()) as any;
    if (data.status === "completed" || data.status === "failed") {
      return data;
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`Job ${jobId} did not complete within ${maxWaitMs}ms`);
}

function assert(condition: boolean, msg: string) {
  if (!condition) {
    console.error(`  FAIL: ${msg}`);
    ndjsonServer?.stop();
    process.exit(1);
  }
  console.log(`  PASS: ${msg}`);
}

async function main() {
  console.log("=== $import E2E Test ===\n");

  // 1. Start NDJSON server
  const port = startNdjsonServer();
  console.log(`NDJSON server started on port ${port}\n`);

  // 2. Get token
  console.log("Getting token...");
  const token = await getToken();
  console.log("Token obtained\n");

  // 3. Cleanup any leftover test resources
  console.log("--- Cleanup: Removing leftover test resources ---");
  for (const id of [
    "import-test-p1",
    "import-test-p2",
    "import-test-p3",
  ]) {
    await fhir(token, "DELETE", `Patient/${id}`);
  }
  for (const id of ["import-test-obs1", "import-test-obs2"]) {
    await fhir(token, "DELETE", `Observation/${id}`);
  }
  console.log("  Done\n");

  // 4. Submit $import with Parameters format
  console.log("--- Test 1: $import with simplified JSON format ---");
  const importResult = await fhir(token, "POST", "$import", {
    input: [
      { type: "Patient", url: `http://localhost:${port}/patients.ndjson` },
      {
        type: "Observation",
        url: `http://localhost:${port}/observations.ndjson`,
      },
    ],
  });
  console.log(`  Status: ${importResult.status}`);
  console.log(`  Response: ${JSON.stringify(importResult.data)}`);
  assert(importResult.status === 200, "$import returns 200");
  assert(!!importResult.data.job_id, "Job ID returned");

  // 5. Poll for completion
  const jobId = importResult.data.job_id;
  console.log(`  Job ID: ${jobId}`);
  const jobResult = await pollJobStatus(token, jobId);
  console.log(`  Job status: ${jobResult.status}`);
  assert(jobResult.status === "completed", "Import job completed");

  // 6. Verify resources were created
  console.log("\n--- Test 2: Verify imported resources ---");
  const p1 = await fhir(token, "GET", "Patient/import-test-p1");
  console.log(`  Patient/import-test-p1: ${p1.status}`);
  assert(p1.status === 200, "Patient p1 exists");
  assert(
    p1.data?.name?.[0]?.family === "ImportTest",
    "Patient p1 has correct name",
  );

  const p2 = await fhir(token, "GET", "Patient/import-test-p2");
  assert(p2.status === 200, "Patient p2 exists");

  const p3 = await fhir(token, "GET", "Patient/import-test-p3");
  assert(p3.status === 200, "Patient p3 exists");

  const obs1 = await fhir(token, "GET", "Observation/import-test-obs1");
  assert(obs1.status === 200, "Observation obs1 exists");

  const obs2 = await fhir(token, "GET", "Observation/import-test-obs2");
  assert(obs2.status === 200, "Observation obs2 exists");

  // 7. Verify search indexes work (reference search)
  console.log("\n--- Test 3: Search verification ---");
  const searchResult = await fhir(
    token,
    "GET",
    "Observation?subject=Patient/import-test-p1",
  );
  console.log(`  Search status: ${searchResult.status}`);
  const total =
    searchResult.data?.total ?? searchResult.data?.entry?.length ?? 0;
  console.log(`  Results: ${total}`);
  assert(searchResult.status === 200, "Search returns 200");
  assert(total >= 1, "Search finds imported observation");

  // 8. Test FHIR Parameters format
  console.log("\n--- Test 4: $import with FHIR Parameters format ---");
  // Clean up first
  for (const id of [
    "import-test-p1",
    "import-test-p2",
    "import-test-p3",
  ]) {
    await fhir(token, "DELETE", `Patient/${id}`);
  }
  for (const id of ["import-test-obs1", "import-test-obs2"]) {
    await fhir(token, "DELETE", `Observation/${id}`);
  }

  const parametersResult = await fhir(token, "POST", "$import", {
    resourceType: "Parameters",
    parameter: [
      {
        name: "input",
        part: [
          { name: "type", valueString: "Patient" },
          {
            name: "url",
            valueUrl: `http://localhost:${port}/patients.ndjson`,
          },
        ],
      },
    ],
  });
  console.log(`  Status: ${parametersResult.status}`);
  assert(parametersResult.status === 200, "Parameters format accepted");

  const job2 = await pollJobStatus(token, parametersResult.data.job_id);
  assert(job2.status === "completed", "Parameters import completed");

  const p1Again = await fhir(token, "GET", "Patient/import-test-p1");
  assert(p1Again.status === 200, "Patient reimported via Parameters format");

  // 9. Cleanup
  console.log("\n--- Cleanup ---");
  for (const id of [
    "import-test-p1",
    "import-test-p2",
    "import-test-p3",
  ]) {
    await fhir(token, "DELETE", `Patient/${id}`);
  }
  for (const id of ["import-test-obs1", "import-test-obs2"]) {
    await fhir(token, "DELETE", `Observation/${id}`);
  }
  console.log("  Resources deleted");

  ndjsonServer?.stop();
  console.log("\n=== All $import tests passed ===");
}

main().catch((err) => {
  console.error("\nFATAL:", err);
  ndjsonServer?.stop();
  process.exit(1);
});
