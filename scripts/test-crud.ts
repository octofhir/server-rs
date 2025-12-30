const BASE_URL = process.env.BASE_URL || "http://localhost:8888";
const AUTH_USER = process.env.AUTH_USER || "admin";
const AUTH_PASSWORD = process.env.AUTH_PASSWORD || "admin123";
const CLIENT_ID = process.env.CLIENT_ID || "k6-test";
const CLIENT_SECRET = process.env.CLIENT_SECRET || Bun.file(".k6-secret").text();

async function getToken(): Promise<string> {
  const secret = typeof CLIENT_SECRET === "string" ? CLIENT_SECRET : await CLIENT_SECRET;
  const body = new URLSearchParams({
    grant_type: "password",
    username: AUTH_USER,
    password: AUTH_PASSWORD,
    client_id: CLIENT_ID,
    client_secret: secret.trim(),
  });

  const res = await fetch(`${BASE_URL}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });

  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Token request failed: ${res.status} ${text}`);
  }

  const data = await res.json();
  return data.access_token;
}

async function createResource(token: string, resourceType: string, resource: object) {
  const res = await fetch(`${BASE_URL}/fhir/${resourceType}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/fhir+json",
      Authorization: `Bearer ${token}`,
    },
    body: JSON.stringify(resource),
  });

  const text = await res.text();
  console.log(`POST /${resourceType}: ${res.status}`);

  if (!res.ok) {
    console.error(`  Error ${res.status}:`);
    try {
      const parsed = JSON.parse(text);
      console.error(JSON.stringify(parsed, null, 2));
    } catch {
      console.error(text);
    }
    return null;
  }

  try {
    return JSON.parse(text);
  } catch {
    console.error(`  Invalid JSON: ${text}`);
    return null;
  }
}

async function deleteResource(token: string, resourceType: string, id: string) {
  const res = await fetch(`${BASE_URL}/fhir/${resourceType}/${id}`, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${token}` },
  });
  const status = res.status;
  if (status !== 200 && status !== 204) {
    const text = await res.text();
    console.log(`  DELETE /${resourceType}/${id}: ${status}`);
    try {
      const parsed = JSON.parse(text);
      console.error(`    Error: ${JSON.stringify(parsed.issue?.[0]?.diagnostics || parsed)}`);
    } catch {
      console.error(`    Error: ${text}`);
    }
    return false;
  }
  console.log(`  DELETE /${resourceType}/${id}: ${status} ✓`);
  return true;
}

