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
  /** Requests timed by the latency summary. */
  timed: number;
  seconds: number;
  /** Latency quantiles over the exporter's rolling window (about a minute), in seconds. */
  quantiles: Quantiles;
}

export interface MetricsSnapshot {
  at: number;
  routes: Record<string, RouteCounters>;
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
      quantiles: { p50: null, p95: null, p99: null },
    };
    return routes[key];
  };
  const snapshot: MetricsSnapshot = { at, routes, inFlight: 0, db: { active: 0, idle: 0, total: 0 } };

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
  return snapshot;
}
