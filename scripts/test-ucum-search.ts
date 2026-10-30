/**
 * UCUM canonical quantity search: a quantity search with the UCUM system matches
 * stored quantities in any commensurable unit (1 g finds 1000 mg).
 *
 * Usage: bun run scripts/test-ucum-search.ts
 */

const BASE_URL = process.env.OCTOFHIR_BASE_URL ?? "http://localhost:8888";
const UCUM = "http://unitsofmeasure.org";
const RUN = Date.now().toString(36);
const SUBJECT = `Patient/ucum-test-${RUN}`;

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
if (!tokenRes.ok) {
  console.error(`token request failed: ${tokenRes.status} ${await tokenRes.text()}`);
  process.exit(1);
}
const { access_token } = (await tokenRes.json()) as { access_token: string };
const headers = {
  "Content-Type": "application/fhir+json",
  Authorization: `Bearer ${access_token}`,
};

type Qty = { value: number; code: string; system?: string };

function observation(id: string, value: Qty, components: Qty[] = []) {
  const qty = (q: Qty) => ({ value: q.value, unit: q.code, system: q.system ?? UCUM, code: q.code });
  return {
    resourceType: "Observation",
    id: `${id}-${RUN}`,
    status: "final",
    code: { coding: [{ system: "http://loinc.org", code: "29463-7" }] },
    subject: { reference: SUBJECT },
    valueQuantity: qty(value),
    ...(components.length > 0 && {
      component: components.map((c) => ({
        code: { coding: [{ system: "http://loinc.org", code: "8480-6" }] },
        valueQuantity: qty(c),
      })),
    }),
  };
}

const fixtures = [
  observation("mg1000", { value: 1000, code: "mg" }),
  observation("g1", { value: 1, code: "g" }),
  observation("g05", { value: 0.5, code: "g" }),
  observation("kg1", { value: 1, code: "kg" }),
  observation("lb2", { value: 2, code: "[lb_av]" }),
  observation("cel37", { value: 37, code: "Cel" }),
  observation("local1", { value: 1, code: "g", system: "http://example.org/units" }),
  observation("bp", { value: 1, code: "1" }, [{ value: 120, code: "mm[Hg]" }]),
];

const subject = { resourceType: "Patient", id: SUBJECT.split("/")[1] };
for (const resource of [subject, ...fixtures]) {
  const res = await fetch(`${BASE_URL}/fhir/${resource.resourceType}/${resource.id}`, {
    method: "PUT",
    headers,
    body: JSON.stringify(resource),
  });
  if (!res.ok) {
    console.error(`PUT ${resource.id} failed: ${res.status} ${await res.text()}`);
    process.exit(1);
  }
}

async function search(query: string): Promise<string[]> {
  const res = await fetch(
    `${BASE_URL}/fhir/Observation?subject=${encodeURIComponent(SUBJECT)}&${query}&_count=100`,
    { headers },
  );
  if (!res.ok) throw new Error(`search ${query} failed: ${res.status} ${await res.text()}`);
  const bundle = (await res.json()) as { entry?: { resource: { id: string } }[] };
  return (bundle.entry ?? []).map((e) => e.resource.id.replace(`-${RUN}`, "")).sort();
}

const q = (prefixValue: string, code: string, param = "value-quantity") =>
  `${param}=${encodeURIComponent(`${prefixValue}|${UCUM}|${code}`)}`;

// eq uses the implicit precision range: 1 is [0.5, 1.5), 1.0 is [0.95, 1.05).
const cases: [string, string, string[]][] = [
  ["1.0 g matches 1000 mg and 1 g", q("1.0", "g"), ["g1", "mg1000"]],
  ["1000 mg matches 1 g and 1000 mg", q("1000", "mg"), ["g1", "mg1000"]],
  ["gt900 mg", q("gt900", "mg"), ["g1", "kg1", "lb2", "mg1000"]],
  ["lt600 mg", q("lt600", "mg"), ["g05"]],
  ["0.0010 t (tonne) matches 1 kg", q("0.0010", "t"), ["kg1"]],
  ["ge0.95 kg", q("ge0.95", "kg"), ["kg1"]],
  ["907 g matches 2 lb", q("907", "g"), ["lb2"]],
  ["310.15 K matches 37 Cel", q("310.15", "K"), ["cel37"]],
  ["98.6 degF matches 37 Cel", q("98.6", "[degF]"), ["cel37"]],
  ["ne1.0 g excludes the gram quantities", q("ne1.0", "g"), ["g05", "kg1", "lb2"]],
  ["non-UCUM system stays an exact match", `value-quantity=${encodeURIComponent("1|http://example.org/units|g")}`, ["local1"]],
  ["no system stays code-only", `value-quantity=${encodeURIComponent("1.0||g")}`, ["g1", "local1"]],
  ["component 16 kPa matches 120 mm[Hg]", q("16", "kPa", "component-value-quantity"), ["bp"]],
];

let failed = 0;
for (const [name, query, expected] of cases) {
  const got = await search(query);
  const ok = JSON.stringify(got) === JSON.stringify([...expected].sort());
  if (!ok) failed++;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : `\n      expected ${expected.sort().join(",")}\n      got      ${got.join(",")}`}`);
}
console.log(`\n${cases.length - failed}/${cases.length} passed`);
process.exit(failed ? 1 : 0);
