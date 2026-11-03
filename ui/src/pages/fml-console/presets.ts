/** A starting map and inputs for the FML console. */

export const DEFAULT_SOURCE = `/// url = 'http://example.org/StructureMap/PatientToBasic'
/// name = 'PatientToBasic'
/// title = 'Patient to Basic'

uses "http://hl7.org/fhir/StructureDefinition/Patient" alias Patient as source
uses "http://hl7.org/fhir/StructureDefinition/Basic" alias Basic as target

group Main(source src : Patient, target tgt : Basic) {
  src.id as v -> tgt.id = v "id";
  src -> tgt.code = cc('http://example.org/kind', 'patient', 'Patient') "code";
  src.name as n -> tgt.extension as e, e.url = 'http://example.org/name' then Name(n, e) "name";
  src.birthDate as d -> tgt.created = d "created";
}

group Name(source n, target e) {
  n.family as f, n.given as g -> e.value = append(g, ' ', f) "fullName";
}
`;

export const SAMPLE_INPUTS = {
  patient: {
    resourceType: "Patient",
    id: "example",
    name: [{ family: "Chalmers", given: ["Peter"] }],
    gender: "male",
    birthDate: "1974-12-25",
  },
  observation: {
    resourceType: "Observation",
    id: "bp",
    status: "final",
    code: { coding: [{ system: "http://loinc.org", code: "85354-9" }] },
    subject: { reference: "Patient/example" },
    valueQuantity: { value: 120, unit: "mmHg" },
  },
} as const;

export type SampleKey = keyof typeof SAMPLE_INPUTS;

export function sampleJson(key: SampleKey): string {
  return JSON.stringify(SAMPLE_INPUTS[key], null, 2);
}
