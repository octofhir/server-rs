const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const CLIENT_ID = process.env.CLIENT_ID || "backend";
const CLIENT_SECRET = process.env.CLIENT_SECRET || "dev-secret-2024";

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

async function searchRefIndex(
  resourceType: string,
  resourceId: string,
): Promise<number> {
  // Direct DB check via psql - just count index rows
  const proc = Bun.spawn(
    [
      "psql",
      "-h",
      "localhost",
      "-p",
      "5450",
      "-U",
      "postgres",
      "-d",
      "octofhir",
      "-t",
      "-c",
      `SELECT count(*) FROM search_idx_reference WHERE resource_type='${resourceType}' AND resource_id='${resourceId}'`,
    ],
    { env: { ...process.env, PGPASSWORD: "postgres" } },
  );
  const text = await new Response(proc.stdout).text();
  return parseInt(text.trim(), 10);
}

async function searchDateIndex(
  resourceType: string,
  resourceId: string,
): Promise<number> {
  const proc = Bun.spawn(
    [
      "psql",
      "-h",
      "localhost",
      "-p",
      "5450",
      "-U",
      "postgres",
      "-d",
      "octofhir",
      "-t",
      "-c",
      `SELECT count(*) FROM search_idx_date WHERE resource_type='${resourceType}' AND resource_id='${resourceId}'`,
    ],
    { env: { ...process.env, PGPASSWORD: "postgres" } },
  );
  const text = await new Response(proc.stdout).text();
  return parseInt(text.trim(), 10);
}

async function deleteIndexRows(
  resourceType: string,
  resourceId: string,
): Promise<void> {
  const proc = Bun.spawn(
    [
      "psql",
      "-h",
      "localhost",
      "-p",
      "5450",
      "-U",
      "postgres",
      "-d",
      "octofhir",
      "-c",
      `DELETE FROM search_idx_reference WHERE resource_type='${resourceType}' AND resource_id='${resourceId}'; DELETE FROM search_idx_date WHERE resource_type='${resourceType}' AND resource_id='${resourceId}';`,
    ],
    { env: { ...process.env, PGPASSWORD: "postgres" } },
  );
  await proc.exited;
}

function assert(condition: boolean, msg: string) {
  if (!condition) {
    console.error(`  FAIL: ${msg}`);
    process.exit(1);
  }
  console.log(`  PASS: ${msg}`);
}

