import { createEvent, createStore } from "effector";
import { type MetricsSnapshot, type Quantiles, type RouteCounters, STATUS_GROUPS, type StatusGroup } from "./prometheus";

export const POLL_INTERVAL_MS = 2000;

/** The longest window the page offers, plus one scrape so its first interval has a baseline. */
const MAX_SNAPSHOTS = (15 * 60 * 1000) / POLL_INTERVAL_MS + 1;

/** A pause between scrapes longer than this breaks the lines instead of bridging it. */
const GAP_MS = POLL_INTERVAL_MS * 3;

export const metricsScraped = createEvent<MetricsSnapshot>();

/**
 * Recent scrapes, oldest first. The store outlives the page, so leaving Performance and coming
 * back keeps the history; the time spent away shows as a gap.
 */
export const $snapshots = createStore<MetricsSnapshot[]>([]).on(metricsScraped, (list, snapshot) =>
  [...list, snapshot].slice(-MAX_SNAPSHOTS)
);

/** Counter increase between scrapes; a smaller value means the server restarted. */
function increase(current: number, previous: number): number {
  return current >= previous ? current - previous : current;
}

function total(requests: Record<StatusGroup, number>): number {
  return requests.ok + requests.client + requests.server;
}

export interface IntervalPoint {
  at: number;
  /** Requests per second by group; `null` marks a gap. */
  rate: Record<StatusGroup, number> | null;
  /** Mean latency over the interval in ms, `null` when nothing was served or for a gap. */
  meanMs: number | null;
}

/** One point per scrape interval (all routes together). */
export function intervalSeries(snapshots: MetricsSnapshot[]): IntervalPoint[] {
  const points: IntervalPoint[] = [];
  for (let index = 1; index < snapshots.length; index++) {
    const previous = snapshots[index - 1];
    const current = snapshots[index];
    const elapsed = current.at - previous.at;
    if (elapsed > GAP_MS) {
      points.push({ at: previous.at + 1, rate: null, meanMs: null });
      continue;
    }
    const rate: Record<StatusGroup, number> = { ok: 0, client: 0, server: 0 };
    let timed = 0;
    let seconds = 0;
    for (const [key, route] of Object.entries(current.routes)) {
      const before = previous.routes[key];
      for (const group of STATUS_GROUPS) {
        rate[group] += increase(route.requests[group], before?.requests[group] ?? 0);
      }
      timed += increase(route.timed, before?.timed ?? 0);
      seconds += Math.max(0, route.seconds - (before?.seconds ?? 0));
    }
    for (const group of STATUS_GROUPS) rate[group] /= elapsed / 1000;
    points.push({ at: current.at, rate, meanMs: timed > 0 ? (seconds / timed) * 1000 : null });
  }
  return points;
}

export interface QuantilePoint extends Record<keyof Quantiles, number | null> {
  at: number;
}

/** A route's rolling quantiles at every scrape, in ms. */
export function routeQuantileSeries(snapshots: MetricsSnapshot[], key: string): QuantilePoint[] {
  const ms = (seconds: number | null | undefined) => (seconds == null ? null : seconds * 1000);
  const points: QuantilePoint[] = [];
  snapshots.forEach((snapshot, index) => {
    const previous = snapshots[index - 1];
    if (previous && snapshot.at - previous.at > GAP_MS) {
      points.push({ at: previous.at + 1, p50: null, p95: null, p99: null });
    }
    const quantiles = snapshot.routes[key]?.quantiles;
    points.push({ at: snapshot.at, p50: ms(quantiles?.p50), p95: ms(quantiles?.p95), p99: ms(quantiles?.p99) });
  });
  return points;
}

export interface RouteStats {
  key: string;
  method: string;
  path: string;
  /** Requests per second over the window; `null` until two scrapes exist. */
  rate: number | null;
  /** Requests in the window (since server start until two scrapes exist). */
  requests: number;
  serverErrors: number;
  clientErrors: number;
  meanMs: number | null;
  p50: number | null;
  p95: number | null;
  p99: number | null;
}

/**
 * Per-route activity between the first scrape at or after `since` and the latest one.
 * With a single scrape the counters since server start are shown, without a rate.
 */
export function routeStats(snapshots: MetricsSnapshot[], since: number): RouteStats[] {
  const latest = snapshots.at(-1);
  if (!latest) return [];
  const baseline = snapshots.find((snapshot) => snapshot.at >= since && snapshot !== latest);
  const seconds = baseline ? (latest.at - baseline.at) / 1000 : null;
  const before = (key: string): RouteCounters | undefined => baseline?.routes[key];

  return Object.entries(latest.routes).map(([key, route]) => {
    const start = before(key);
    const requests = increase(total(route.requests), start ? total(start.requests) : 0);
    const timed = increase(route.timed, start?.timed ?? 0);
    const spent = Math.max(0, route.seconds - (start?.seconds ?? 0));
    const ms = (value: number | null) => (value == null ? null : value * 1000);
    return {
      key,
      method: route.method,
      path: route.path,
      rate: seconds ? requests / seconds : null,
      requests,
      serverErrors: increase(route.requests.server, start?.requests.server ?? 0),
      clientErrors: increase(route.requests.client, start?.requests.client ?? 0),
      meanMs: timed > 0 ? (spent / timed) * 1000 : null,
      p50: ms(route.quantiles.p50),
      p95: ms(route.quantiles.p95),
      p99: ms(route.quantiles.p99),
    };
  });
}
