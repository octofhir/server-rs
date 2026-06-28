/**
 * Parsing for the `$cql` operation response.
 *
 * The backend returns a FHIR Parameters resource shaped as:
 *   { resourceType: "Parameters", parameter: [
 *       { name: "expression", valueString: "<expr>" },
 *       { name: "return",     valueString: "<json-serialized result>" }
 *   ]}
 *
 * Unlike FHIRPath (which returns a typed collection), CQL returns a single
 * result value serialized as a JSON string.
 */

export type CqlDatatype =
  | "string"
  | "integer"
  | "decimal"
  | "boolean"
  | "date"
  | "dateTime"
  | "time"
  | "list"
  | "tuple"
  | "null";

export interface CqlEvaluationResult {
  expression: string;
  /** Parsed result value (string, number, boolean, array, object, …). */
  value: unknown;
  /** Inferred CQL datatype for display tagging. */
  datatype: CqlDatatype;
  /** Raw JSON-serialized string the server returned. */
  raw: string;
}

interface FhirParameterPart {
  name: string;
  valueString?: string;
}

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;
const ISO_DATETIME = /^\d{4}-\d{2}-\d{2}T/;
const ISO_TIME = /^\d{2}:\d{2}/;

export function inferDatatype(value: unknown): CqlDatatype {
  if (value === null || value === undefined) return "null";
  if (Array.isArray(value)) return "list";
  if (typeof value === "boolean") return "boolean";
  if (typeof value === "number") return Number.isInteger(value) ? "integer" : "decimal";
  if (typeof value === "object") return "tuple";
  if (typeof value === "string") {
    if (ISO_DATETIME.test(value)) return "dateTime";
    if (ISO_DATE.test(value)) return "date";
    if (ISO_TIME.test(value)) return "time";
    return "string";
  }
  return "string";
}

export function parseCqlResponse(params: { parameter?: FhirParameterPart[] }): CqlEvaluationResult {
  const parts = params.parameter ?? [];
  const expression = parts.find((p) => p.name === "expression")?.valueString ?? "";
  const raw = parts.find((p) => p.name === "return")?.valueString ?? "null";

  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    // Server returned a bare (non-JSON) string — keep it verbatim.
    value = raw;
  }

  return { expression, value, datatype: inferDatatype(value), raw };
}
