import { createEvent, createStore } from "effector";
import { type MetricsSnapshot, type Quantiles, type RouteCounters, STATUS_GROUPS, type StatusGroup } from "./prometheus";

export const POLL_INTERVAL_MS = 2000;

/** The longest window the page offers, plus one scrape so its first interval has a baseline. */
const MAX_SNAPSHOTS = (15 * 60 * 1000) / POLL_INTERVAL_MS + 1;

/** A pause between scrapes longer than this breaks the lines instead of bridging it. */
const GAP_MS = POLL_INTERVAL_MS * 3;

export const metricsScraped = createEvent<MetricsSnapshot>();

/** Bucket counts only grow with `timed`, so equal `timed` means equal buckets. */
function sameCounters(a: RouteCounters, b: RouteCounters): boolean {
  return (
    a.timed === b.timed &&
    a.seconds === b.seconds &&
    a.requests.ok === b.requests.ok &&
    a.requests.client === b.requests.client &&
    a.requests.server === b.requests.server &&
    a.quantiles.p50 === b.quantiles.p50 &&
    a.quantiles.p95 === b.quantiles.p95 &&
    a.quantiles.p99 === b.quantiles.p99
  );
}

/** Routes that did not change keep the previous scrape's object, so idle routes cost nothing per scrape. */
function shareUnchanged(previous: MetricsSnapshot | undefined, next: MetricsSnapshot): MetricsSnapshot {
  if (!previous) return next;
  for (const [key, route] of Object.entries(next.routes)) {
    const before = previous.routes[key];
    if (before && sameCounters(before, route)) next.routes[key] = before;
  }
  return next;
}

/**
 * Recent scrapes, oldest first. The store outlives the page, so leaving Performance and coming
 * back keeps the history; the time spent away shows as a gap.
 */
export const $snapshots = createStore<MetricsSnapshot[]>([]).on(metricsScraped, (list, snapshot) =>
  [...list, shareUnchanged(list.at(-1), snapshot)].slice(-MAX_SNAPSHOTS)
);

/** Counter increase between scrapes; a smaller value means the server restarted. */
function increase(current: number, previous: number): number {
  return current >= previous ? current - previous : current;
}

function total(requests: Record<StatusGroup, number>): number {
  return requests.ok + requests.client + requests.server;
}

const QUANTILES = { p50: 0.5, p95: 0.95, p99: 0.99 } as const;
const NO_QUANTILES: Quantiles = { p50: null, p95: null, p99: null };

/** Cumulative bucket counts added between two scrapes of one route. */
function bucketIncrease(current: number[], previous: number[] | undefined): number[] {
  return current.map((count, index) => increase(count, previous?.[index] ?? 0));
}

/**
 * Quantile of a cumulative histogram, interpolated linearly inside the bucket it falls in
 * (what Prometheus' histogram_quantile does). Values in the +Inf bucket report its lower bound.
 */
export function histogramQuantile(q: number, bounds: number[], cumulative: number[]): number | null {
  const count = cumulative.at(-1) ?? 0;
  if (count === 0) return null;
  const rank = q * count;
  const index = cumulative.findIndex((value) => value >= rank);
  if (index < 0) return null;
  const upper = bounds[index];
  const lower = index === 0 ? 0 : bounds[index - 1];
  if (!Number.isFinite(upper)) return lower;
  const below = index === 0 ? 0 : cumulative[index - 1];
  const inBucket = cumulative[index] - below;
  return inBucket === 0 ? upper : lower + (upper - lower) * ((rank - below) / inBucket);
}

/** p50/p95/p99 in ms of a cumulative histogram. */
function quantilesMs(bounds: number[], cumulative: number[]): Quantiles {
  if (cumulative.length === 0 || cumulative.length !== bounds.length) return NO_QUANTILES;
  const ms = (q: number) => {
    const seconds = histogramQuantile(q, bounds, cumulative);
    return seconds == null ? null : seconds * 1000;
  };
  return { p50: ms(QUANTILES.p50), p95: ms(QUANTILES.p95), p99: ms(QUANTILES.p99) };
}

