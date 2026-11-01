/**
 * The visual query builder's model: a structured query that renders to PromQL, and the inverse
 * for queries in the shapes the builder writes. PromQL stays the stored form; the builder is a
 * way to write it.
 */

export type MatchOp = "=" | "!=" | "=~" | "!~";

export interface LabelFilter {
  label: string;
  op: MatchOp;
  value: string;
}

export type RangeFunction = "rate" | "irate" | "increase" | "avg_over_time" | "max_over_time" | "min_over_time";
export type BuilderFunction = "none" | RangeFunction | "histogram_quantile";
export type Aggregation = "none" | "sum" | "avg" | "max" | "min" | "count";

export interface BuilderQuery {
  metric: string;
  filters: LabelFilter[];
  fn: BuilderFunction;
  /** Range for range functions, e.g. `$__rate_interval`, `$__range`, `5m`. */
  range: string;
  /** histogram_quantile only, 0–1. */
  quantile: number;
  aggregation: Aggregation;
  by: string[];
  /** Keep only the largest k series; 0 keeps all. */
  topk: number;
}

export const RANGE_FUNCTIONS: RangeFunction[] = ["rate", "irate", "increase", "avg_over_time", "max_over_time", "min_over_time"];

export function emptyBuilder(metric = ""): BuilderQuery {
  return { metric, filters: [], fn: "none", range: "$__rate_interval", quantile: 0.95, aggregation: "none", by: [], topk: 0 };
}

/** A sensible start for a metric: counters get a rate, histograms a p95. */
export function builderFor(metric: string, type: string | null | undefined): BuilderQuery {
  const base = emptyBuilder(metric);
  if (metric.endsWith("_bucket") || type === "histogram") {
    return { ...base, metric: metric.endsWith("_bucket") ? metric : `${metric}_bucket`, fn: "histogram_quantile" };
  }
  if (metric.endsWith("_total") || type === "counter") return { ...base, fn: "rate", aggregation: "sum" };
  return base;
}

function quote(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function selector(query: BuilderQuery): string {
  const filters = query.filters.filter((f) => f.label.trim() !== "");
  if (filters.length === 0) return query.metric;
  return `${query.metric}{${filters.map((f) => `${f.label}${f.op}${quote(f.value)}`).join(", ")}}`;
}

/** Renders the builder's query as PromQL. */
export function toPromql(query: BuilderQuery): string {
  if (!query.metric.trim()) return "";
  const sel = selector(query);
  let expr: string;
  if (query.fn === "histogram_quantile") {
    const by = ["le", ...query.by.filter((l) => l !== "le")].join(", ");
    expr = `histogram_quantile(${query.quantile}, sum by (${by}) (rate(${sel}[${query.range}])))`;
  } else {
    const inner = query.fn === "none" ? sel : `${query.fn}(${sel}[${query.range}])`;
    expr =
      query.aggregation === "none"
        ? inner
        : query.by.length > 0
          ? `${query.aggregation} by (${query.by.join(", ")}) (${inner})`
          : `${query.aggregation}(${inner})`;
  }
  return query.topk > 0 ? `topk(${query.topk}, ${expr})` : expr;
}

const METRIC = /^([a-zA-Z_:][a-zA-Z0-9_:]*)(?:\{(.*)\})?$/s;
const MATCHER = /^\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*(=~|!~|!=|=)\s*"((?:[^"\\]|\\.)*)"\s*$/s;

function splitMatchers(text: string): string[] | null {
  const parts: string[] = [];
  let current = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (quoted && ch === "\\") {
      current += ch + (text[i + 1] ?? "");
      i++;
      continue;
    }
    if (ch === '"') quoted = !quoted;
    if (ch === "," && !quoted) {
      parts.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  if (quoted) return null;
  if (current.trim()) parts.push(current);
  return parts;
}

function parseSelector(text: string): Pick<BuilderQuery, "metric" | "filters"> | null {
  const match = METRIC.exec(text.trim());
  if (!match) return null;
  const filters: LabelFilter[] = [];
  if (match[2]?.trim()) {
    const parts = splitMatchers(match[2]);
    if (!parts) return null;
    for (const part of parts) {
      const m = MATCHER.exec(part);
      if (!m) return null;
      filters.push({ label: m[1], op: m[2] as MatchOp, value: m[3].replace(/\\(.)/g, "$1") });
    }
  }
  return { metric: match[1], filters };
}

function splitBy(text: string | undefined): string[] {
  return (text ?? "").split(",").map((l) => l.trim()).filter(Boolean);
}

/**
 * Reads a query back into the builder when it has one of the shapes the builder writes;
 * `null` otherwise (the query is then edited as code).
 */
export function fromPromql(promql: string): BuilderQuery | null {
  let text = promql.trim().replace(/\s+/g, " ");
  if (!text) return emptyBuilder();
  const query = emptyBuilder();

  const topk = /^topk\((\d+), (.*)\)$/.exec(text);
  if (topk) {
    query.topk = Number(topk[1]);
    text = topk[2];
  }

  const hist = /^histogram_quantile\(([\d.]+), sum by \(([^)]*)\) \(rate\((.+)\[([^\]]+)\]\)\)\)$/.exec(text);
  if (hist) {
    const sel = parseSelector(hist[3]);
    const by = splitBy(hist[2]);
    if (!sel || !by.includes("le")) return null;
    return { ...query, ...sel, fn: "histogram_quantile", quantile: Number(hist[1]), range: hist[4], by: by.filter((l) => l !== "le") };
  }

  const agg = /^(sum|avg|max|min|count)(?: by \(([^)]*)\))? ?\((.*)\)$/.exec(text);
  if (agg) {
    query.aggregation = agg[1] as Aggregation;
    query.by = splitBy(agg[2]);
    text = agg[3];
  }

  const fn = /^(rate|irate|increase|avg_over_time|max_over_time|min_over_time)\((.+)\[([^\]]+)\]\)$/.exec(text);
  if (fn) {
    query.fn = fn[1] as RangeFunction;
    query.range = fn[3];
    text = fn[2];
  }

  const sel = parseSelector(text);
  if (!sel) return null;
  return { ...query, ...sel };
}
