/**
 * Parsing for the `$cql` operation response.
 *
 * Two shapes:
 *   • Single expression — { parameter: [{name:"expression",…},{name:"return",valueString}] }
 *   • Library (multi-define) — { parameter: [{name:"result", part:[{name,valueString}, …]}] }
 *
 * Both collapse to an ordered list of named defines so the UI renders them
 * uniformly.
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

export interface CqlDefine {
  name: string;
  /** Parsed result value. */
  value: unknown;
  datatype: CqlDatatype;
  /** Raw JSON-serialized string from the server. */
  raw: string;
}

export interface CqlEvaluationResult {
  mode: "expression" | "library";
  defines: CqlDefine[];
}

interface FhirParameterPart {
  name: string;
  valueString?: string;
  valueInteger?: number;
  part?: FhirParameterPart[];
}

export interface CqlDiagnostic {
  severity: string;
  message: string;
  line?: number;
  column?: number;
}

/** Parse the parse-only `validate` response into a flat diagnostics list. */
export function parseValidateResponse(params: {
  parameter?: FhirParameterPart[];
}): CqlDiagnostic[] {
  return (params.parameter ?? [])
    .filter((p) => p.name === "issue")
    .map((p) => {
      const find = (n: string) => p.part?.find((x) => x.name === n);
      return {
        severity: find("severity")?.valueString ?? "error",
        message: find("message")?.valueString ?? "Unknown error",
        line: find("line")?.valueInteger,
        column: find("column")?.valueInteger,
      };
    });
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

function toDefine(name: string, raw: string): CqlDefine {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    value = raw;
  }
  return { name, value, datatype: inferDatatype(value), raw };
}

export function parseCqlResponse(params: { parameter?: FhirParameterPart[] }): CqlEvaluationResult {
  const parts = params.parameter ?? [];

  // Library mode: a single `result` parameter carrying one part per define.
  const resultParam = parts.find((p) => p.name === "result");
  if (resultParam?.part) {
    const defines = resultParam.part
      .filter((p) => p.valueString !== undefined)
      .map((p) => toDefine(p.name, p.valueString ?? "null"));
    return { mode: "library", defines };
  }

  // Expression mode: a single `return` value.
  const raw = parts.find((p) => p.name === "return")?.valueString ?? "null";
  return { mode: "expression", defines: [toDefine("Result", raw)] };
}