async function main() {
  console.log("Getting token...");
  const token = await getToken();
  console.log("Token obtained ✓\n");

  // === CREATE ALL RESOURCES ===
  console.log("=== Creating resources ===");

  const patientResult = await createResource(token, "Patient", {
    resourceType: "Patient",
    name: [{ family: "Test", given: ["Patient"] }],
    gender: "unknown",
  });

  const practitionerResult = await createResource(token, "Practitioner", {
    resourceType: "Practitioner",
    name: [{ family: "Doctor", given: ["Test"] }],
    gender: "unknown",
  });

  const organizationResult = await createResource(token, "Organization", {
    resourceType: "Organization",
    name: "Test Organization",
    active: true,
  });

  const locationResult = await createResource(token, "Location", {
    resourceType: "Location",
    name: "Test Location",
    status: "active",
    managingOrganization: {
      display: "Test Org",
      identifier: { value: "test-org-id", system: "test" },
    },
  });

  const encounterResult = await createResource(token, "Encounter", {
    resourceType: "Encounter",
    status: "planned",
    class: { system: "http://terminology.hl7.org/CodeSystem/v3-ActCode", code: "AMB" },
    subject: patientResult ? { reference: `Patient/${patientResult.id}` } : undefined,
    location: locationResult
      ? [{ location: { reference: `Location/${locationResult.id}` } }]
      : undefined,
    participant: practitionerResult
      ? [{ individual: { reference: `Practitioner/${practitionerResult.id}` } }]
      : undefined,
    serviceProvider: organizationResult
      ? { reference: `Organization/${organizationResult.id}` }
      : undefined,
  });

  const observationResult = await createResource(token, "Observation", {
    resourceType: "Observation",
    status: "final",
    code: {
      coding: [{ system: "http://loinc.org", code: "8867-4", display: "Heart rate" }],
    },
    subject: patientResult ? { reference: `Patient/${patientResult.id}` } : undefined,
    encounter: encounterResult ? { reference: `Encounter/${encounterResult.id}` } : undefined,
    valueQuantity: { value: 72, unit: "beats/minute", system: "http://unitsofmeasure.org", code: "/min" },
    issued: new Date().toISOString(),
  });

  const medicationRequestResult = await createResource(token, "MedicationRequest", {
    resourceType: "MedicationRequest",
    status: "active",
    intent: "order",
    medicationCodeableConcept: {
      coding: [{ system: "http://www.nlm.nih.gov/research/umls/rxnorm", code: "1049630", display: "Test Med" }],
      text: "Test Med",
    },
    subject: patientResult ? { reference: `Patient/${patientResult.id}` } : undefined,
    encounter: encounterResult ? { reference: `Encounter/${encounterResult.id}` } : undefined,
    requester: practitionerResult ? { reference: `Practitioner/${practitionerResult.id}` } : undefined,
    authoredOn: new Date().toISOString(),
  });

  const claimResult = await createResource(token, "Claim", {
    resourceType: "Claim",
    status: "active",
    type: { coding: [{ system: "http://terminology.hl7.org/CodeSystem/claim-type", code: "institutional" }] },
    use: "claim",
    patient: patientResult ? { reference: `Patient/${patientResult.id}` } : undefined,
    created: new Date().toISOString(),
    provider: organizationResult ? { reference: `Organization/${organizationResult.id}` } : undefined,
    priority: { coding: [{ code: "normal" }] },
    prescription: medicationRequestResult
      ? { reference: `MedicationRequest/${medicationRequestResult.id}` }
      : undefined,
    insurance: [
      {
        sequence: 1,
        focal: true,
        coverage: { display: "Test Coverage" },
      },
    ],
    item: [
      {
        sequence: 1,
        productOrService: { coding: [{ code: "exam" }] },
        encounter: encounterResult ? [{ reference: `Encounter/${encounterResult.id}` }] : undefined,
      },
    ],
  });

  // ExplanationOfBenefit with contained resources
  const explanationOfBenefitResult = await createResource(token, "ExplanationOfBenefit", {
    resourceType: "ExplanationOfBenefit",
    contained: [
      {
        resourceType: "ServiceRequest",
        id: "referral",
        status: "completed",
        intent: "order",
        subject: patientResult ? { reference: `Patient/${patientResult.id}` } : { display: "Patient" },
        requester: practitionerResult
          ? { reference: `Practitioner/${practitionerResult.id}` }
          : { display: "Practitioner" },
        performer: practitionerResult
          ? [{ reference: `Practitioner/${practitionerResult.id}` }]
          : [{ display: "Practitioner" }],
      },
      {
        resourceType: "Coverage",
        id: "coverage",
        status: "active",
        type: { text: "private" },
        beneficiary: patientResult ? { reference: `Patient/${patientResult.id}` } : { display: "Patient" },
        payor: [{ display: "private" }],
      },
    ],
    status: "active",
    type: { coding: [{ system: "http://terminology.hl7.org/CodeSystem/claim-type", code: "institutional" }] },
    use: "claim",
    patient: patientResult ? { reference: `Patient/${patientResult.id}` } : { display: "Patient" },
    created: new Date().toISOString(),
    insurer: { display: "private" },
    provider: practitionerResult
      ? { reference: `Practitioner/${practitionerResult.id}` }
      : { display: "Practitioner" },
    referral: { reference: "#referral" },
    claim: claimResult ? { reference: `Claim/${claimResult.id}` } : { display: "Claim" },
    outcome: "complete",
    careTeam: [
      {
        sequence: 1,
        provider: practitionerResult
          ? { reference: `Practitioner/${practitionerResult.id}` }
          : { display: "Practitioner" },
        role: { coding: [{ code: "primary" }] },
      },
    ],
    insurance: [
      {
        focal: true,
        coverage: { reference: "#coverage", display: "private" },
      },
    ],
    item: [
      {
        sequence: 1,
        productOrService: { coding: [{ code: "exam" }] },
        encounter: encounterResult ? [{ reference: `Encounter/${encounterResult.id}` }] : undefined,
      },
    ],
    total: [
      {
        category: { coding: [{ code: "submitted" }] },
        amount: { value: 100, currency: "USD" },
      },
    ],
  });

  // === SUMMARY ===
  console.log("\n=== Results ===");
  const resources = {
    Patient: patientResult,
    Practitioner: practitionerResult,
    Organization: organizationResult,
    Location: locationResult,
    Encounter: encounterResult,
    Observation: observationResult,
    MedicationRequest: medicationRequestResult,
    Claim: claimResult,
    ExplanationOfBenefit: explanationOfBenefitResult,
  };

  for (const [rt, result] of Object.entries(resources)) {
    if (result) {
      console.log(`  ${rt}: ${result.id} ✓`);
    } else {
      console.log(`  ${rt}: FAILED ✗`);
    }
  }

  // === DELETE IN REVERSE ORDER (to respect referential integrity) ===
  console.log("\n=== Deleting resources (reverse order) ===");
  const deleteOrder = [
    "ExplanationOfBenefit",
    "Claim",
    "MedicationRequest",
    "Observation",
    "Encounter",
    "Location",
    "Organization",
    "Practitioner",
    "Patient",
  ];

  for (const rt of deleteOrder) {
    const result = resources[rt as keyof typeof resources];
    if (result?.id) {
      await deleteResource(token, rt, result.id);
    }
  }
}

main().catch(console.error);
