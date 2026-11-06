/**
 * End-to-end Bulk Data checks with real tokens: $export, its status URL and files,
 * and $bulk-submit with a provider served from this script.
 *
 * The server needs the client to hold system/bulk-submit and the submitter below in
 * bulk_submit.submitters, and must be able to reach this machine (server.allow_private_outbound).
 *
 * Usage: BASE_URL=http://localhost:8891 bun run scripts/test-bulk-data.ts
 */
const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const CLIENT_ID = process.env.CLIENT_ID || "backend";
const CLIENT_SECRET = process.env.CLIENT_SECRET || "dev-secret-2024";
const SUBMITTER = { system: process.env.SUBMITTER_SYSTEM || "urn:test:submitters", value: process.env.SUBMITTER_VALUE || "provider-a" };
const PROVIDER_PORT = Number(process.env.PROVIDER_PORT || 8899);
const FHIR = `${BASE_URL}/fhir`;
const RUN = Date.now().toString(36);

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown) {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok || detail === undefined ? "" : ` -> ${JSON.stringify(detail)}`}`);
  if (!ok) failures += 1;
}

async function token(scope: string): Promise<string> {
  const res = await fetch(`${BASE_URL}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "client_credentials", client_id: CLIENT_ID, client_secret: CLIENT_SECRET, scope }),
  });
  if (!res.ok) throw new Error(`Token request failed: ${res.status} ${await res.text()}`);
  return ((await res.json()) as { access_token: string }).access_token;
}

function call(bearer: string, method: string, url: string, body?: object, headers: Record<string, string> = {}) {
  return fetch(url.startsWith("http") ? url : `${FHIR}/${url}`, {
    method,
    headers: {
      Accept: "application/fhir+json",
      "Content-Type": "application/fhir+json",
      Authorization: `Bearer ${bearer}`,
      ...headers,
    },
    body: body ? JSON.stringify(body) : undefined,
  });
}

const parameters = (parameter: object[]) => ({ resourceType: "Parameters", parameter });

async function finish(bearer: string, statusUrl: string): Promise<{ status: number; body: any }> {
  for (let attempt = 0; attempt < 240; attempt += 1) {
    const res = await call(bearer, "GET", statusUrl, undefined, { Accept: "application/json" });
    if (res.status !== 202 && res.status !== 429) return { status: res.status, body: await res.json() };
    await Bun.sleep(1100);
  }
  throw new Error(`No result from ${statusUrl}`);
}

