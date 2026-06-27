import type { DbColumnInfo, DbIndexInfo, DbTableInfo } from "@/shared/api/types";

export interface DbSchemaTableView {
  id: string;
  schema: string;
  name: string;
  displayName: string;
  kind: string;
  isView: boolean;
  rowEstimateLabel?: string;
  rowEstimate?: number;
  deadRows?: number;
  /** dead/(live+dead) — bloat signal; undefined when no rows. */
  deadRatio?: number;
  totalSizeBytes?: number;
  totalSizeLabel?: string;
  tableSizeLabel?: string;
  indexesSizeLabel?: string;
  lastVacuumLabel?: string;
  lastAnalyzeLabel?: string;
}

export interface DbColumnView {
  id: string;
  name: string;
  dataType: string;
  nullability: "nullable" | "required";
}

export interface DbIndexView {
  id: string;
  name: string;
  indexType: string;
  columnList: string;
  isPrimary: boolean;
  isUnique: boolean;
  sizeLabel?: string;
  definition?: string;
}

export function formatDbSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** Relative time-ago label from an ISO timestamp; "never" when missing. */
function formatLastRun(ts?: string): string {
  if (!ts) return "never";
  const then = new Date(ts).getTime();
  if (Number.isNaN(then)) return "never";
  const diffMs = Date.now() - then;
  const min = Math.floor(diffMs / 60000);
  if (min < 1) return "just now";
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  return `${Math.floor(hr / 24)}d ago`;
}

/** Most recent of manual + auto vacuum/analyze. */
function mostRecent(a?: string, b?: string): string | undefined {
  if (!a) return b;
  if (!b) return a;
  return new Date(a).getTime() >= new Date(b).getTime() ? a : b;
}

export function getDbSchemaTableView(table: DbTableInfo): DbSchemaTableView {
  const isView = table.tableType === "VIEW";
  const live = table.rowEstimate ?? 0;
  const dead = table.deadRows ?? 0;
  const deadRatio = live + dead > 0 ? dead / (live + dead) : undefined;
  const lastVacuum = mostRecent(table.lastVacuum, table.lastAutovacuum);
  const lastAnalyze = mostRecent(table.lastAnalyze, table.lastAutoanalyze);

  return {
    id: `${table.schema}.${table.name}`,
    schema: table.schema,
    name: table.name,
    displayName: table.schema !== "public" ? `${table.schema}.${table.name}` : table.name,
    kind: table.tableType.toLowerCase(),
    isView,
    rowEstimateLabel:
      table.rowEstimate != null && table.rowEstimate > 0
        ? `~${table.rowEstimate.toLocaleString()} rows`
        : undefined,
    rowEstimate: table.rowEstimate,
    deadRows: table.deadRows,
    deadRatio,
    totalSizeBytes: table.totalSizeBytes,
    totalSizeLabel: table.totalSizeBytes != null ? formatDbSize(table.totalSizeBytes) : undefined,
    tableSizeLabel: table.tableSizeBytes != null ? formatDbSize(table.tableSizeBytes) : undefined,
    indexesSizeLabel:
      table.indexesSizeBytes != null ? formatDbSize(table.indexesSizeBytes) : undefined,
    lastVacuumLabel: isView ? undefined : formatLastRun(lastVacuum),
    lastAnalyzeLabel: isView ? undefined : formatLastRun(lastAnalyze),
  };
}

export function getDbSchemaTableViews(tables: DbTableInfo[]): DbSchemaTableView[] {
  return tables.map(getDbSchemaTableView);
}

export function filterDbSchemaTables(tables: DbTableInfo[], search: string): DbTableInfo[] {
  const query = search.trim().toLowerCase();
  if (!query) return tables;

  return tables.filter(
    (table) =>
      table.name.toLowerCase().includes(query) || table.schema.toLowerCase().includes(query)
  );
}

export function getDbColumnViews(columns: DbColumnInfo[]): DbColumnView[] {
  return columns.map((column) => ({
    id: column.name,
    name: column.name,
    dataType: column.dataType,
    nullability: column.isNullable ? "nullable" : "required",
  }));
}

export function getDbIndexViews(indexes: DbIndexInfo[]): DbIndexView[] {
  return indexes.map((index) => ({
    id: index.name,
    name: index.name,
    indexType: index.indexType,
    columnList: `(${index.columns.join(", ")})`,
    isPrimary: index.isPrimary,
    isUnique: index.isUnique,
    sizeLabel: index.sizeBytes != null ? formatDbSize(index.sizeBytes) : undefined,
    definition: index.definition,
  }));
}