async function main() {
  console.log("=== $reindex E2E Test ===\n");

  // 1. Get token
  console.log("Getting token...");
  const token = await getToken();
  console.log("Token obtained\n");

  // 2. Create test resources
  console.log("--- Setup: Creating test resources ---");
  const patient = await fhir(token, "PUT", "Patient/reindex-e2e-1", {
    resourceType: "Patient",
    id: "reindex-e2e-1",
    name: [{ family: "ReindexE2E", given: ["Test"] }],
    birthDate: "1990-05-15",
    generalPractitioner: [{ reference: "Practitioner/doc-1" }],
  });
  console.log(`  Patient: ${patient.status}`);
  assert(patient.status === 200 || patient.status === 201, "Patient created");

  const obs = await fhir(token, "PUT", "Observation/reindex-e2e-obs-1", {
    resourceType: "Observation",
    id: "reindex-e2e-obs-1",
    status: "final",
    code: { coding: [{ system: "http://loinc.org", code: "8867-4" }] },
    subject: { reference: "Patient/reindex-e2e-1" },
    effectiveDateTime: "2024-01-15T10:30:00Z",
    valueQuantity: { value: 72, unit: "bpm" },
  });
  console.log(`  Observation: ${obs.status}`);
  assert(obs.status === 200 || obs.status === 201, "Observation created");

  // 3. Verify indexes exist after create
  console.log("\n--- Verify: Indexes present after create ---");
  const patRefsBefore = await searchRefIndex("Patient", "reindex-e2e-1");
  const obsRefsBefore = await searchRefIndex("Observation", "reindex-e2e-obs-1");
  const obsDatesBefore = await searchDateIndex("Observation", "reindex-e2e-obs-1");
  console.log(
    `  Patient refs: ${patRefsBefore}, Obs refs: ${obsRefsBefore}, Obs dates: ${obsDatesBefore}`,
  );
  assert(patRefsBefore > 0, "Patient has reference index rows");
  assert(obsRefsBefore > 0, "Observation has reference index rows");
  assert(obsDatesBefore > 0, "Observation has date index rows");

  // 4. Delete index rows (simulate drift)
  console.log("\n--- Simulate: Delete index rows (drift) ---");
  await deleteIndexRows("Patient", "reindex-e2e-1");
  await deleteIndexRows("Observation", "reindex-e2e-obs-1");
  const patRefsAfterDelete = await searchRefIndex("Patient", "reindex-e2e-1");
  const obsRefsAfterDelete = await searchRefIndex("Observation", "reindex-e2e-obs-1");
  console.log(
    `  Patient refs: ${patRefsAfterDelete}, Obs refs: ${obsRefsAfterDelete}`,
  );
  assert(patRefsAfterDelete === 0, "Patient index rows deleted");
  assert(obsRefsAfterDelete === 0, "Observation index rows deleted");

  // 5. Test instance-level reindex (synchronous)
  console.log("\n--- Test 1: Instance-level $reindex (Patient) ---");
  const instanceResult = await fhir(
    token,
    "POST",
    "Patient/reindex-e2e-1/$reindex",
  );
  console.log(`  Status: ${instanceResult.status}`);
  console.log(`  Response: ${JSON.stringify(instanceResult.data)}`);
  assert(instanceResult.status === 200, "Instance reindex returns 200");

  const patRefsAfterReindex = await searchRefIndex("Patient", "reindex-e2e-1");
  console.log(`  Patient refs after reindex: ${patRefsAfterReindex}`);
  assert(
    patRefsAfterReindex > 0,
    "Patient reference indexes restored after instance reindex",
  );

  // 6. Test type-level reindex (async)
  console.log("\n--- Test 2: Type-level $reindex (Observation) ---");
  const typeResult = await fhir(token, "POST", "Observation/$reindex");
  console.log(`  Status: ${typeResult.status}`);
  console.log(`  Response: ${JSON.stringify(typeResult.data)}`);

  if (typeResult.status === 200 && typeResult.data.job_id) {
    // Async - poll for completion
    console.log(`  Job ID: ${typeResult.data.job_id}`);
    const jobResult = await pollJobStatus(token, typeResult.data.job_id);
    console.log(`  Job status: ${jobResult.status}`);
    assert(jobResult.status === "completed", "Type reindex job completed");
  } else if (typeResult.status === 202) {
    // 202 Accepted with Content-Location
    console.log("  Accepted (202)");
  } else {
    assert(typeResult.status === 200, `Type reindex returns 200, got ${typeResult.status}`);
  }

  const obsRefsAfterTypeReindex = await searchRefIndex(
    "Observation",
    "reindex-e2e-obs-1",
  );
  const obsDatesAfterTypeReindex = await searchDateIndex(
    "Observation",
    "reindex-e2e-obs-1",
  );
  console.log(
    `  Obs refs after type reindex: ${obsRefsAfterTypeReindex}, dates: ${obsDatesAfterTypeReindex}`,
  );
  assert(
    obsRefsAfterTypeReindex > 0,
    "Observation reference indexes restored after type reindex",
  );
  assert(
    obsDatesAfterTypeReindex > 0,
    "Observation date indexes restored after type reindex",
  );

  // 7. Test system-level reindex (async)
  console.log("\n--- Test 3: System-level $reindex ---");
  // Delete again to test system-level
  await deleteIndexRows("Patient", "reindex-e2e-1");
  const sysResult = await fhir(token, "POST", "$reindex");
  console.log(`  Status: ${sysResult.status}`);
  console.log(`  Response: ${JSON.stringify(sysResult.data)}`);

  if (sysResult.status === 200 && sysResult.data.job_id) {
    console.log(`  Job ID: ${sysResult.data.job_id}`);
    const jobResult = await pollJobStatus(token, sysResult.data.job_id, 60_000);
    console.log(`  Job status: ${jobResult.status}`);
    assert(jobResult.status === "completed", "System reindex job completed");
  }

  const patRefsAfterSysReindex = await searchRefIndex("Patient", "reindex-e2e-1");
  console.log(`  Patient refs after system reindex: ${patRefsAfterSysReindex}`);
  assert(
    patRefsAfterSysReindex > 0,
    "Patient indexes restored after system reindex",
  );

  // 8. Verify search actually works after reindex
  console.log("\n--- Test 4: Search verification ---");
  const searchResult = await fhir(
    token,
    "GET",
    "Observation?subject=Patient/reindex-e2e-1",
  );
  console.log(`  Search status: ${searchResult.status}`);
  const total = searchResult.data?.total ?? searchResult.data?.entry?.length ?? 0;
  console.log(`  Results: ${total}`);
  assert(searchResult.status === 200, "Search returns 200");
  assert(total > 0, "Search finds observation after reindex");

  // 9. Cleanup
  console.log("\n--- Cleanup ---");
  await fhir(token, "DELETE", "Observation/reindex-e2e-obs-1");
  await fhir(token, "DELETE", "Patient/reindex-e2e-1");
  console.log("  Resources deleted");

  console.log("\n=== All $reindex tests passed ===");
}

main().catch((err) => {
  console.error("\nFATAL:", err);
  process.exit(1);
});
