// End-to-end checks of StructureMap/$transform: FML text, a stored map, imports,
// translate() with a stored ConceptMap, errors with positions, and the debug trace.
// Run against a local server: bun run scripts/test-fml-transform.ts
const BASE = process.env.BASE_URL ?? "http://localhost:8888";

async function token(): Promise<string> {
  const res = await fetch(`${BASE}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "password",
      client_id: "octofhir-ui",
      username: "admin",
      password: "admin123",
      scope: "openid user/*.cruds system/*.cruds",
    }),
  });
  if (!res.ok) throw new Error(`token: ${res.status} ${await res.text()}`);
  return (await res.json()).access_token;
}

const auth = await token();
const headers = {
  Authorization: `Bearer ${auth}`,
  "Content-Type": "application/fhir+json",
  Accept: "application/fhir+json",
};

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown) {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
  if (!ok) {
    failures++;
    if (detail !== undefined) console.log(JSON.stringify(detail, null, 2));
  }
}

async function post(path: string, body: unknown) {
  const res = await fetch(`${BASE}/fhir${path}`, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  const text = await res.text();
  return { status: res.status, body: text ? JSON.parse(text) : null };
}

async function put(resource: { resourceType: string; id: string }, skipValidation = false) {
  const res = await fetch(`${BASE}/fhir/${resource.resourceType}/${resource.id}`, {
    method: "PUT",
    headers: skipValidation ? { ...headers, "X-Skip-Validation": "true" } : headers,
    body: JSON.stringify(resource),
  });
  if (!res.ok) throw new Error(`PUT ${resource.resourceType}: ${res.status} ${await res.text()}`);
}

const patient = {
  resourceType: "Patient",
  id: "p1",
  gender: "female",
  name: [{ family: "Doe", given: ["Jane"] }],
};

const mainFml = `/// url = 'http://example.org/StructureMap/test-main'
/// name = 'TestMain'
uses "http://hl7.org/fhir/StructureDefinition/Patient" alias Patient as source
uses "http://hl7.org/fhir/StructureDefinition/Basic" alias Basic as target
imports "http://example.org/StructureMap/test-lib*"

group Main(source src : Patient, target tgt : Basic) {
  src.id as v -> tgt.id = v "id";
  src.gender as g -> tgt.code = translate(g, 'http://example.org/ConceptMap/test-gender', 'CodeableConcept') "code";
  src.name as n -> tgt.extension as e, e.url = 'http://example.org/name' then Name(n, e) "name";
}
`;

const libFml = `/// url = 'http://example.org/StructureMap/test-lib-names'
/// name = 'TestLibNames'
group Name(source n, target e) {
  n.family as f -> e.value = f "family";
}
`;

// 1. FML text with an inline library and a stored ConceptMap.
await put({
  resourceType: "ConceptMap",
  id: "test-gender",
  url: "http://example.org/ConceptMap/test-gender",
  status: "active",
  group: [
    {
      source: "http://hl7.org/fhir/administrative-gender",
      target: "http://example.org/sex",
      element: [{ code: "female", target: [{ code: "F", equivalence: "equivalent" }] }],
    },
  ],
} as never);

let r = await post("/StructureMap/$transform", {
  resourceType: "Parameters",
  parameter: [
    { name: "srcMap", valueString: mainFml },
    { name: "srcMap", valueString: libFml },
    { name: "content", resource: patient },
  ],
});
check("srcMap transform returns the target", r.status === 200 && r.body?.resourceType === "Basic", r.body);
check("translate() used the stored ConceptMap", r.body?.code?.coding?.[0]?.code === "F", r.body);
check("the imported group ran", r.body?.extension?.[0]?.valueString === "Doe", r.body);

// 2. A stored map, called on the instance, importing a stored library.
await put({ resourceType: "StructureMap", id: "test-lib-names", ...(await compiled(libFml)) } as never);
await put({ resourceType: "StructureMap", id: "test-main", ...(await compiled(mainFml)) } as never);
r = await post("/StructureMap/test-main/$transform", {
  resourceType: "Parameters",
  parameter: [{ name: "content", resource: patient }],
});
check("instance transform with stored imports", r.status === 200 && r.body?.extension?.[0]?.valueString === "Doe", r.body);

// 3. By canonical.
r = await post("/StructureMap/$transform", {
  resourceType: "Parameters",
  parameter: [
    { name: "source", valueUri: "http://example.org/StructureMap/test-main" },
    { name: "content", resource: patient },
  ],
});
check("transform by canonical", r.status === 200 && r.body?.id === "p1", r.body);

// 4. FML that does not compile: 422 with the line.
r = await post("/StructureMap/$transform", {
  resourceType: "Parameters",
  parameter: [
    { name: "srcMap", valueString: "group G(source src, target tgt) {\n  src -> tgt.a = evaluate(src, );\n}\n" },
    { name: "content", resource: patient },
  ],
});
const line = r.body?.issue?.[0]?.extension?.find((e: { url: string }) => e.url.endsWith("issue-line"));
check("compile error is a 422 with the line", r.status === 422 && line?.valueInteger === 2, r.body);

// 5. A map that fails at run time: 422 with the rule position.
const failing = mainFml.replace("tgt.id = v", "tgt.nope = v");
r = await post("/StructureMap/$transform", {
  resourceType: "Parameters",
  parameter: [
    { name: "srcMap", valueString: failing },
    { name: "srcMap", valueString: libFml },
    { name: "content", resource: patient },
  ],
});
check(
  "runtime failure is a 422 at the rule",
  r.status === 422 && r.body?.issue?.[0]?.expression?.[0] === "StructureMap.group[0].rule[0].target[0]",
  r.body,
);

// 6. debug=true: the trace Parameters with text positions.
r = await post("/StructureMap/$transform?debug=true", {
  resourceType: "Parameters",
  parameter: [
    { name: "srcMap", valueString: mainFml },
    { name: "srcMap", valueString: libFml },
    { name: "content", resource: patient },
  ],
});
const parameters = r.body?.parameter ?? [];
const status = parameters.find((p: { name: string }) => p.name === "traceStatus")?.valueCode;
const events = parameters.filter((p: { name: string }) => p.name === "trace");
const located = events.find((e: { part: { name: string }[] }) => e.part.some((p) => p.name === "startRow"));
check("debug trace is complete", r.status === 200 && status === "complete" && events.length > 5, r.body);
check("trace events carry text positions", located !== undefined, events[0]);

// 7. A Binary content (JSON of a logical model style input) and a Binary result for a logical target.
const logicalFml = `/// url = 'http://example.org/StructureMap/test-logical'
/// name = 'TestLogical'
uses "http://hl7.org/fhir/StructureDefinition/Patient" alias Patient as source
uses "http://example.org/StructureDefinition/Flat" alias Flat as target

group Main(source src : Patient, target tgt : Flat) {
  src.name as n -> tgt.family = (n.family) "family";
}
`;
await put({
  resourceType: "StructureDefinition",
  id: "test-flat",
  url: "http://example.org/StructureDefinition/Flat",
  name: "Flat",
  status: "draft",
  kind: "logical",
  abstract: false,
  type: "http://example.org/StructureDefinition/Flat",
  baseDefinition: "http://hl7.org/fhir/StructureDefinition/Base",
  derivation: "specialization",
  differential: {
    element: [
      { id: "Flat", path: "Flat", min: 0, max: "*" },
      { id: "Flat.family", path: "Flat.family", min: 0, max: "1", type: [{ code: "string" }] },
    ],
  },
  // Validation of R4 logical models stops on sdf-8a in the FHIRPath engine (startsWith with a
  // uri argument); not what this test is about.
} as never);
r = await post("/StructureMap/$transform", {
  resourceType: "Parameters",
  parameter: [
    { name: "srcMap", valueString: logicalFml },
    {
      name: "content",
      resource: { resourceType: "Binary", contentType: "application/fhir+json", data: btoa(JSON.stringify(patient)) },
    },
  ],
});
const flat = r.body?.data ? JSON.parse(atob(r.body.data)) : null;
check(
  "a logical target comes back as a Binary",
  r.status === 200 && r.body?.resourceType === "Binary" && flat?.family === "Doe",
  r.body,
);

// 8. debug=true needs the configured role: a user without it is refused.
const adminHeaders = { Authorization: `Bearer ${auth}`, "Content-Type": "application/json" };
const username = `fml-test-${crypto.randomUUID().slice(0, 8)}`;
const created = await fetch(`${BASE}/admin/User`, {
  method: "POST",
  headers: adminHeaders,
  body: JSON.stringify({ resourceType: "User", username, roles: ["practitioner"], active: true }),
});
const userId = (await created.json()).id;
await fetch(`${BASE}/admin/User/${userId}/$reset-password`, {
  method: "POST",
  headers: adminHeaders,
  body: JSON.stringify({ newPassword: "fml-test-password" }),
});
const userToken = await fetch(`${BASE}/auth/token`, {
  method: "POST",
  headers: { "Content-Type": "application/x-www-form-urlencoded" },
  body: new URLSearchParams({
    grant_type: "password",
    client_id: "octofhir-ui",
    username,
    password: "fml-test-password",
    scope: "openid user/*.cruds",
  }),
});
if (userToken.ok) {
  const res = await fetch(`${BASE}/fhir/StructureMap/test-main/$transform?debug=true`, {
    method: "POST",
    headers: { ...headers, Authorization: `Bearer ${(await userToken.json()).access_token}` },
    body: JSON.stringify({ resourceType: "Parameters", parameter: [{ name: "content", resource: patient }] }),
  });
  check("debug without the role is forbidden", res.status === 403, await res.text());
} else {
  check("a user without the admin role gets a token", false, await userToken.text());
}
await fetch(`${BASE}/admin/User/${userId}`, { method: "DELETE", headers: adminHeaders });

// 9. QuestionnaireResponse/$extract with the Questionnaire's target StructureMap.
const extractFml = `/// url = 'http://example.org/StructureMap/test-extract'
/// name = 'TestExtract'
uses "http://hl7.org/fhir/StructureDefinition/QuestionnaireResponse" alias QR as source
uses "http://hl7.org/fhir/StructureDefinition/Patient" alias Patient as target

group Extract(source src : QR, target tgt : Patient) {
  src.item as item where (linkId = 'family') then {
    item.answer as answer then {
      answer.value as v -> tgt.name as name, name.family = v "family";
    } "answer";
  } "item";
}
`;
await put({ resourceType: "StructureMap", id: "test-extract", ...(await compiled(extractFml)) } as never);
await put({
  resourceType: "Questionnaire",
  id: "test-extract",
  url: "http://example.org/Questionnaire/test-extract",
  status: "active",
  extension: [
    {
      url: "http://hl7.org/fhir/uv/sdc/StructureDefinition/sdc-questionnaire-targetStructureMap",
      valueCanonical: "http://example.org/StructureMap/test-extract",
    },
  ],
  item: [{ linkId: "family", text: "Family name", type: "string" }],
} as never);
const response = {
  resourceType: "QuestionnaireResponse",
  id: "test-extract",
  questionnaire: "http://example.org/Questionnaire/test-extract",
  status: "completed",
  item: [{ linkId: "family", answer: [{ valueString: "Smith" }] }],
};
r = await post("/QuestionnaireResponse/$extract", {
  resourceType: "Parameters",
  parameter: [{ name: "questionnaire-response", resource: response }],
});
check("$extract runs the target map", r.status === 200 && r.body?.name?.[0]?.family === "Smith", r.body);
await put(response as never);
r = await post("/QuestionnaireResponse/test-extract/$extract", { resourceType: "Parameters", parameter: [] });
check("$extract on a stored response", r.status === 200 && r.body?.name?.[0]?.family === "Smith", r.body);

// 10. $convert R4 -> R5 with the HL7 cross-version maps (when the server has them, for example
// through fml.map_folders pointing at a fhir-cross-version checkout).
{
  const medicationRequest = {
    resourceType: "MedicationRequest",
    id: "convert-test",
    status: "active",
    intent: "order",
    medicationCodeableConcept: { text: "aspirin" },
    subject: { reference: "Patient/p1" },
  };
  const res = await fetch(`${BASE}/fhir/$convert`, {
    method: "POST",
    headers: {
      ...headers,
      "Content-Type": "application/fhir+json; fhirVersion=4.0",
      Accept: "application/fhir+json; fhirVersion=5.0",
    },
    body: JSON.stringify({ resourceType: "Parameters", parameter: [{ name: "input", resource: medicationRequest }] }),
  });
  const text = await res.text();
  if (res.status === 422 && text.includes("No map converts")) {
    console.log("  (no cross-version maps on the server; skipped $convert)");
  } else {
    const converted = JSON.parse(text);
    check(
      "$convert R4 to R5 answers in the R5 media type",
      res.status === 200 && (res.headers.get("content-type") ?? "").includes("fhirVersion=5.0"),
      text.slice(0, 500),
    );
    check(
      "$convert turns medication[x] into a CodeableReference",
      converted?.medication?.concept?.text === "aspirin" && converted?.medicationCodeableConcept === undefined,
      converted,
    );
  }
}

// 11. Validating a StructureMap reports what fml-check finds, as warnings at the rule.
{
  const typo = mainFml.replace("src.gender as g", "src.gendr as g");
  const res = await post("/StructureMap/$validate", { ...(await compiled(typo)), resourceType: "StructureMap" });
  const issue = (res.body?.issue ?? []).find((i: { diagnostics?: string }) => i.diagnostics?.includes("gendr"));
  check(
    "$validate reports fml-check findings at the rule",
    issue?.severity === "warning" && (issue?.expression?.[0] ?? issue?.location?.[0]) === "StructureMap.group[0].rule[1].source[0]",
    res.body,
  );
}

// 12. `model`: a logical model and a ConceptMap for this run only, nothing stored.
{
  const modelFml = `/// url = 'http://example.org/StructureMap/test-model'
/// name = 'TestModel'
uses "http://hl7.org/fhir/StructureDefinition/Patient" alias Patient as source
uses "http://example.org/StructureDefinition/OnlyHere" alias OnlyHere as target

group Main(source src : Patient, target tgt : OnlyHere) {
  src.gender as g -> tgt.sex = translate(g, 'http://example.org/ConceptMap/only-here', 'code') "sex";
}
`;
  const logical = {
    resourceType: "StructureDefinition",
    url: "http://example.org/StructureDefinition/OnlyHere",
    name: "OnlyHere",
    status: "draft",
    kind: "logical",
    abstract: false,
    type: "http://example.org/StructureDefinition/OnlyHere",
    baseDefinition: "http://hl7.org/fhir/StructureDefinition/Base",
    derivation: "specialization",
    differential: {
      element: [
        { id: "OnlyHere", path: "OnlyHere", min: 0, max: "*" },
        { id: "OnlyHere.sex", path: "OnlyHere.sex", min: 0, max: "1", type: [{ code: "code" }] },
      ],
    },
  };
  const conceptMap = {
    resourceType: "ConceptMap",
    url: "http://example.org/ConceptMap/only-here",
    status: "active",
    group: [
      {
        source: "http://hl7.org/fhir/administrative-gender",
        target: "http://example.org/sex",
        element: [{ code: "female", target: [{ code: "F", relationship: "equivalent" }] }],
      },
    ],
  };
  r = await post("/StructureMap/$transform", {
    resourceType: "Parameters",
    parameter: [
      { name: "srcMap", valueString: modelFml },
      { name: "content", resource: patient },
      { name: "model", resource: { resourceType: "Bundle", type: "collection", entry: [{ resource: logical }] } },
      { name: "model", valueString: JSON.stringify(conceptMap) },
    ],
  });
  const onlyHere = r.body?.data ? JSON.parse(atob(r.body.data)) : null;
  check("`model` brings a logical model and a ConceptMap", r.status === 200 && onlyHere?.sex === "F", r.body);
}

// 13. A stored map prints back as FML, and trace variables from the input carry their path.
{
  const stored = await fetch(`${BASE}/fhir/StructureMap/test-main`, { headers }).then((res) => res.json());
  const printed = await fetch(`${BASE}/api/fml/print`, {
    method: "POST",
    headers: { ...headers, "Content-Type": "application/json" },
    body: JSON.stringify(stored),
  }).then((res) => res.json());
  check("/api/fml/print gives the map as FML", String(printed?.source ?? "").includes("group Main("), printed);

  const traced = await post("/StructureMap/$transform?debug=true", {
    resourceType: "Parameters",
    parameter: [
      { name: "srcMap", valueString: mainFml },
      { name: "srcMap", valueString: libFml },
      { name: "content", resource: patient },
    ],
  });
  const paths = JSON.stringify(traced.body).match(/"name":"path","valueString":"([^"]+)"/g) ?? [];
  check(
    "trace variables carry their input path",
    paths.some((part) => part.includes("Patient.name[0]")),
    paths.slice(0, 5),
  );
}

// The StructureMap JSON of FML text, from the fml CLI (FML_BIN, default `fml`), in the form of
// the server's release (FHIR_RELEASE, default r4).
async function compiled(text: string): Promise<Record<string, unknown>> {
  const path = `${import.meta.dir}/../target/fml-test-${crypto.randomUUID()}.fml`;
  await Bun.write(path, text);
  const proc = Bun.spawn([process.env.FML_BIN ?? "fml", "compile", "--fhir", process.env.FHIR_RELEASE ?? "r4", path], { stdout: "pipe", stderr: "pipe" });
  const out = await new Response(proc.stdout).text();
  const code = await proc.exited;
  await Bun.file(path).delete();
  if (code !== 0) throw new Error(`fml compile failed: ${await new Response(proc.stderr).text()}`);
  const map = JSON.parse(out);
  delete map.id;
  return map;
}

console.log(failures === 0 ? "\nall passed" : `\n${failures} failed`);
process.exit(failures === 0 ? 0 : 1);
