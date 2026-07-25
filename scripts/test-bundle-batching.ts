/**
 * Behaviour tests for transaction-Bundle write batching: batched POST/PUT/DELETE
 * must be indistinguishable from the per-entry path, and the cases that cannot
 * be batched (If-Match, repeated ids, conditional urls) must still work.
 *
 * Usage: bun run scripts/test-bundle-batching.ts [baseUrl]
 */

const BASE_URL = process.argv[2] ?? "http://localhost:8889";

let token = "";
let passed = 0;
let failed = 0;

async function auth(): Promise<void> {
  const res = await fetch(`${BASE_URL}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "client_credentials",
      client_id: "backend",
      client_secret: "dev-secret-2024",
      scope: "system/*.cruds",
    }).toString(),
  });
  if (!res.ok) throw new Error(`token failed: ${res.status} ${await res.text()}`);
  token = ((await res.json()) as { access_token: string }).access_token;
}

async function tx(bundle: unknown): Promise<{ status: number; body: any }> {
  const res = await fetch(`${BASE_URL}/fhir`, {
    method: "POST",
    headers: {
      "Content-Type": "application/fhir+json",
      Authorization: `Bearer ${token}`,
    },
    body: JSON.stringify(bundle),
  });
  return { status: res.status, body: await res.json() };
}

async function read(type: string, id: string): Promise<{ status: number; body: any }> {
  const res = await fetch(`${BASE_URL}/fhir/${type}/${id}`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  return { status: res.status, body: await res.json() };
}

function bundleOf(entry: unknown[]) {
  return { resourceType: "Bundle", type: "transaction", entry };
}

function statuses(body: any): string[] {
  return (body.entry ?? []).map((e: any) => e.response?.status ?? "?");
}

function check(name: string, ok: boolean, detail = "") {
  if (ok) {
    passed++;
    console.log(`  ok   ${name}`);
  } else {
    failed++;
    console.log(`  FAIL ${name}${detail ? ` — ${detail}` : ""}`);
  }
}

const run = `b${Date.now().toString(36)}`;
const obsId = (i: number) => `${run}-obs-${i}`;
const patId = `${run}-pat`;

function patient(id: string, family: string) {
  return {
    resourceType: "Patient",
    id,
    identifier: [{ system: "urn:test:batching", value: `${run}-${family}` }],
    name: [{ family }],
    gender: "unknown",
  };
}

function observation(id: string, value: number, subject: string) {
  return {
    resourceType: "Observation",
    id,
    status: "final",
    code: { coding: [{ system: "http://loinc.org", code: "8867-4" }] },
    subject: { reference: subject },
    valueQuantity: {
      value,
      unit: "/min",
      system: "http://unitsofmeasure.org",
      code: "/min",
    },
  };
}

await auth();
console.log(`base ${BASE_URL}, run id ${run}\n`);

// Reference-existence validation is on, so the Patient every Observation below
// points at has to exist before those bundles are sent.
{
  const setup = await tx(
    bundleOf([
      {
        resource: patient(patId, "Anchor"),
        request: { method: "PUT", url: `Patient/${patId}` },
      },
    ]),
  );
  if (setup.status !== 200) {
    console.log(`setup failed: ${setup.status} ${JSON.stringify(setup.body).slice(0, 400)}`);
    process.exit(1);
  }
}

// ---------------------------------------------------------------- POST batch
console.log("POST batch + urn:uuid reference resolution");
{
  const patUrn = "urn:uuid:11111111-1111-1111-1111-111111111111";
  const entry = [
    {
      fullUrl: patUrn,
      resource: { ...patient(patId, "PostBatch"), id: undefined },
      request: { method: "POST", url: "Patient" },
    },
    ...[0, 1, 2].map((i) => ({
      fullUrl: `urn:uuid:2222222${i}-1111-1111-1111-111111111111`,
      resource: { ...observation(obsId(i), 60 + i, patUrn), id: undefined },
      request: { method: "POST", url: "Observation" },
    })),
  ];
  const { status, body } = await tx(bundleOf(entry));
  check("transaction accepted", status === 200, `status ${status}`);
  check(
    "all entries created",
    statuses(body).every((s) => s === "201 Created"),
    JSON.stringify(statuses(body)),
  );
  const createdPatient = body.entry?.[0]?.response?.location ?? "";
  const patientRef = createdPatient.split("/").slice(0, 2).join("/");
  const obsLoc = body.entry?.[1]?.response?.location ?? "";
  const obs = await read("Observation", obsLoc.split("/")[1] ?? "");
  check(
    "urn:uuid reference rewritten to Patient/id",
    obs.body?.subject?.reference === patientRef,
    `got ${obs.body?.subject?.reference}, want ${patientRef}`,
  );
}

// ------------------------------------------------- PUT batch (update-as-create)
console.log("\nPUT batch — update-as-create for unknown ids");
{
  const entry = [0, 1, 2].map((i) => ({
    resource: observation(obsId(`new${i}` as unknown as number), 70 + i, `Patient/${patId}`),
    request: { method: "PUT", url: `Observation/${obsId(`new${i}` as unknown as number)}` },
  }));
  const { status, body } = await tx(bundleOf(entry));
  check("transaction accepted", status === 200, `status ${status}`);
  check(
    "unknown ids created",
    statuses(body).every((s) => s === "201 Created"),
    JSON.stringify(statuses(body)),
  );
  const stored = await read("Observation", obsId("new0" as unknown as number));
  check("created row readable", stored.status === 200, `status ${stored.status}`);
  check(
    "value stored",
    stored.body?.valueQuantity?.value === 70,
    `got ${stored.body?.valueQuantity?.value}`,
  );
}

// ------------------------------------------------------------- PUT batch update
console.log("\nPUT batch — update of existing ids");
{
  const entry = [0, 1, 2].map((i) => ({
    resource: observation(obsId(`new${i}` as unknown as number), 90 + i, `Patient/${patId}`),
    request: { method: "PUT", url: `Observation/${obsId(`new${i}` as unknown as number)}` },
  }));
  const { status, body } = await tx(bundleOf(entry));
  check("transaction accepted", status === 200, `status ${status}`);
  check(
    "existing ids updated",
    statuses(body).every((s) => s === "200 OK"),
    JSON.stringify(statuses(body)),
  );
  const stored = await read("Observation", obsId("new1" as unknown as number));
  check(
    "new value visible",
    stored.body?.valueQuantity?.value === 91,
    `got ${stored.body?.valueQuantity?.value}`,
  );
  check(
    "versionId advanced",
    Number(stored.body?.meta?.versionId) > 0,
    `got ${stored.body?.meta?.versionId}`,
  );
}

// -------------------------------------------------- PUT batch, mixed hit/miss
console.log("\nPUT batch — mixed existing and unknown ids in one bundle");
{
  const entry = [
    {
      resource: observation(obsId("new0" as unknown as number), 111, `Patient/${patId}`),
      request: { method: "PUT", url: `Observation/${obsId("new0" as unknown as number)}` },
    },
    {
      resource: observation(obsId("mix" as unknown as number), 222, `Patient/${patId}`),
      request: { method: "PUT", url: `Observation/${obsId("mix" as unknown as number)}` },
    },
  ];
  const { status, body } = await tx(bundleOf(entry));
  check("transaction accepted", status === 200, `status ${status}`);
  check(
    "per-entry statuses correct",
    JSON.stringify(statuses(body)) === JSON.stringify(["200 OK", "201 Created"]),
    JSON.stringify(statuses(body)),
  );
}

// ------------------------------------------------ repeated id in the same PUT
console.log("\nPUT — same id twice in one bundle, last write wins");
{
  const id = obsId("dup" as unknown as number);
  const entry = [
    {
      resource: observation(id, 1, `Patient/${patId}`),
      request: { method: "PUT", url: `Observation/${id}` },
    },
    {
      resource: observation(id, 2, `Patient/${patId}`),
      request: { method: "PUT", url: `Observation/${id}` },
    },
  ];
  const { status } = await tx(bundleOf(entry));
  check("transaction accepted", status === 200, `status ${status}`);
  const stored = await read("Observation", id);
  check(
    "last entry won",
    stored.body?.valueQuantity?.value === 2,
    `got ${stored.body?.valueQuantity?.value}`,
  );
}

// ------------------------------------------------------------------- If-Match
console.log("\nPUT with If-Match — stays on the per-entry path");
{
  const id = obsId("etag" as unknown as number);
  await tx(
    bundleOf([
      {
        resource: observation(id, 5, `Patient/${patId}`),
        request: { method: "PUT", url: `Observation/${id}` },
      },
    ]),
  );
  const current = await read("Observation", id);
  const version = current.body?.meta?.versionId;

  const good = await tx(
    bundleOf([
      {
        resource: observation(id, 6, `Patient/${patId}`),
        request: { method: "PUT", url: `Observation/${id}`, ifMatch: `W/"${version}"` },
      },
    ]),
  );
  check("matching If-Match succeeds", statuses(good.body)[0] === "200 OK", JSON.stringify(statuses(good.body)));

  const bad = await tx(
    bundleOf([
      {
        resource: observation(id, 7, `Patient/${patId}`),
        request: { method: "PUT", url: `Observation/${id}`, ifMatch: 'W/"1"' },
      },
    ]),
  );
  check("stale If-Match rejected", bad.status === 409 || bad.status === 412, `status ${bad.status}`);
  const after = await read("Observation", id);
  check(
    "rejected transaction rolled back",
    after.body?.valueQuantity?.value === 6,
    `got ${after.body?.valueQuantity?.value}`,
  );
}

// -------------------------------------------------------- conditional create
console.log("\nConditional create (ifNoneExist) — batched pre-scan");
{
  const entry = [
    {
      resource: { ...patient(`${run}-cond`, "CondCreate"), id: undefined },
      request: { method: "POST", url: "Patient", ifNoneExist: `identifier=${run}-CondCreate` },
    },
  ];
  const first = await tx(bundleOf(entry));
  check("first run creates", statuses(first.body)[0] === "201 Created", JSON.stringify(statuses(first.body)));
  const location = first.body.entry?.[0]?.response?.location ?? "";

  const second = await tx(bundleOf(entry));
  check("second run reuses", statuses(second.body)[0] === "200 OK", JSON.stringify(statuses(second.body)));
  const secondId = second.body.entry?.[0]?.response?.location?.split("/")[1];
  check(
    "same resource returned",
    secondId === location.split("/")[1],
    `${secondId} vs ${location.split("/")[1]}`,
  );
}

// -------------------------------------------------------- conditional update
console.log("\nConditional update (PUT Type?query)");
{
  const ident = `${run}-CondUpdate`;
  const create = await tx(
    bundleOf([
      {
        resource: { ...patient(`${run}-cu`, "CondUpdate"), id: undefined },
        request: { method: "PUT", url: `Patient?identifier=${ident}` },
      },
    ]),
  );
  check("no match creates", statuses(create.body)[0] === "201 Created", JSON.stringify(statuses(create.body)));

  const update = await tx(
    bundleOf([
      {
        resource: { ...patient(`${run}-cu`, "CondUpdate"), id: undefined, gender: "female" },
        request: { method: "PUT", url: `Patient?identifier=${ident}` },
      },
    ]),
  );
  check("match updates", statuses(update.body)[0] === "200 OK", JSON.stringify(statuses(update.body)));
  const id = update.body.entry?.[0]?.response?.location?.split("/")[1];
  const stored = await read("Patient", id ?? "");
  check("update applied", stored.body?.gender === "female", `got ${stored.body?.gender}`);
}

// ------------------------------------------------------------- PUT validation
console.log("\nPUT validation — invalid body must be rejected");
{
  const { status, body } = await tx(
    bundleOf([
      {
        resource: {
          resourceType: "Observation",
          id: `${run}-invalid`,
          status: "not-a-valid-status",
          code: { coding: [{ system: "http://loinc.org", code: "8867-4" }] },
        },
        request: { method: "PUT", url: `Observation/${run}-invalid` },
      },
    ]),
  );
  check("invalid PUT rejected", status === 422, `status ${status}`);
  const stored = await read("Observation", `${run}-invalid`);
  check("nothing written", stored.status === 404, `status ${stored.status}`);
  if (status !== 422) console.log(`     ${JSON.stringify(body).slice(0, 300)}`);
}

// ------------------------------------------------------------- DELETE batch
console.log("\nDELETE batch");
{
  const ids = [0, 1, 2].map((i) => obsId(`del${i}` as unknown as number));
  await tx(
    bundleOf(
      ids.map((id) => ({
        resource: observation(id, 42, `Patient/${patId}`),
        request: { method: "PUT", url: `Observation/${id}` },
      })),
    ),
  );
  const { status, body } = await tx(
    bundleOf(ids.map((id) => ({ request: { method: "DELETE", url: `Observation/${id}` } }))),
  );
  check("transaction accepted", status === 200, `status ${status}`);
  check(
    "all deleted",
    statuses(body).every((s) => s === "204 No Content"),
    JSON.stringify(statuses(body)),
  );
  const gone = await read("Observation", ids[0]);
  check("row gone", gone.status === 404 || gone.status === 410, `status ${gone.status}`);

  const again = await tx(
    bundleOf(ids.map((id) => ({ request: { method: "DELETE", url: `Observation/${id}` } }))),
  );
  check(
    "delete is idempotent",
    statuses(again.body).every((s) => s === "204 No Content"),
    JSON.stringify(statuses(again.body)),
  );
}

// ------------------------------------------------- DELETE before PUT ordering
console.log("\nDELETE then PUT of the same id in one bundle");
{
  const id = obsId("order" as unknown as number);
  await tx(
    bundleOf([
      {
        resource: observation(id, 1, `Patient/${patId}`),
        request: { method: "PUT", url: `Observation/${id}` },
      },
    ]),
  );
  const { status } = await tx(
    bundleOf([
      { request: { method: "DELETE", url: `Observation/${id}` } },
      {
        resource: observation(id, 99, `Patient/${patId}`),
        request: { method: "PUT", url: `Observation/${id}` },
      },
    ]),
  );
  check("transaction accepted", status === 200, `status ${status}`);
  const stored = await read("Observation", id);
  check(
    "PUT applied after DELETE",
    stored.status === 200 && stored.body?.valueQuantity?.value === 99,
    `status ${stored.status}, value ${stored.body?.valueQuantity?.value}`,
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed === 0 ? 0 : 1);
