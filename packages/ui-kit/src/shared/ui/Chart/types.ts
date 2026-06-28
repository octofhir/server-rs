/** Chart spec — a serializable description of a chart, decoupled from ECharts. */

export type ChartType = "bar" | "line" | "area" | "pie" | "scatter";

export type Aggregation = "none" | "sum" | "avg" | "count" | "min" | "max";

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
    /** Dimension column (category axis / pie label / scatter x). */
    x?: string;
    /** Measure columns. */
    series: ChartSeriesSpec[];
    /** Pivot column — produces one series per distinct value (uses series[0]). */
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
}

/** Generic tabular input — column names + positional rows. */
export interface TabularData {
    columns: string[];
    rows: unknown[][];
}

/** Lightweight column type hints (see inferColumnType). */
export type ColumnType = "int" | "num" | "bool" | "date" | "uuid" | "json" | "text" | "null";
