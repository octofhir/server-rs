/**
 * Verifies the "group bundle entries by resource type, one multi-row INSERT per
 * type" theory: posts a transaction Bundle with N Observations + N Conditions
 * and reports wall-clock time. Pair with postgres `log_statement=all` to count
 * the actual number of INSERT round-trips.
 *
 * Usage: bun run scripts/bench-bundle-batch.ts [baseUrl] [n]
 */

const BASE_URL = process.argv[2] ?? "http://localhost:8889";
const N = Number(process.argv[3] ?? 100);
/** "post" builds create-only entries; "put" re-sends the same ids as updates. */
const MODE = (process.argv[4] ?? "post") as "post" | "put";

async function token(): Promise<string | null> {
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
  if (!res.ok) return null;
  const body = (await res.json()) as { access_token?: string };
  return body.access_token ?? null;
}

function uuid(): string {
  return `urn:uuid:${crypto.randomUUID()}`;
}

function buildBundle(n: number) {
  const patientUrn = uuid();
  const entry: unknown[] = [
    {
      fullUrl: patientUrn,
      resource: {
        resourceType: "Patient",
        name: [{ family: "BatchBench", given: ["Theory"] }],
        gender: "unknown",
        birthDate: "1980-01-01",
      },
      request: { method: "POST", url: "Patient" },
    },
  ];

  for (let i = 0; i < n; i++) {
    entry.push({
      fullUrl: uuid(),
      resource: {
        resourceType: "Observation",
        status: "final",
        code: {
          coding: [
            { system: "http://loinc.org", code: "8867-4", display: "Heart rate" },
          ],
        },
        subject: { reference: patientUrn },
        effectiveDateTime: "2026-07-25T10:00:00Z",
        valueQuantity: {
          value: 60 + (i % 40),
          unit: "/min",
          system: "http://unitsofmeasure.org",
          code: "/min",
        },
      },
      request: { method: "POST", url: "Observation" },
    });
  }

  for (let i = 0; i < n; i++) {
    entry.push({
      fullUrl: uuid(),
      resource: {
        resourceType: "Condition",
        clinicalStatus: {
          coding: [
            {
              system:
                "http://terminology.hl7.org/CodeSystem/condition-clinical",
              code: "active",
            },
          ],
        },
        code: {
          coding: [
            { system: "http://snomed.info/sct", code: "44054006", display: "Diabetes" },
          ],
        },
        subject: { reference: patientUrn },
        onsetDateTime: "2026-07-25T10:00:00Z",
      },
      request: { method: "POST", url: "Condition" },
    });
  }

  return { resourceType: "Bundle", type: "transaction", entry };
}

const accessToken = await token();
const headers: Record<string, string> = {
  "Content-Type": "application/fhir+json",
};
if (accessToken) headers.Authorization = `Bearer ${accessToken}`;

const bundle = buildBundle(N);

if (MODE === "put") {
  // Turn every entry into a PUT against a fixed id, so the same bundle can be
  // replayed as an update-only transaction. Reference resolution still works
  // because PUT urls carry the resolved `Type/id`.
  const ids = new Map<string, string>();
  for (const e of bundle.entry as {
    fullUrl: string;
    resource: { resourceType: string; id?: string };
    request: { method: string; url: string };
  }[]) {
    const id = `bench-${e.resource.resourceType.toLowerCase()}-${ids.size}`;
    ids.set(e.fullUrl, `${e.resource.resourceType}/${id}`);
    e.resource.id = id;
    e.request = { method: "PUT", url: `${e.resource.resourceType}/${id}` };
  }
  const patched = JSON.stringify(bundle).replace(
    /urn:uuid:[0-9a-f-]{36}/g,
    (m) => ids.get(m) ?? m,
  );
  Object.assign(bundle, JSON.parse(patched));
}

const bytes = new TextEncoder().encode(JSON.stringify(bundle)).length;

const started = performance.now();
const res = await fetch(`${BASE_URL}/fhir`, {
  method: "POST",
  headers,
  body: JSON.stringify(bundle),
});
const elapsed = performance.now() - started;
const body = await res.json();

const entries = (body as { entry?: unknown[] }).entry ?? [];
const statuses = new Map<string, number>();
for (const e of entries as { response?: { status?: string } }[]) {
  const s = e.response?.status ?? "?";
  statuses.set(s, (statuses.get(s) ?? 0) + 1);
}

console.log(`url        ${BASE_URL}/fhir`);
console.log(`entries    ${(bundle.entry as unknown[]).length} (${N} Observation + ${N} Condition + 1 Patient)`);
console.log(`payload    ${(bytes / 1024).toFixed(1)} KiB`);
console.log(`http       ${res.status}`);
console.log(`elapsed    ${elapsed.toFixed(0)} ms`);
console.log(`responses  ${JSON.stringify(Object.fromEntries(statuses))}`);
if (!res.ok) console.log(JSON.stringify(body, null, 2).slice(0, 2000));
