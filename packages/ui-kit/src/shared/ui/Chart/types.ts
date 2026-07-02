/** Chart spec — a serializable description of a chart, decoupled from ECharts. */

export type ChartType = "bar" | "line" | "area" | "pie" | "scatter";

export type Aggregation = "none" | "sum" | "avg" | "count" | "min" | "max";

/** Calendar bucket for truncating a date X-axis. */
export type DateBucket = "year" | "quarter" | "month" | "week" | "day";

export type ChartPalette = "theme" | "vibrant" | "cool" | "warm" | "sunset" | "mono";

export interface ChartSeriesSpec {
  /** Source column name for this measure. */
  column: string;
  /** Aggregation applied per x-category. Defaults to "none". */
  agg?: Aggregation;
  /** Display name; falls back to the column name. */
  name?: string;
}

export interface ChartSpec {
  type: ChartType;
  /** Dimension field (category axis / pie label / scatter x). */
  x?: string;
  /** Measure fields. */
  series: ChartSeriesSpec[];
  /** Pivot field — produces one series per distinct value (uses series[0]). */
  groupBy?: string;
  /** Stack bar/area series. */
  stack?: boolean;
  /** Smooth line/area. */
  smooth?: boolean;
  /** Horizontal orientation (bar only). */
  horizontal?: boolean;
  /** Show the legend. */
  legend?: boolean;
  title?: string;
  xLabel?: string;
  yLabel?: string;
  /** Truncate a date X-axis into calendar buckets (e.g. births per year). */
  xBucket?: DateBucket;
  /** Force the value axis to include zero. Defaults: bar=true, others=false. */
  yZero?: boolean;
  /** Fixed value-axis minimum (auto when undefined). */
  yMin?: number;
  /** Fixed value-axis maximum (auto when undefined). */
  yMax?: number;
  /** Named categorical color palette (see PALETTES). Falls back to theme colors. */
  palette?: ChartPalette;
  /** Draw value labels on the series (bars/points/slices). */
  showLabels?: boolean;
  /** Render pie as a donut (ignored for non-pie). */
  donut?: boolean;
  /**
   * Derived fields extracted from JSON cells (micro-DBT). When present, the
   * builder flattens the source data through these before charting. Field
   * `name`s are referenced by `x` / `series[].column` / `groupBy`.
   */
  derive?: DerivedField[];
}

/** A flattened/extracted field definition discovered from tabular data. */
export interface FieldDef {
  /** Stable identifier — equals the path; referenced by ChartSpec fields. */
  name: string;
  /** Extraction path, e.g. `resource.name[0].family` or `telecom[].value`. */
  path: string;
  /** Short display label, e.g. `name[0].family`. */
  label: string;
  /** Inferred value type. */
  type: ColumnType;
  /** Top-level source column the path roots in. */
  sourceColumn: string;
  /** True when the path flattens an array (`[]`) — explodes rows. */
  array: boolean;
}

/** Minimal serializable form of a derived field (persisted in ChartSpec). */
export interface DerivedField {
  name: string;
  path: string;
  array?: boolean;
  type?: ColumnType;
}

/** Generic tabular input — column names + positional rows. */
export interface TabularData {
  columns: string[];
  rows: unknown[][];
}

/** Lightweight column type hints (see inferColumnType). */
export type ColumnType = "int" | "num" | "bool" | "date" | "uuid" | "json" | "text" | "null";
