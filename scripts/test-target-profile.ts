// End-to-end check for targetProfile conformance validation.
//
// Prerequisites:
//   1. US-Core IG loaded (provides us-core-patient + us-core-average-blood-pressure).
//   2. Server started with targetProfile conformance ON:
//        OCTOFHIR__VALIDATION__CHECK_TARGET_PROFILE=true \
//        OCTOFHIR__VALIDATION__SKIP_REFERENCE_VALIDATION=false \
//        cargo run
//
// What it does: creates a plain Patient that does NOT conform to us-core-patient,
// then posts a us-core-average-blood-pressure Observation whose `subject` points
// at that Patient. Its Observation.subject declares targetProfile=us-core-patient,
// so validation should flag a conformance error (FS1017) on subject.reference.
//
// Run: bun run scripts/test-target-profile.ts

const BASE_URL = process.env.BASE_URL ?? "http://localhost:8888";
const US_CORE_PATIENT =
  "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient";
const US_CORE_AVG_BP =
  "http://hl7.org/fhir/us/core/StructureDefinition/us-core-average-blood-pressure";

async function token(): Promise<string> {
  const res = await fetch(`${BASE_URL}/oauth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: "grant_type=password&username=admin&password=admin123",
  });
  if (!res.ok) throw new Error(`token failed: ${res.status} ${await res.text()}`);
  return (await res.json()).access_token;
}

async function post(auth: string, type: string, body: unknown) {
  const res = await fetch(`${BASE_URL}/fhir/${type}`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${auth}`,
      "Content-Type": "application/fhir+json",
    },
    body: JSON.stringify(body),
  });
  const json = await res.json().catch(() => ({}));
  return { status: res.status, json };
}

/** Collect OperationOutcome issue diagnostics that mention targetProfile / FS1017. */
function targetProfileIssues(outcome: any): string[] {
  const issues = outcome?.issue ?? [];
  return issues
    .map((i: any) => i.diagnostics ?? i.details?.text ?? "")
    .filter(
      (d: string) =>
        d.includes("FS1017") ||
        /targetProfile|does not conform/i.test(d),
    );
}

const auth = await token();

// A plain Patient that does NOT satisfy us-core-patient (no identifier/name/gender).
const patient = await post(auth, "Patient", {
  resourceType: "Patient",
  active: true,
});
console.log(`Created non-conforming Patient -> ${patient.status}`);
const patientId = patient.json?.id;
if (!patientId) {
  console.error("Could not create Patient:", JSON.stringify(patient.json, null, 2));
  process.exit(1);
}

// A us-core-average-blood-pressure Observation referencing that Patient.
const bp = {
  resourceType: "Observation",
  meta: { profile: [US_CORE_AVG_BP] },
  status: "final",
  category: [
    {
      coding: [
        {
          system: "http://terminology.hl7.org/CodeSystem/observation-category",
          code: "vital-signs",
        },
      ],
    },
  ],
  code: {
    coding: [{ system: "http://loinc.org", code: "96607-7" }],
  },
  subject: { reference: `Patient/${patientId}` },
  effectiveDateTime: "2026-07-01",
  component: [
    {
      code: { coding: [{ system: "http://loinc.org", code: "96608-5" }] },
      valueQuantity: { value: 120, unit: "mmHg", system: "http://unitsofmeasure.org", code: "mm[Hg]" },
    },
    {
      code: { coding: [{ system: "http://loinc.org", code: "96609-3" }] },
      valueQuantity: { value: 80, unit: "mmHg", system: "http://unitsofmeasure.org", code: "mm[Hg]" },
    },
  ],
};

const obs = await post(auth, "Observation", bp);
console.log(`Posted avg-BP Observation -> ${obs.status}`);

const tpIssues = targetProfileIssues(obs.json);
if (tpIssues.length > 0) {
  console.log("\n✅ targetProfile conformance ENFORCED. subject failed us-core-patient:");
  for (const d of tpIssues) console.log("   -", d);
  console.log(`\n(expected: subject must conform to ${US_CORE_PATIENT})`);
} else {
  console.log("\n⚠️  No targetProfile issue found. Either:");
  console.log("   - check_target_profile is OFF, or");
  console.log("   - US-Core (us-core-patient) is not loaded, or");
  console.log("   - the Patient unexpectedly conforms.");
  console.log("\nFull OperationOutcome:");
  console.log(JSON.stringify(obs.json, null, 2));
}
