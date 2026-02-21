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
        method: "POST", headers, body: JSON.stringify(resource),
      });
      if (!res.ok) throw new Error(`Create failed: ${res.status} ${await res.text()}`);
      return res.json();
    },
    async search(query: string): Promise<any> {
      const res = await fetch(`${BASE_URL}/fhir/${query}`, { headers });
      const body = await res.json();
      return { status: res.status, ...body };
    },
  };
}

async function main() {
  const token = await getToken();
  const api = fhir(token);

  // Create known test data
  console.log("Creating test data...");
  const pat = await api.create({
    resourceType: "Patient",
    name: [{ family: "DebugFamily", given: ["DebugGiven"] }],
    birthDate: "1985-06-15",
    gender: "female",
    identifier: [{ system: "http://test.org", value: "debug-123" }],
  });
  console.log(`  Patient/${pat.id} family=DebugFamily birthDate=1985-06-15`);

  const obs = await api.create({
    resourceType: "Observation",
    status: "final",
    code: { coding: [{ system: "http://loinc.org", code: "8867-4", display: "Heart rate" }] },
    subject: { reference: `Patient/${pat.id}` },
    effectiveDateTime: "2025-03-10T14:00:00Z",
    valueQuantity: { value: 72, unit: "bpm" },
  });
  console.log(`  Observation/${obs.id} code=8867-4 subject=Patient/${pat.id}\n`);

  // Run all search variants
  const tests: [string, string, (b: any) => boolean][] = [
    // === STRING SEARCH ===
    ["STR  family exact",     `Patient?family=DebugFamily&_count=5`,          b => b.total > 0],
    ["STR  family prefix",    `Patient?family=Debug&_count=5`,                b => b.total > 0],
    ["STR  family contains",  `Patient?family:contains=bugFam&_count=5`,      b => b.total > 0],
    ["STR  family exact (ci)", `Patient?family=debugfamily&_count=5`,         b => b.total > 0],
    ["STR  name (any)",       `Patient?name=DebugGiven&_count=5`,             b => b.total > 0],
    ["STR  given",            `Patient?given=DebugGiven&_count=5`,            b => b.total > 0],

    // === TOKEN SEARCH ===
    ["TOK  gender",           `Patient?gender=female&_count=5`,               b => b.total > 0],
    ["TOK  identifier",       `Patient?identifier=debug-123&_count=5`,        b => b.total > 0],
    ["TOK  identifier sys|val", `Patient?identifier=http://test.org|debug-123&_count=5`, b => b.total > 0],
    ["TOK  obs code",         `Observation?code=8867-4&_count=5`,             b => b.total > 0],
    ["TOK  obs code sys|val", `Observation?code=http://loinc.org|8867-4&_count=5`, b => b.total > 0],

    // === DATE SEARCH ===
    ["DATE birthdate eq",     `Patient?birthdate=1985-06-15&_count=5`,        b => b.total > 0],
    ["DATE birthdate eq year", `Patient?birthdate=1985&_count=5`,             b => b.total > 0],
    ["DATE birthdate gt",     `Patient?birthdate=gt1980-01-01&_count=5`,      b => b.total > 0],
    ["DATE birthdate lt",     `Patient?birthdate=lt1990-01-01&_count=5`,      b => b.total > 0],
    ["DATE birthdate ge",     `Patient?birthdate=ge1985-06-15&_count=5`,      b => b.total > 0],
    ["DATE birthdate le",     `Patient?birthdate=le1985-06-15&_count=5`,      b => b.total > 0],
    ["DATE obs date eq",      `Observation?date=2025-03-10&_count=5`,         b => b.total > 0],
    ["DATE obs date gt",      `Observation?date=gt2025-01-01&_count=5`,       b => b.total > 0],
    ["DATE obs date lt",      `Observation?date=lt2026-01-01&_count=5`,       b => b.total > 0],

    // === REFERENCE SEARCH ===
    ["REF  subject typed",    `Observation?subject=Patient/${pat.id}&_count=5`, b => b.total > 0],
    ["REF  subject untyped",  `Observation?subject=${pat.id}&_count=5`,        b => b.total > 0],
    ["REF  patient param",    `Observation?patient=${pat.id}&_count=5`,         b => b.total > 0],

    // === INCLUDE ===
    ["INC  _include",         `Observation?code=8867-4&_include=Observation:subject&_count=5`,
      b => b.entry?.some((e: any) => e.resource?.resourceType === "Patient")],
    ["INC  _revinclude",      `Patient?family=DebugFamily&_revinclude=Observation:subject&_count=5`,
      b => b.entry?.some((e: any) => e.resource?.resourceType === "Observation")],

    // === CHAINED ===
    ["CHN  obs->patient.family", `Observation?subject:Patient.family=DebugFamily&_count=5`, b => b.total > 0],
    ["CHN  obs->patient.name",   `Observation?subject:Patient.name=DebugGiven&_count=5`,    b => b.total > 0],

    // === SORT ===
    ["SORT _lastUpdated",     `Patient?_sort=-_lastUpdated&_count=5`,         b => b.total > 0],
    ["SORT date",             `Observation?_sort=-date&_count=5`,             b => b.total > 0],

    // === SCAN (baseline) ===
    ["SCAN Patient",          `Patient?_count=5`,                             b => b.total > 0],
    ["SCAN Observation",      `Observation?_count=5`,                         b => b.total > 0],
  ];

  let pass = 0, fail = 0, error = 0;

  for (const [label, query, check] of tests) {
    try {
      const result = await api.search(query);
      const ok = check(result);
      const icon = ok ? "✓" : "✗";
      const total = result.total ?? "?";
      console.log(`  ${ok ? "✓" : "✗"} ${label.padEnd(28)} total=${String(total).padStart(4)}  ${query}`);
      if (ok) pass++; else {
        fail++;
        if (result.issue) {
          for (const i of result.issue) console.log(`      ${i.severity}: ${i.diagnostics || ""}`);
        }
      }
    } catch (e: any) {
      console.log(`  ! ${label.padEnd(28)} ERROR  ${e.message?.substring(0, 100)}`);
      error++;
    }
  }

  console.log(`\n=== Results: ${pass} pass, ${fail} fail, ${error} error (of ${tests.length}) ===`);
}

main().catch(e => { console.error(e); process.exit(1); });
