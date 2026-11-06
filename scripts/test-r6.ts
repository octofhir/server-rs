/**
 * R6 against a running server started with `[fhir] version = "R6"`: the CapabilityStatement,
 * embedded chains, history deletion, $purge and Measure/$evaluate.
 *
 * Usage: bun run scripts/test-r6.ts [baseUrl]
 */
const BASE_URL = process.argv[2] ?? "http://localhost:8888";

const tokenResponse = await fetch(`${BASE_URL}/auth/token`, {
  method: "POST",
  headers: { "Content-Type": "application/x-www-form-urlencoded" },
  body: new URLSearchParams({
    grant_type: "client_credentials",
    client_id: "backend",
    client_secret: "dev-secret-2024",
    scope: "system/*.cruds",
  }).toString(),
});
const { access_token: token } = (await tokenResponse.json()) as { access_token: string };

async function call(method: string, path: string, body?: unknown): Promise<[number, any]> {
  const response = await fetch(`${BASE_URL}/fhir${path}`, {
    method,
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/fhir+json",
      Accept: "application/fhir+json",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  return [response.status, text ? JSON.parse(text) : null];
}

let failures = 0;
function check(name: string, actual: unknown, expected: unknown) {
  const passed = JSON.stringify(actual) === JSON.stringify(expected);
  if (!passed) failures += 1;
  console.log(`${passed ? "ok  " : "FAIL"} ${name}${passed ? "" : `: ${JSON.stringify(actual)}`}`);
}

// CapabilityStatement
const [, metadata] = await call("GET", "/metadata");
check("fhirVersion", metadata.fhirVersion, "6.0.0-snapshot1");
const patientRest = metadata.rest[0].resource.find((r: any) => r.type === "Patient");
const codes = patientRest.interaction.map((i: any) => i.code);
for (const code of ["create-conditional", "delete-history", "delete-history-version"]) {
  check(`declares ${code}`, codes.includes(code), true);
}
check(
  "DeviceAlert declared",
  metadata.rest[0].resource.some((r: any) => r.type === "DeviceAlert"),
  true,
);
const [, outcome] = await call("POST", "/CapabilityStatement/$validate", metadata);
check(
  "CapabilityStatement valid",
  (outcome.issue ?? []).filter((issue: any) => issue.severity === "error"),
  [],
);

// Embedded chains
const [, patient] = await call("POST", "/Patient", {
  resourceType: "Patient",
  gender: "female",
  name: [{ family: "Six" }],
});
const [, composition] = await call("POST", "/Composition", {
  resourceType: "Composition",
  status: "final",
  type: { text: "Discharge" },
  subject: [{ reference: `Patient/${patient.id}` }],
  date: "2026-01-01",
  author: [{ display: "A" }],
  title: "Discharge",
});
await call("POST", "/Bundle", {
  resourceType: "Bundle",
  type: "document",
  identifier: { system: "urn:example", value: crypto.randomUUID() },
  timestamp: "2026-01-01T00:00:00Z",
  entry: [{ fullUrl: `urn:uuid:${crypto.randomUUID()}`, resource: composition }],
});
check("composition has no test", (await call("GET", "/Bundle?composition=x"))[0], 400);
check("chain by id", (await call("GET", `/Bundle?composition._id=${composition.id}`))[1].total, 1);
check(
  "chain by subject",
  (await call("GET", `/Bundle?composition.subject=Patient/${patient.id}`))[1].total,
  1,
);

// History
const put = (family: string) =>
  call("PUT", `/Patient/${patient.id}`, {
    resourceType: "Patient",
    id: patient.id,
    gender: "female",
    name: [{ family }],
  });
await put("Seven");
const [, current] = await put("Eight");
const versions = async () =>
  (await call("GET", `/Patient/${patient.id}/_history`))[1].entry.map(
    (entry: any) => entry.resource.meta.versionId,
  );
const all = await versions();
check("three versions", all.length, 3);
check(
  "current version stays",
  (await call("DELETE", `/Patient/${patient.id}/_history/${current.meta.versionId}`))[0],
  409,
);
check("old version goes", (await call("DELETE", `/Patient/${patient.id}/_history/${all[1]}`))[0], 204);
check("history goes", (await call("DELETE", `/Patient/${patient.id}/_history`))[0], 204);
check("current version left", await versions(), [current.meta.versionId]);

// Measure/$evaluate
const canonical = `http://example.org/r6-${crypto.randomUUID()}`;
const cql = `library R6 version '1.0.0'\nusing FHIR version '4.0.1'\ncontext Patient\ndefine "Pop": true\ndefine "Num": Patient.gender = 'female'\n`;
await call("POST", "/Library", {
  resourceType: "Library",
  url: `${canonical}/Library`,
  status: "active",
  type: { coding: [{ system: "http://terminology.hl7.org/CodeSystem/library-type", code: "logic-library" }] },
  content: [{ contentType: "text/cql", data: btoa(cql) }],
});
const population = (code: string, expression: string) => ({
  code: { coding: [{ system: "http://terminology.hl7.org/CodeSystem/measure-population", code }] },
  criteria: { language: "text/cql-identifier", expression },
});
await call("POST", "/Measure", {
  resourceType: "Measure",
  url: `${canonical}/Measure`,
  status: "active",
  library: [`${canonical}/Library`],
  group: [
    {
      scoring: { coding: [{ system: "http://terminology.hl7.org/CodeSystem/measure-scoring", code: "proportion" }] },
      population: [
        population("initial-population", "Pop"),
        population("denominator", "Pop"),
        population("numerator", "Num"),
      ],
    },
  ],
});
const evaluate = (reportType: string) =>
  call("POST", "/Measure/$evaluate", {
    resourceType: "Parameters",
    parameter: [
      { name: "measureUrl", valueCanonical: `${canonical}/Measure` },
      { name: "periodStart", valueDate: "2026" },
      { name: "periodEnd", valueDate: "2026" },
      { name: "subject", valueString: `Patient/${patient.id}` },
      { name: "reportType", valueCode: reportType },
    ],
  });
const [status, reports] = await evaluate("individual");
check("evaluate", status, 200);
const report = reports.parameter[0].resource.entry[0].resource;
check("individual report", report.type, "individual");
check("numerator", report.group[0].population[2].count, 1);
check("score", report.group[0].measureScoreQuantity?.value, 1);

const [, bySubjectGroup] = await call("POST", "/Measure/$evaluate", {
  resourceType: "Parameters",
  parameter: [
    { name: "measureUrl", valueCanonical: `${canonical}/Measure` },
    { name: "periodStart", valueDate: "2026" },
    { name: "periodEnd", valueDate: "2026" },
    {
      name: "subjectGroup",
      resource: {
        resourceType: "Group",
        type: "person",
        membership: "enumerated",
        member: [{ entity: { reference: `Patient/${patient.id}` } }],
      },
    },
    { name: "reporter", valueString: "Organization/reporting" },
  ],
});
const summary = bySubjectGroup.parameter?.[0]?.resource?.entry?.[0]?.resource;
check("subjectGroup gives a summary", summary?.type, "summary");
check("subjectGroup counts its member", summary?.group?.[0]?.population?.[0]?.count, 1);
check("reporter is carried", summary?.reporter?.reference, "Organization/reporting");

// $purge
const [, observation] = await call("POST", "/Observation", {
  resourceType: "Observation",
  status: "final",
  code: { text: "x" },
  subject: { reference: `Patient/${patient.id}` },
});
const [, group] = await call("POST", "/Group", {
  resourceType: "Group",
  type: "person",
  membership: "enumerated",
  member: [{ entity: { reference: `Patient/${patient.id}` } }],
});
const [, linked] = await call("POST", "/Patient", {
  resourceType: "Patient",
  link: [{ other: { reference: `Patient/${patient.id}` }, type: "replaced-by" }],
});
check(
  "enumerated Group has members",
  (await call("GET", `/Group/${group.id}/$everything`))[0],
  200,
);
const [, dry] = await call("GET", `/Patient/${patient.id}/$purge`);
check("dry run counts", dry.issue[0].diagnostics.startsWith("Would remove"), true);
check("dry run removes nothing", (await call("GET", `/Patient/${patient.id}`))[0], 200);
check("purge", (await call("POST", `/Patient/${patient.id}/$purge`, { resourceType: "Parameters" }))[0], 200);
check("patient gone", (await call("GET", `/Patient/${patient.id}`))[0], 404);
check("history gone", (await call("GET", `/Patient/${patient.id}/_history`))[0], 404);
check("record gone", (await call("GET", `/Observation/${observation.id}`))[0], 404);
check("linked patient gone", (await call("GET", `/Patient/${linked.id}`))[0], 404);
check("cohort stays", (await call("GET", `/Group/${group.id}`))[0], 200);

console.log(failures === 0 ? "all passed" : `${failures} failed`);
process.exit(failures === 0 ? 0 : 1);
