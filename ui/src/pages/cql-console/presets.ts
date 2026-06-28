/**
 * Quick-start content for the CQL console: example expressions and sample
 * resources users can insert with one click.
 */

export type SampleKey = "patient" | "observation" | "none";

export interface ExpressionExample {
  label: string;
  expression: string;
  /** Sample resource this expression is meant to run against. */
  sample?: SampleKey;
}

export const EXPRESSION_EXAMPLES: ExpressionExample[] = [
  { label: "Arithmetic", expression: "1 + 1", sample: "none" },
  { label: "Boolean", expression: "true and false", sample: "none" },
  { label: "String concat", expression: "'Hello' + ' ' + 'World'", sample: "none" },
  { label: "Comparison", expression: "5 > 3", sample: "none" },
  { label: "List", expression: "{1, 2, 3, 4, 5}", sample: "none" },
  { label: "List count", expression: "Count({1, 2, 3, 4, 5})", sample: "none" },
  { label: "Interval", expression: "Interval[3, 5]", sample: "none" },
  { label: "Coalesce", expression: "Coalesce(null, 'fallback')", sample: "none" },
];

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
      name: [{ use: "official", family: "Smith", given: ["John", "Bob"] }],
      gender: "male",
      birthDate: "1985-04-12",
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
];

export function sampleJsonString(key: SampleKey): string {
  if (key === "none") return "";
  const sample = SAMPLE_RESOURCES.find((s) => s.key === key);
  return sample ? JSON.stringify(sample.json, null, 2) : "";
}
