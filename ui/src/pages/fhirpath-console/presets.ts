/**
 * Quick-start content for the FHIRPath console: example expressions and
 * sample resources users can insert with one click.
 */

export interface ExpressionExample {
  label: string;
  expression: string;
  /** Sample resource key this expression is meant to run against. */
  sample?: SampleKey;
}

export const EXPRESSION_EXAMPLES: ExpressionExample[] = [
  { label: "Given names", expression: "Patient.name.given", sample: "patient" },
  {
    label: "Official family",
    expression: "Patient.name.where(use = 'official').family",
    sample: "patient",
  },
  {
    label: "Full name",
    expression: "Patient.name.given.first() + ' ' + Patient.name.family",
    sample: "patient",
  },
  { label: "Is adult", expression: "Patient.birthDate <= today() - 18 years", sample: "patient" },
  {
    label: "Phone contacts",
    expression: "Patient.telecom.where(system = 'phone').value",
    sample: "patient",
  },
  {
    label: "Obs value",
    expression: "Observation.value.ofType(Quantity).value",
    sample: "observation",
  },
  {
    label: "Bundle patient ids",
    expression: "Bundle.entry.resource.ofType(Patient).id",
    sample: "bundle",
  },
];

export type SampleKey = "patient" | "observation" | "bundle";

export interface SampleResource {
  key: SampleKey;
  label: string;
  resourceType: string;
  json: unknown;
}

export const SAMPLE_RESOURCES: SampleResource[] = [
  {
    key: "patient",
    label: "Patient",
    resourceType: "Patient",
    json: {
      resourceType: "Patient",
      id: "example",
      active: true,
      name: [
        { use: "official", family: "Smith", given: ["John", "Bob"] },
        { use: "nickname", given: ["Johnny"] },
      ],
      gender: "male",
      birthDate: "1985-04-12",
      telecom: [
        { system: "phone", value: "+1-555-0100", use: "home" },
        { system: "email", value: "john.smith@example.com" },
      ],
    },
  },
  {
    key: "observation",
    label: "Observation",
    resourceType: "Observation",
    json: {
      resourceType: "Observation",
      id: "bp-example",
      status: "final",
      code: {
        coding: [{ system: "http://loinc.org", code: "29463-7", display: "Body weight" }],
      },
      valueQuantity: {
        value: 72.5,
        unit: "kg",
        system: "http://unitsofmeasure.org",
        code: "kg",
      },
      effectiveDateTime: "2026-06-01T09:30:00Z",
    },
  },
  {
    key: "bundle",
    label: "Bundle",
    resourceType: "Bundle",
    json: {
      resourceType: "Bundle",
      type: "collection",
      entry: [
        {
          resource: {
            resourceType: "Patient",
            id: "p1",
            name: [{ family: "Doe", given: ["Jane"] }],
          },
        },
        {
          resource: {
            resourceType: "Patient",
            id: "p2",
            name: [{ family: "Roe", given: ["Richard"] }],
          },
        },
      ],
    },
  },
];

export function sampleJsonString(key: SampleKey): string {
  const sample = SAMPLE_RESOURCES.find((s) => s.key === key);
  return JSON.stringify(sample?.json ?? {}, null, 2);
}
