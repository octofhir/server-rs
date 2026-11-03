import { isRecord } from "@/shared/api/guards";

/** A problem in the FML source, from `/api/fml/check` (1-based lines, UTF-16 columns). */
export interface Problem {
  severity: "error" | "warning";
  message: string;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
}

export interface CheckResult {
  url?: string;
  name?: string;
  problems: Problem[];
}

export function parseCheckResult(data: unknown): CheckResult {
  if (!isRecord(data)) return { problems: [] };
  return {
    url: typeof data.url === "string" ? data.url : undefined,
    name: typeof data.name === "string" ? data.name : undefined,
    problems: Array.isArray(data.problems) ? (data.problems as Problem[]) : [],
  };
}