async function ndjson(bearer: string, url: string): Promise<any[]> {
  const res = await call(bearer, "GET", url, undefined, { Accept: "application/fhir+ndjson" });
  return (await res.text()).trim().split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

const admin = await token("system/*.cruds");
const patientId = `bulk-${RUN}`;
await call(admin, "PUT", `Patient/${patientId}`, { resourceType: "Patient", id: patientId, gender: "female" });
await call(admin, "PUT", `Observation/${patientId}`, {
  resourceType: "Observation", id: patientId, status: "final", code: { text: "x" }, subject: { reference: `Patient/${patientId}` },
});

// Export with a token: status and files need it too.
const kick = await call(admin, "POST", "Patient/$export", parameters([
  { name: "_type", valueString: "Patient" },
  { name: "_type", valueString: "Observation" },
  { name: "patient", valueReference: { reference: `Patient/${patientId}` } },
  { name: "_elements", valueString: "id" },
  { name: "organizeOutputBy", valueString: "Patient" },
]), { Prefer: "respond-async" });
check("kick-off is 202", kick.status === 202, await kick.clone().text());
const statusUrl = kick.headers.get("content-location") ?? "";
const done = await finish(admin, statusUrl);
check("export completes", done.status === 200, done.body);
check("manifest requires a token", done.body.requiresAccessToken === true);
check("manifest is organized", done.body.outputOrganizedBy === "Patient");
const file = done.body.output[0].url as string;
const anonymous = await fetch(file);
check("file needs a token", anonymous.status === 401, anonymous.status);
const block = await ndjson(admin, file);
check("block starts with its header", block[0]?.parameter?.[0]?.name === "header", block[0]);
check("elements are subsetted", block[1]?.meta?.tag?.some((tag: any) => tag.code === "SUBSETTED"), block[1]);
const remove = await call(admin, "DELETE", statusUrl);
check("delete is 202", remove.status === 202, remove.status);
const after = await call(admin, "GET", statusUrl);
check("status is 404 after delete", after.status === 404, after.status);

// Bulk submit with system/bulk-submit: no resource scope is involved.
const submitToken = await token("system/bulk-submit");
const incoming = `sub-${RUN}`;
const provider = Bun.serve({
  port: PROVIDER_PORT,
  fetch(request) {
    const path = new URL(request.url).pathname;
    if (path === "/manifest.json") {
      return Response.json({
        transactionTime: new Date().toISOString(),
        requiresAccessToken: false,
        output: [{ type: "Patient", url: `http://localhost:${PROVIDER_PORT}/Patient.ndjson` }],
      });
    }
    if (path === "/Patient.ndjson") {
      if (request.headers.get("x-file-key") !== "secret") return new Response("no key", { status: 403 });
      return new Response(
        `${JSON.stringify({ resourceType: "Patient", id: incoming, gender: "male" })}\n${JSON.stringify({ resourceType: "Patient", id: `${incoming}-bad`, gender: "nonsense" })}\n`,
      );
    }
    return new Response("not found", { status: 404 });
  },
});

const identity = [
  { name: "submitter", valueIdentifier: SUBMITTER },
  { name: "submissionId", valueString: `run-${RUN}` },
];
const manifestUrl = `http://localhost:${PROVIDER_PORT}/manifest.json`;
const forbidden = await call(await token("system/Patient.rs"), "POST", "$bulk-submit", parameters([...identity, { name: "manifestUrl", valueUrl: manifestUrl }, { name: "fhirBaseUrl", valueUrl: "http://example.org/fhir" }]));
check("read-only token cannot submit", forbidden.status === 403, forbidden.status);
const submitted = await call(submitToken, "POST", "$bulk-submit", parameters([
  ...identity,
  { name: "manifestUrl", valueUrl: manifestUrl },
  { name: "fhirBaseUrl", valueUrl: `http://localhost:${PROVIDER_PORT}/fhir` },
  { name: "fileRequestHeader", part: [{ name: "headerName", valueString: "x-file-key" }, { name: "headerValue", valueString: "secret" }] },
]));
check("submit is 200", submitted.status === 200, await submitted.clone().text());
const closed = await call(submitToken, "POST", "$bulk-submit", parameters([
  ...identity,
  { name: "submissionStatus", valueCoding: { system: "http://hl7.org/fhir/event-status", code: "completed" } },
]));
check("completing is 200", closed.status === 200, closed.status);
const statusKick = await call(submitToken, "POST", "$bulk-submit-status", parameters(identity), { Prefer: "respond-async" });
check("status kick-off is 202", statusKick.status === 202, await statusKick.clone().text());
const report = await finish(submitToken, statusKick.headers.get("content-location") ?? "");
check("status completes", report.status === 200, report.body);
check("status names the manifest", report.body.outcome?.[0]?.manifestUrl === manifestUrl, report.body);
const outcomes = await ndjson(submitToken, report.body.outcome[0].url);
check("invalid resource is reported", outcomes.some((o) => o.extension?.[0]?.valueReference?.reference?.endsWith(`Patient/${incoming}-bad`)), outcomes);
const stored = await call(admin, "GET", `Patient/${incoming}`);
check("valid resource is stored", stored.status === 200, stored.status);
const rejected = await call(admin, "GET", `Patient/${incoming}-bad`);
check("invalid resource is not stored", rejected.status === 404, rejected.status);

provider.stop(true);
console.log(failures === 0 ? "\nAll checks passed" : `\n${failures} checks failed`);
process.exit(failures === 0 ? 0 : 1);
