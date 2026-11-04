/**
 * $ccda-to-fhir and $fhir-to-ccda against a running server.
 *
 * Usage: bun run scripts/test-ccda.ts [path/to/document.xml]
 * Defaults to the CCD in ../ccda-fhir/tests/data.
 */

const BASE_URL = process.env.OCTOFHIR_BASE_URL ?? "http://localhost:8888";
const DOCUMENT = process.argv[2] ?? "../ccda-fhir/tests/data/ccd-1.xml";

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
if (!tokenRes.ok) throw new Error(`token: ${tokenRes.status} ${await tokenRes.text()}`);
const { access_token } = await tokenRes.json();
const headers = {
  Authorization: `Bearer ${access_token}`,
  "Content-Type": "application/fhir+json",
  Accept: "application/fhir+json",
};

let failures = 0;
function check(name: string, ok: boolean, detail = "") {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${detail ? ` — ${detail}` : ""}`);
  if (!ok) failures++;
}

async function post(path: string, body: unknown) {
  const res = await fetch(`${BASE_URL}/fhir/${path}`, { method: "POST", headers, body: JSON.stringify(body) });
  return { status: res.status, json: await res.json() };
}

const xml = await Bun.file(DOCUMENT).text();
const binary = { resourceType: "Binary", contentType: "application/xml", data: Buffer.from(xml).toString("base64") };
const param = (json: any, name: string) => json.parameter?.find((p: any) => p.name === name)?.resource;

// CDA → FHIR, the document in Parameters.
const toFhir = await post("$ccda-to-fhir", {
  resourceType: "Parameters",
  parameter: [{ name: "document", resource: binary }],
});
check("$ccda-to-fhir answers 200", toFhir.status === 200, String(toFhir.status));
const bundle = param(toFhir.json, "return");
const outcome = param(toFhir.json, "outcome");
check("a document Bundle", bundle?.resourceType === "Bundle" && bundle?.type === "document");
check("the Composition first", bundle?.entry?.[0]?.resource?.resourceType === "Composition");
const types = new Set<string>(bundle?.entry?.map((e: any) => e.resource.resourceType) ?? []);
for (const type of ["Patient", "AllergyIntolerance", "Condition", "MedicationRequest", "Immunization", "DiagnosticReport", "Procedure"]) {
  check(`has ${type}`, types.has(type));
}
check("losses as an OperationOutcome", outcome?.resourceType === "OperationOutcome", `${outcome?.issue?.length ?? 0} issue(s)`);

// The Binary as the body.
const direct = await post("$ccda-to-fhir", binary);
check("a Binary body works too", direct.status === 200 && param(direct.json, "return")?.type === "document");

// FHIR → CDA.
const toCda = await post("$fhir-to-ccda", bundle);
check("$fhir-to-ccda answers 200", toCda.status === 200, String(toCda.status));
const cda = param(toCda.json, "return");
check("a Binary with XML", cda?.resourceType === "Binary" && cda?.contentType === "application/xml");
const back = Buffer.from(cda?.data ?? "", "base64").toString("utf8");
check("a ClinicalDocument in the US Realm", back.includes("<ClinicalDocument") && back.includes('root="2.16.840.1.113883.10.20.22.1.1"'));

// A Composition alone.
const composition = await post("$fhir-to-ccda", {
  resourceType: "Parameters",
  parameter: [{ name: "resource", resource: bundle.entry[0].resource }],
});
check("a Composition alone", composition.status === 200);

// Bad input: 400 with an OperationOutcome.
const bad = await post("$ccda-to-fhir", { resourceType: "Patient" });
check("not a Binary: 400", bad.status === 400 && bad.json.resourceType === "OperationOutcome", String(bad.status));
const notCda = await post("$ccda-to-fhir", {
  resourceType: "Binary",
  contentType: "application/xml",
  data: Buffer.from("<observation xmlns='urn:hl7-org:v3'/>").toString("base64"),
});
check("not a document: 400", notCda.status === 400, String(notCda.status));

// Advertised.
const metadata = await (await fetch(`${BASE_URL}/fhir/metadata`, { headers })).json();
const operations = JSON.stringify(metadata);
check("advertised in the CapabilityStatement", operations.includes("ccda-to-fhir") && operations.includes("fhir-to-ccda"));

// The FML console: a map run over the CDA document, with its trace and XML positions.
const demoMap = `map "http://example.org/StructureMap/cda-demo" = "CdaDemo"
uses "http://hl7.org/cda/stds/core/StructureDefinition/ClinicalDocument" alias ClinicalDocument as source
uses "http://hl7.org/fhir/StructureDefinition/Bundle" alias Bundle as target
group Demo(source cda : ClinicalDocument, target bundle : Bundle) {
  cda.id as id -> bundle.identifier as identifier then {
    id.root as root -> identifier.value = root;
  };
}`;
const consoleRes = await fetch(`${BASE_URL}/api/fml/cda-run`, {
  method: "POST",
  headers: { ...headers, "Content-Type": "application/json", Accept: "application/json" },
  body: JSON.stringify({ source: demoMap, xml, debug: true }),
});
const run = (await consoleRes.json()) as any;
check("console: a map over the CDA runs", consoleRes.status === 200 && !run.error, run.error ?? String(consoleRes.status));
check("console: the result", typeof run.result?.identifier?.value === "string", JSON.stringify(run.result?.identifier));
check("console: the trace", run.debug?.resourceType === "Parameters");
check(
  "console: XML positions of the input",
  run.positions?.["ClinicalDocument.id"]?.line > 0,
  `${Object.keys(run.positions ?? {}).length} path(s)`,
);

console.log(failures === 0 ? "\nall checks passed" : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
