// The SQL on FHIR console's server side, end to end against a running server:
//   bun run scripts/test-sof-console.ts            (BASE_URL defaults to :8888)
//
// - a stored ViewDefinition, ValueSet (with expansion) and ConceptMap;
// - $sql-describe of a SQLQuery over them: result columns, each dependency's
//   kind, resolved version, columns and terminology provenance, and `check`
//   for SQL that does not prepare;
// - $sql-run of the query with a bound parameter, the x-sof-terminology header;
// - the SQLQuery saved as a Library and found by its type;
// - the SQL language server in SQL on FHIR mode (/api/lsp/pg?mode=sof): the
//   dependencies as relations, parameters, the sandbox check, completion.

const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const LIBRARY_TYPES = "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes";

let failures = 0;
function check(condition: unknown, message: string, detail?: unknown) {
  if (condition) {
    console.log(`  ok   ${message}`);
  } else {
    failures++;
    console.log(`  FAIL ${message}`, detail === undefined ? "" : JSON.stringify(detail).slice(0, 600));
  }
}

async function getToken(): Promise<string> {
  const res = await fetch(`${BASE_URL}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "password",
      username: "admin",
      password: "admin123",
      client_id: "octofhir-ui",
      scope: "openid user/*.cruds",
    }).toString(),
  });
  if (!res.ok) throw new Error(`token failed: ${res.status} ${await res.text()}`);
  return ((await res.json()) as { access_token: string }).access_token;
}

const token = await getToken();
const headers = {
  Authorization: `Bearer ${token}`,
  "Content-Type": "application/fhir+json",
};

async function put(resource: Record<string, unknown>) {
  const res = await fetch(`${BASE_URL}/fhir/${resource.resourceType}/${resource.id}`, {
    method: "PUT",
    headers,
    body: JSON.stringify(resource),
  });
  if (!res.ok) throw new Error(`PUT ${resource.resourceType}/${resource.id}: ${res.status} ${await res.text()}`);
  return res.json();
}

async function operation(name: string, parameter: unknown[], accept = "application/fhir+json") {
  const res = await fetch(`${BASE_URL}/fhir/${name}`, {
    method: "POST",
    headers: { ...headers, Accept: accept },
    body: JSON.stringify({ resourceType: "Parameters", parameter }),
  });
  const text = await res.text();
  let body: unknown = text;
  try {
    body = JSON.parse(text);
  } catch {
    // not JSON
  }
  return { status: res.status, headers: res.headers, body };
}

type Part = { name: string; valueString?: string; valueCode?: string; part?: Part[] };
const partsOf = (body: unknown): Part[] => ((body as { parameter?: Part[] }).parameter ?? []);
const valueOf = (parts: Part[], name: string) => {
  const p = parts.find((x) => x.name === name);
  return p?.valueString ?? p?.valueCode;
};
const columnsOf = (parts: Part[]) =>
  parts.filter((p) => p.name === "column").map((c) => `${valueOf(c.part ?? [], "name")}:${valueOf(c.part ?? [], "type")}`);
const dependencyOf = (body: unknown, label: string) =>
  partsOf(body)
    .filter((p) => p.name === "dependency")
    .map((p) => p.part ?? [])
    .find((parts) => valueOf(parts, "label") === label) ?? [];

function sqlLibrary(sql: string, extra: Record<string, unknown> = {}) {
  return {
    resourceType: "Library",
    url: "http://example.org/Library/console-check",
    name: "console_check",
    status: "draft",
    type: { coding: [{ system: LIBRARY_TYPES, code: "sql-query" }] },
    relatedArtifact: [
      { type: "depends-on", label: "patient", resource: "http://example.org/ViewDefinition/console-patients" },
      { type: "depends-on", label: "genders", resource: "http://example.org/ValueSet/console-genders|1" },
      { type: "depends-on", label: "gender_map", resource: "http://example.org/ConceptMap/console-gender-map|1" },
    ],
    parameter: [{ name: "g", use: "in", type: "code", max: "1" }],
    content: [
      {
        contentType: "application/sql",
        extension: [{ url: "http://hl7.org/fhir/uv/sql-on-fhir/StructureDefinition/sql-text", valueString: sql }],
        data: Buffer.from(sql).toString("base64"),
      },
    ],
    ...extra,
  };
}

console.log(`SQL on FHIR console checks against ${BASE_URL}`);

// Artifacts the query depends on.
await put({
  resourceType: "ViewDefinition",
  id: "console-patients",
  url: "http://example.org/ViewDefinition/console-patients",
  name: "console_patients",
  status: "active",
  resource: "Patient",
  select: [
    {
      column: [
        { name: "id", path: "getResourceKey()", type: "id" },
        { name: "gender", path: "gender", type: "code" },
        { name: "birth_date", path: "birthDate", type: "date" },
      ],
    },
  ],
});
await put({
  resourceType: "ValueSet",
  id: "console-genders",
  url: "http://example.org/ValueSet/console-genders",
  version: "1",
  status: "active",
  expansion: {
    timestamp: "2026-11-04T00:00:00Z",
    total: 2,
    contains: [
      { system: "http://hl7.org/fhir/administrative-gender", code: "female", display: "Female" },
      { system: "http://hl7.org/fhir/administrative-gender", code: "male", display: "Male" },
    ],
  },
});
await put({
  resourceType: "ConceptMap",
  id: "console-gender-map",
  url: "http://example.org/ConceptMap/console-gender-map",
  version: "1",
  status: "active",
  group: [
    {
      source: "http://hl7.org/fhir/administrative-gender",
      target: "http://terminology.hl7.org/CodeSystem/v3-AdministrativeGender",
      element: [
        { code: "female", target: [{ code: "F", equivalence: "equivalent" }] },
        { code: "male", target: [{ code: "M", equivalence: "equivalent" }] },
      ],
    },
  ],
});

const SQL =
  "SELECT patient.id, patient.gender, gender_map.target_code AS v3\n" +
  "FROM patient\n" +
  "JOIN gender_map ON gender_map.source_code = patient.gender\n" +
  "WHERE EXISTS (SELECT 1 FROM genders WHERE genders.code = patient.gender)\n" +
  "  AND patient.gender = :g\n" +
  "ORDER BY patient.id";

console.log("$sql-describe");
{
  const { status, body } = await operation("$sql-describe", [{ name: "subjectResource", resource: sqlLibrary(SQL) }]);
  check(status === 200, "describes the query", body);
  check(
    JSON.stringify(columnsOf(partsOf(body))) === JSON.stringify(["id:text", "gender:text", "v3:varchar"]),
    "result columns with PostgreSQL types",
    columnsOf(partsOf(body))
  );
  const patient = dependencyOf(body, "patient");
  check(valueOf(patient, "kind") === "ViewDefinition", "patient is a ViewDefinition", patient);
  check(columnsOf(patient).length === 3, "the view's columns", columnsOf(patient));
  const genders = dependencyOf(body, "genders");
  check(valueOf(genders, "kind") === "ValueSet", "genders is a ValueSet", genders);
  check(valueOf(genders, "resolved") === "http://example.org/ValueSet/console-genders|1", "resolved version", genders);
  const provenance = JSON.parse(valueOf(genders, "provenance") ?? "{}");
  check(provenance.members === 2 && provenance.source, "value set provenance (members, source)", provenance);
  const map = dependencyOf(body, "gender_map");
  check(valueOf(map, "kind") === "ConceptMap" && columnsOf(map).length === 9, "concept map, 9 columns", map);
  check(JSON.parse(valueOf(map, "provenance") ?? "{}").mappings === 2, "concept map provenance", map);
}
{
  const { status, body } = await operation("$sql-describe", [
    { name: "subjectResource", resource: sqlLibrary("SELECT patient.nope FROM patient") },
  ]);
  check(status === 200 && (valueOf(partsOf(body), "check") ?? "").includes("nope"), "a missing column is a check", body);
}

console.log("$sql-run");
{
  const { status, headers: h, body } = await operation(
    "$sql-run",
    [
      { name: "subjectResource", resource: sqlLibrary(SQL) },
      { name: "_format", valueCode: "json" },
      { name: "parameters", resource: { resourceType: "Parameters", parameter: [{ name: "g", valueCode: "female" }] } },
    ],
    "application/json"
  );
  const rows = Array.isArray(body) ? (body as Record<string, unknown>[]) : [];
  check(status === 200 && rows.length > 0, "rows for the bound parameter", body);
  check(rows.every((r) => r.gender === "female" && r.v3 === "F"), "translated through the concept map", rows.slice(0, 2));
  const header = JSON.parse(h.get("x-sof-terminology") ?? "[]");
  check(header.length === 2, "x-sof-terminology lists both terminology dependencies", header);
}
{
  const { status, body } = await operation("$sql-run", [
    {
      name: "subjectResource",
      resource: {
        resourceType: "ViewDefinition",
        name: "inline",
        status: "draft",
        resource: "Patient",
        select: [{ column: [{ name: "id", path: "getResourceKey()" }] }],
      },
    },
    { name: "_format", valueCode: "json" },
    { name: "_limit", valueInteger: 2 },
  ], "application/json");
  check(status === 200 && Array.isArray(body) && body.length === 2, "a ViewDefinition subject with _limit", body);
}

console.log("Library storage");
{
  const library = { ...sqlLibrary(SQL), id: "console-check" };
  await put(library);
  const res = await fetch(
    `${BASE_URL}/fhir/Library?type=${encodeURIComponent(`${LIBRARY_TYPES}|sql-query`)}&_count=200`,
    { headers }
  );
  const bundle = (await res.json()) as { entry?: { resource: { id: string } }[] };
  check(res.ok && bundle.entry?.some((e) => e.resource.id === "console-check"), "found by Library.type", bundle);
}

console.log("SQL language server, mode=sof");
{
  const ws = new WebSocket(`${BASE_URL.replace(/^http/, "ws")}/api/lsp/pg?mode=sof&token=${token}`);
  let id = 0;
  const pending = new Map<number, (value: unknown) => void>();
  const diagnostics: unknown[][] = [];
  ws.onmessage = (event) => {
    const message = JSON.parse(String(event.data));
    if (message.id !== undefined && pending.has(message.id)) {
      pending.get(message.id)?.(message.result);
      pending.delete(message.id);
    } else if (message.method === "textDocument/publishDiagnostics") {
      diagnostics.push(message.params.diagnostics);
    }
  };
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });
  const send = (method: string, params: unknown) => ws.send(JSON.stringify({ jsonrpc: "2.0", method, params }));
  const request = (method: string, params: unknown) =>
    new Promise((resolve) => {
      const n = ++id;
      pending.set(n, resolve);
      ws.send(JSON.stringify({ jsonrpc: "2.0", id: n, method, params }));
    });
  const waitDiagnostics = async (count: number) => {
    for (let i = 0; i < 50 && diagnostics.length < count; i++) await Bun.sleep(50);
    return diagnostics[count - 1] as { message: string; severity: number }[];
  };

  await request("initialize", { processId: null, rootUri: null, capabilities: {} });
  send("initialized", {});
  send("workspace/didChangeConfiguration", {
    settings: {
      sof: {
        relations: [
          { name: "patient", kind: "ViewDefinition", columns: [{ name: "id", type: "text" }, { name: "gender", type: "text" }] },
          { name: "genders", kind: "ValueSet", columns: [{ name: "system", type: "varchar" }, { name: "code", type: "varchar" }] },
        ],
        parameters: [{ name: "g", type: "code" }],
      },
    },
  });
  const uri = "inmemory://console/query.sql";
  const text = "SELECT patient.id FROM patient AS patient WHERE patient.gender = :g";
  send("textDocument/didOpen", { textDocument: { uri, languageId: "pgsql", version: 1, text } });
  const clean = await waitDiagnostics(1);
  check(clean && clean.filter((d) => d.severity === 1).length === 0, "a valid query has no errors", clean);

  send("textDocument/didChange", {
    textDocument: { uri, version: 2 },
    contentChanges: [{ text: "SELECT pg_sleep(1) AS pause FROM observation AS o WHERE o.x = :nope" }],
  });
  const bad = await waitDiagnostics(2);
  const messages = (bad ?? []).map((d) => d.message).join(" | ");
  check(/observation/i.test(messages), "an undeclared relation is reported", messages);
  check(messages.includes(":nope"), "an undeclared parameter is reported", messages);

  const tables = "SELECT * FROM ";
  send("textDocument/didChange", { textDocument: { uri, version: 3 }, contentChanges: [{ text: tables }] });
  const completion = (await request("textDocument/completion", {
    textDocument: { uri },
    position: { line: 0, character: tables.length },
  })) as { items: { label: string }[] };
  const labels = completion.items.map((i) => i.label);
  check(labels.includes("patient") && labels.includes("genders"), "relations complete after FROM", labels.slice(0, 20));
  check(!labels.some((l) => l === "_resource_type" || l.startsWith("pg_")), "no database catalog in completion", labels.slice(0, 20));

  const param = "SELECT * FROM patient WHERE patient.gender = :";
  send("textDocument/didChange", { textDocument: { uri, version: 4 }, contentChanges: [{ text: param }] });
  const params = (await request("textDocument/completion", {
    textDocument: { uri },
    position: { line: 0, character: param.length },
  })) as { items: { label: string }[] };
  check(params.items.length === 1 && params.items[0].label === "g", "parameters complete after ':'", params.items);
  ws.close();
}

console.log(failures === 0 ? "\nAll checks passed" : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
