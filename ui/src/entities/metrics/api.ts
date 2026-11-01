import { fhirClient } from "@/shared/api";
import { HttpError } from "@/shared/api/fhirClient";
import type { Dashboard } from "./dashboard";

/**
 * The server's metrics API. Every query goes to Prometheus through the server, which scopes it
 * to this deployment's series; the console never reads `/metrics` itself (behind a load balancer
 * that would be a random instance each time).
 */

export interface MetricsTarget {
  instance: string;
  up: boolean;
  version: string | null;
}

export interface MetricsStatus {
  configured: boolean;
  selector: string | null;
  scrapeIntervalSecs: number | null;
  reachable: boolean;
  error: string | null;
  targets: MetricsTarget[];
}

export interface MetricInfo {
  name: string;
  type: string | null;
  help: string | null;
  unit: string | null;
}

/** One series of a range query; timestamps in ms, gaps as `null`. */
export interface Series {
  labels: Record<string, string>;
  points: [number, number | null][];
}

export interface RangeResult {
  window: { start: number; end: number; step: number };
  series: Series[];
}

export interface InstantSample {
  labels: Record<string, string>;
  value: number | null;
}

export interface ParseResult {
  ok: boolean;
  error: { message: string; line: number | null; column: number | null } | null;
}

export type Variables = Record<string, string>;

export interface GrafanaTarget {
  url: string;
  token: string;
}

export interface GrafanaDatasource {
  uid: string;
  name: string;
  isDefault: boolean;
}

export interface GrafanaFolder {
  uid: string;
  title: string;
}

export interface PushedDashboard {
  title: string;
  url: string;
}

/** An error the metrics API reported, with its kind (`bad_query`, `unavailable`, `timeout`, …). */
export class MetricsApiError extends Error {
  readonly kind: string;
  readonly status: number;

  constructor(message: string, kind: string, status: number) {
    super(message);
    this.name = "MetricsApiError";
    this.kind = kind;
    this.status = status;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

async function call<T>(method: "GET" | "POST", path: string, body?: unknown): Promise<T> {
  try {
    const response = await fhirClient.rawRequest(method, path, body, { timeout: 30_000 });
    return response.data as T;
  } catch (error) {
    if (error instanceof HttpError) {
      const data = error.response.data;
      const detail = isRecord(data) && isRecord(data.error) ? data.error : null;
      const message = typeof detail?.message === "string" ? detail.message : error.message;
      const kind = typeof detail?.kind === "string" ? detail.kind : "http";
      throw new MetricsApiError(message, kind, error.response.status);
    }
    throw error;
  }
}

function sampleValue(raw: unknown): number | null {
  if (!Array.isArray(raw) || typeof raw[1] !== "string") return null;
  const value = Number(raw[1]);
  return Number.isFinite(value) ? value : null;
}

interface RawSample {
  metric?: Record<string, string>;
  value?: unknown;
  values?: unknown[];
}

interface RawData {
  resultType?: string;
  result?: unknown;
}

/** Prometheus matrix → series, with `null` wherever a step has no sample. */
function toSeries(data: RawData, window: RangeResult["window"]): Series[] {
  if (data.resultType !== "matrix" || !Array.isArray(data.result)) return [];
  const steps = Math.round((window.end - window.start) / window.step);
  return (data.result as RawSample[]).map((sample) => {
    const byTime = new Map<number, number | null>();
    for (const raw of sample.values ?? []) {
      if (Array.isArray(raw) && typeof raw[0] === "number") {
        byTime.set(Math.round(raw[0] / window.step), sampleValue(raw));
      }
    }
    const first = Math.round(window.start / window.step);
    const points: [number, number | null][] = [];
    for (let index = 0; index <= steps; index++) {
      const slot = first + index;
      points.push([slot * window.step * 1000, byTime.get(slot) ?? null]);
    }
    return { labels: sample.metric ?? {}, points };
  });
}

function toSamples(data: RawData): InstantSample[] {
  if (data.resultType === "scalar") return [{ labels: {}, value: sampleValue(data.result) }];
  if (data.resultType !== "vector" || !Array.isArray(data.result)) return [];
  return (data.result as RawSample[]).map((sample) => ({
    labels: sample.metric ?? {},
    value: sampleValue(sample.value),
  }));
}

export const metricsApi = {
  status: () => call<MetricsStatus>("GET", "/api/metrics/status"),

  async queryRange(request: {
    query: string;
    start: number;
    end: number;
    step?: number;
    variables?: Variables;
  }): Promise<RangeResult> {
    const response = await call<{ window: RangeResult["window"]; data: RawData }>(
      "POST",
      "/api/metrics/query_range",
      request
    );
    return { window: response.window, series: toSeries(response.data, response.window) };
  },

  async query(request: {
    query: string;
    time?: number;
    range?: number;
    variables?: Variables;
  }): Promise<InstantSample[]> {
    const response = await call<{ data: RawData }>("POST", "/api/metrics/query", request);
    return toSamples(response.data);
  },

  metrics: () => call<MetricInfo[]>("GET", "/api/metrics/metrics"),

  labels: (metric?: string) =>
    call<string[]>("GET", `/api/metrics/labels${metric ? `?${new URLSearchParams({ metric })}` : ""}`),

  labelValues: (label: string, metric?: string) =>
    call<string[]>(
      "GET",
      `/api/metrics/label/${encodeURIComponent(label)}/values${metric ? `?${new URLSearchParams({ metric })}` : ""}`
    ),

  parse: (query: string) => call<ParseResult>("POST", "/api/metrics/parse", { query }),

  builtinDashboards: () => call<Dashboard[]>("GET", "/api/dashboards/builtin"),

  exportGrafana: (dashboard: Dashboard) =>
    call<Record<string, unknown>>("POST", "/api/dashboards/grafana/export", dashboard),

  grafanaDatasources: (target: GrafanaTarget) =>
    call<GrafanaDatasource[]>("POST", "/api/grafana/datasources", target),

  grafanaFolders: (target: GrafanaTarget) => call<GrafanaFolder[]>("POST", "/api/grafana/folders", target),

  grafanaPush: (request: GrafanaTarget & { datasourceUid: string; folderUid?: string; dashboards: Dashboard[] }) =>
    call<PushedDashboard[]>("POST", "/api/grafana/push", request),
};