function summaryMs(quantiles: Quantiles): Quantiles {
  const ms = (seconds: number | null) => (seconds == null ? null : seconds * 1000);
  return { p50: ms(quantiles.p50), p95: ms(quantiles.p95), p99: ms(quantiles.p99) };
}

export interface IntervalPoint {
  at: number;
  /** Requests per second by group; `null` marks a gap. */
  rate: Record<StatusGroup, number> | null;
  /** Mean latency over the interval in ms, `null` when nothing was served or for a gap. */
  meanMs: number | null;
  /** Latency quantiles of all requests in the interval, in ms (histogram servers only). */
  quantiles: Quantiles;
}

/** One point per scrape interval (all routes together). */
export function intervalSeries(snapshots: MetricsSnapshot[]): IntervalPoint[] {
  const points: IntervalPoint[] = [];
  for (let index = 1; index < snapshots.length; index++) {
    const previous = snapshots[index - 1];
    const current = snapshots[index];
    const elapsed = current.at - previous.at;
    if (elapsed > GAP_MS) {
      points.push({ at: previous.at + 1, rate: null, meanMs: null, quantiles: NO_QUANTILES });
      continue;
    }
    const rate: Record<StatusGroup, number> = { ok: 0, client: 0, server: 0 };
    let timed = 0;
    let seconds = 0;
    // Bucket bounds are the same for every route, so their increases add up.
    const buckets: number[] = current.bounds.map(() => 0);
    for (const [key, route] of Object.entries(current.routes)) {
      const before = previous.routes[key];
      for (const group of STATUS_GROUPS) {
        rate[group] += increase(route.requests[group], before?.requests[group] ?? 0);
      }
      timed += increase(route.timed, before?.timed ?? 0);
      seconds += Math.max(0, route.seconds - (before?.seconds ?? 0));
      bucketIncrease(route.buckets, before?.buckets).forEach((count, index) => {
        if (index < buckets.length) buckets[index] += count;
      });
    }
    for (const group of STATUS_GROUPS) rate[group] /= elapsed / 1000;
    points.push({
      at: current.at,
      rate,
      meanMs: timed > 0 ? (seconds / timed) * 1000 : null,
      quantiles: quantilesMs(current.bounds, buckets),
    });
  }
  return points;
}

export interface QuantilePoint extends Record<keyof Quantiles, number | null> {
  at: number;
}

/**
 * One route's latency quantiles per scrape interval, in ms: from the histogram increase over the
 * interval, or the exporter's rolling summary on servers without a histogram.
 */
export function routeQuantileSeries(snapshots: MetricsSnapshot[], key: string): QuantilePoint[] {
  const points: QuantilePoint[] = [];
  for (let index = 1; index < snapshots.length; index++) {
    const previous = snapshots[index - 1];
    const current = snapshots[index];
    if (current.at - previous.at > GAP_MS) {
      points.push({ at: previous.at + 1, ...NO_QUANTILES });
      continue;
    }
    const route = current.routes[key];
    const quantiles = !route
      ? NO_QUANTILES
      : route.buckets.length > 0
        ? quantilesMs(current.bounds, bucketIncrease(route.buckets, previous.routes[key]?.buckets))
        : summaryMs(route.quantiles);
    points.push({ at: current.at, ...quantiles });
  }
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
    const quantiles =
      route.buckets.length > 0
        ? quantilesMs(latest.bounds, bucketIncrease(route.buckets, start?.buckets))
        : summaryMs(route.quantiles);
    return {
      key,
      method: route.method,
      path: route.path,
      rate: seconds ? requests / seconds : null,
      requests,
      serverErrors: increase(route.requests.server, start?.requests.server ?? 0),
      clientErrors: increase(route.requests.client, start?.requests.client ?? 0),
      meanMs: timed > 0 ? (spent / timed) * 1000 : null,
      ...quantiles,
    };
  });
}
