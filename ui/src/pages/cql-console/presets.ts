/**
 * Quick-start content for the CQL console.
 *
 * Examples stick to features the engine actually evaluates: `library`, `using`,
 * `context`, `parameter` and multiple `define` statements. (valueset / include /
 * define function are not yet supported by the parser, so they're avoided here.)
 */

export type SampleKey = "patient" | "observation" | "none";

export interface CqlExample {
  label: string;
  /** Full source inserted into the editor. */
  source: string;
  /** Sample context resource this example is meant to run against. */
  sample?: SampleKey;
}

const ADULT_CHECK = `library AdultCheck version '1.0.0'
using FHIR version '4.0.1'
context Patient

define Birth: Patient.birthDate
define IsAdult: AgeInYears() >= 18
define Greeting: 'Hello, ' + First(Patient.name.given)`;

const ARITHMETIC = `library Playground version '1.0.0'

define Sum: 1 + 2 + 3
define Product: 6 * 7
define Comparison: 5 > 3
define Numbers: { 1, 2, 3, 4, 5 }
define Total: Count({ 1, 2, 3, 4, 5 })
define Average: Avg({ 10.0, 20.0, 30.0 })`;

const INTERVALS = `library Intervals version '1.0.0'

define Range: Interval[3, 10]
define Low: start of Range
define High: end of Range
define Width: width of Range
define Contains: 5 in Range`;

export const CQL_EXAMPLES: CqlExample[] = [
  { label: "Arithmetic", source: ARITHMETIC, sample: "none" },
  { label: "Intervals", source: INTERVALS, sample: "none" },
  { label: "Patient (context)", source: ADULT_CHECK, sample: "patient" },
];

export const DEFAULT_SOURCE = ARITHMETIC;

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
