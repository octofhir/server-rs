import type { ChartSpec, EChartsType } from "@octofhir/ui-kit";
import type { SqlResponse, SqlValue } from "@/shared/api/types";

function quoteCSVField(value: string): string {
  if (value.includes(",") || value.includes('"') || value.includes("\n")) {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return value;
}

function cellToCSV(cell: SqlValue): string {
  if (cell === null) return "";
  if (typeof cell === "object") return quoteCSVField(JSON.stringify(cell));
  return quoteCSVField(String(cell));
}

export function resultToCSV(columns: string[], rows: SqlValue[][]): string {
  const header = columns.map(quoteCSVField).join(",");
  const body = rows.map((row) => row.map(cellToCSV).join(","));
  return [header, ...body].join("\n");
}

/** Convert a SQL result into an array of plain row objects keyed by column name. */
export function resultToObjects(data: SqlResponse): Record<string, SqlValue>[] {
  return data.rows.map((row) => {
    const obj: Record<string, SqlValue> = {};
    data.columns.forEach((column, index) => {
      obj[column] = row[index] ?? null;
    });
    return obj;
  });
}

export function resultToJSON(data: SqlResponse): string {
  return JSON.stringify(resultToObjects(data), null, 2);
}

export function downloadCSV(columns: string[], rows: SqlValue[][]): void {
  const csv = resultToCSV(columns, rows);
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.setAttribute("href", url);
  link.setAttribute("download", `query-results-${Date.now()}.csv`);
  link.style.visibility = "hidden";
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
}

/** Infer a lightweight column type hint from the first non-null sample value. */
export function inferColumnType(rows: SqlValue[][], columnIndex: number): string {
  for (const row of rows) {
    const value = row[columnIndex];
    if (value === null || value === undefined) continue;
    if (typeof value === "number") return Number.isInteger(value) ? "int" : "num";
    if (typeof value === "boolean") return "bool";
    if (typeof value === "object") return "json";
    // Heuristics for common string shapes.
    if (/^\d{4}-\d{2}-\d{2}([T\s]\d{2}:\d{2})?/.test(value)) return "date";
    if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-/i.test(value)) return "uuid";
    return "text";
  }
  return "null";
}

/** Case-insensitive substring match of a value against a filter term. */
export function cellMatches(cell: SqlValue, term: string): boolean {
  if (cell === null) return "null".includes(term);
  const text = typeof cell === "object" ? JSON.stringify(cell) : String(cell);
  return text.toLowerCase().includes(term);
}

// --- Chart spec persistence ---------------------------------------------------

/** Stable djb2 hash of the query text — keys persisted chart specs. */
export function hashQuery(query: string): string {
  let hash = 5381;
  for (let i = 0; i < query.length; i++) {
    hash = (hash * 33) ^ query.charCodeAt(i);
  }
  return (hash >>> 0).toString(36);
}

const CHART_SPEC_PREFIX = "db-console-chart:";

export function loadChartSpec(queryHash: string): ChartSpec | null {
  try {
    const raw = localStorage.getItem(`${CHART_SPEC_PREFIX}${queryHash}`);
    return raw ? (JSON.parse(raw) as ChartSpec) : null;
  } catch {
    return null;
  }
}

export function saveChartSpec(queryHash: string, spec: ChartSpec): void {
  try {
    localStorage.setItem(`${CHART_SPEC_PREFIX}${queryHash}`, JSON.stringify(spec));
  } catch {
    // ignore quota / serialization errors
  }
}

/** Export a chart instance to a PNG and trigger a download. */
export function downloadChartPNG(instance: EChartsType): void {
  const url = instance.getDataURL({ pixelRatio: 2, backgroundColor: "#fff" });
  const link = document.createElement("a");
  link.setAttribute("href", url);
  link.setAttribute("download", `chart-${Date.now()}.png`);
  link.style.visibility = "hidden";
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
}
