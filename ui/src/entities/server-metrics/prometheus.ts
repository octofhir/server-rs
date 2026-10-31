/** Response groups shown on the throughput chart; 3xx counts as OK. */
export type StatusGroup = "ok" | "client" | "server";

export const STATUS_GROUPS: readonly StatusGroup[] = ["ok", "client", "server"];

export interface Quantiles {
  p50: number | null;
  p95: number | null;
  p99: number | null;
}

/** Cumulative counters for one `METHOD path` route, as of one scrape. */
export interface RouteCounters {
  method: string;
  path: string;
  requests: Record<StatusGroup, number>;
  /** Requests timed by the latency histogram. */
  timed: number;
  seconds: number;
  /** Cumulative latency histogram counts, aligned with {@link MetricsSnapshot.bounds}. */
  buckets: number[];
  /**
   * Quantiles from a latency summary, in seconds. Older servers export a summary instead of a
   * histogram; its quantiles cover the exporter's rolling window (about a minute).
   */
  quantiles: Quantiles;
}

export interface MetricsSnapshot {
  at: number;
  routes: Record<string, RouteCounters>;
  /** Upper bounds (seconds) of the latency histogram buckets, ascending, ending with Infinity. */
  bounds: number[];
  inFlight: number;
  db: { active: number; idle: number; total: number };
}

interface Sample {
  name: string;
  labels: Record<string, string>;
  value: number;
}

const SAMPLE_LINE = /^([A-Za-z_:][\w:]*)(?:\{(.*)\})?\s+(\S+)/;
const LABEL = /(\w+)="((?:[^"\\]|\\.)*)"/g;

function parseSamples(text: string): Sample[] {
  const samples: Sample[] = [];
  for (const line of text.split("\n")) {
    if (!line || line.startsWith("#")) continue;
    const match = SAMPLE_LINE.exec(line);
    if (!match) continue;
    const labels: Record<string, string> = {};
    for (const [, key, value] of (match[2] ?? "").matchAll(LABEL)) {
      labels[key] = value.replace(/\\(.)/g, "$1");
    }
    samples.push({ name: match[1], labels, value: Number(match[3]) });
  }
  return samples;
}

function statusGroup(statusClass: string | undefined): StatusGroup {
  if (statusClass === "5xx") return "server";
  if (statusClass === "4xx") return "client";
  return "ok";
}

const QUANTILE_KEYS: Record<string, keyof Quantiles> = { "0.5": "p50", "0.95": "p95", "0.99": "p99" };

/** Reads one `/metrics` scrape into per-route counters and the connection gauges. */
export function toSnapshot(text: string, at: number): MetricsSnapshot {
  const routes: Record<string, RouteCounters> = {};
  const route = (labels: Record<string, string>): RouteCounters => {
    const method = labels.method ?? "GET";
    const path = labels.path ?? "/";
    const key = `${method} ${path}`;
    routes[key] ??= {
      method,
      path,
      requests: { ok: 0, client: 0, server: 0 },
      timed: 0,
      seconds: 0,
      buckets: [],
      quantiles: { p50: null, p95: null, p99: null },
    };
    return routes[key];
  };
  const snapshot: MetricsSnapshot = { at, routes, bounds: [], inFlight: 0, db: { active: 0, idle: 0, total: 0 } };
  /** Per route: upper bound -> cumulative count, in exposition order. */
  const histograms = new Map<RouteCounters, Map<number, number>>();

  for (const { name, labels, value } of parseSamples(text)) {
    switch (name) {
      case "http_requests_total":
        route(labels).requests[statusGroup(labels.status_class)] += value;
        break;
      case "http_request_duration_seconds": {
        const key = QUANTILE_KEYS[labels.quantile ?? ""];
        // The exporter reports 0 once its window has no samples; that is "no data", not 0 ms.
        if (key) route(labels).quantiles[key] = Number.isFinite(value) && value > 0 ? value : null;
        break;
      }
      case "http_request_duration_seconds_bucket": {
        const bound = labels.le === "+Inf" ? Number.POSITIVE_INFINITY : Number(labels.le);
        if (Number.isNaN(bound)) break;
        const counters = route(labels);
        const histogram = histograms.get(counters) ?? new Map<number, number>();
        histogram.set(bound, value);
        histograms.set(counters, histogram);
        break;
      }
      case "http_request_duration_seconds_count":
        route(labels).timed = value;
        break;
      case "http_request_duration_seconds_sum":
        route(labels).seconds = value;
        break;
      case "http_active_connections":
        snapshot.inFlight = value;
        break;
      case "db_pool_connections_active":
        snapshot.db.active = value;
        break;
      case "db_pool_connections_idle":
        snapshot.db.idle = value;
        break;
      case "db_pool_connections_total":
        snapshot.db.total = value;
        break;
    }
  }
  const bounds = [...new Set([...histograms.values()].flatMap((histogram) => [...histogram.keys()]))].sort(
    (a, b) => a - b
  );
  snapshot.bounds = bounds;
  for (const [counters, histogram] of histograms) {
    counters.buckets = bounds.map((bound) => histogram.get(bound) ?? 0);
  }
  return snapshot;
}
