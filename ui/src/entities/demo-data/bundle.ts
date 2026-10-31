import type { FhirResource } from "@/shared/api/types";

/**
 * Sample clinic for a fresh server: 5 organizations, 8 practitioners, 30 patients, each with an
 * encounter and two vital signs, and a condition for every third patient. Mirrors
 * `scripts/seed-demo.ts`, but as one transaction with `urn:uuid` references, so it either loads
 * completely or not at all.
 */
export function buildDemoBundle(): { resourceType: "Bundle"; type: "transaction"; entry: unknown[] } {
  const entries: Array<{ fullUrl: string; resource: FhirResource & { meta: { tag: unknown[] } } }> = [];
  const add = (resource: FhirResource): string => {
    const fullUrl = `urn:uuid:${crypto.randomUUID()}`;
    entries.push({ fullUrl, resource: { ...resource, meta: { tag: [DEMO_TAG] } } });
    return fullUrl;
  };

  const orgs = ORGS.map((name, i) =>
    add({
      resourceType: "Organization",
      name,
      active: true,
      address: [{ city: pick(CITIES, i), country: "US" }],
    })
  );

  const practitioners = Array.from({ length: 8 }, (_, i) =>
    add({
      resourceType: "Practitioner",
      active: true,
      name: [{ given: [pick(GIVEN, i + 3)], family: pick(FAMILY, i + 5), prefix: ["Dr"] }],
      gender: i % 2 === 0 ? "female" : "male",
    })
  );

  for (let i = 0; i < 30; i++) {
    const year = 1945 + ((i * 7) % 70);
    const month = String(1 + (i % 12)).padStart(2, "0");
    const day = String(1 + (i % 27)).padStart(2, "0");
    const patient = add({
      resourceType: "Patient",
      active: i % 9 !== 0,
      name: [{ given: [pick(GIVEN, i)], family: pick(FAMILY, i * 3) }],
      gender: pick(["male", "female", "other", "unknown"], i),
      birthDate: `${year}-${month}-${day}`,
      telecom: [{ system: "phone", value: `555-01${String(i).padStart(2, "0")}`, use: "mobile" }],
      address: [{ city: pick(CITIES, i), country: "US" }],
      managingOrganization: { reference: pick(orgs, i) },
      generalPractitioner: [{ reference: pick(practitioners, i) }],
    });

    const encounter = add({
      resourceType: "Encounter",
      status: pick(["finished", "in-progress", "planned"], i),
      class: {
        system: "http://terminology.hl7.org/CodeSystem/v3-ActCode",
        code: pick(["AMB", "IMP", "EMER"], i),
        display: pick(["ambulatory", "inpatient encounter", "emergency"], i),
      },
      subject: { reference: patient },
      participant: [{ individual: { reference: pick(practitioners, i) } }],
    });

    for (const vital of VITALS) {
      add({
        resourceType: "Observation",
        status: "final",
        category: [
          {
            coding: [
              {
                system: "http://terminology.hl7.org/CodeSystem/observation-category",
                code: "vital-signs",
                display: "Vital Signs",
              },
            ],
          },
        ],
        code: { coding: [{ system: "http://loinc.org", code: vital.loinc, display: vital.name }], text: vital.name },
        subject: { reference: patient },
        encounter: { reference: encounter },
        effectiveDateTime: `2026-0${1 + (i % 6)}-1${i % 9}T09:00:00Z`,
        valueQuantity: {
          value: vital.base + (i % vital.spread),
          unit: vital.unit,
          system: "http://unitsofmeasure.org",
          code: vital.ucum,
        },
      });
    }

    if (i % 3 === 0) {
      add({
        resourceType: "Condition",
        clinicalStatus: {
          coding: [
            {
              system: "http://terminology.hl7.org/CodeSystem/condition-clinical",
              code: pick(["active", "resolved", "recurrence"], i),
            },
          ],
        },
        code: {
          coding: [{ system: "http://snomed.info/sct", code: "44054006", display: "Diabetes mellitus type 2" }],
          text: "Type 2 Diabetes",
        },
        subject: { reference: patient },
        recordedDate: `2025-1${i % 3}-15`,
      });
    }
  }

  return {
    resourceType: "Bundle",
    type: "transaction",
    entry: entries.map(({ fullUrl, resource }) => ({
      fullUrl,
      resource,
      request: { method: "POST", url: resource.resourceType },
    })),
  };
}

/** Every demo resource carries this tag, so `?_tag=demo-data` finds it all again. */
export const DEMO_TAG = { system: "https://octofhir.com/tags", code: "demo-data", display: "OctoFHIR demo data" };

const pick = <T,>(items: T[], i: number): T => items[i % items.length];

const VITALS = [
  { name: "Heart rate", loinc: "8867-4", unit: "beats/minute", ucum: "/min", base: 60, spread: 40 },
  { name: "Body weight", loinc: "29463-7", unit: "kg", ucum: "kg", base: 50, spread: 60 },
];

const GIVEN = [
  "John", "Jane", "Robert", "Mary", "Michael", "Patricia", "David", "Linda", "James", "Barbara",
  "William", "Elizabeth", "Richard", "Susan", "Joseph", "Jessica", "Thomas", "Sarah", "Charles", "Karen",
];
const FAMILY = [
  "Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller", "Davis", "Rodriguez", "Martinez",
  "Hernandez", "Lopez", "Wilson", "Anderson", "Taylor", "Thomas", "Moore", "Jackson", "Martin", "Lee",
];
const CITIES = ["Boston", "Austin", "Denver", "Seattle", "Chicago", "Portland", "Miami", "Phoenix"];
const ORGS = ["Mercy General", "Lakeside Clinic", "Summit Health", "Riverside Medical", "Pine Valley Hospital"];
