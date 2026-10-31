import { describe, expect, it } from "vitest";
import { intervalSeries, routeStats } from "./model";
import { toSnapshot } from "./prometheus";

function scrape(at: number, ok: number, errors: number, seconds: number, p99 = 0.004) {
  return toSnapshot(
    [
      "# TYPE http_requests_total counter",
      `http_requests_total{method="GET",path="/fhir/Patient",status_class="2xx"} ${ok}`,
      `http_requests_total{method="GET",path="/fhir/Patient",status_class="5xx"} ${errors}`,
      `http_request_duration_seconds{method="GET",path="/fhir/Patient",quantile="0.5"} 0.002`,
      `http_request_duration_seconds{method="GET",path="/fhir/Patient",quantile="0.99"} ${p99}`,
      `http_request_duration_seconds_sum{method="GET",path="/fhir/Patient"} ${seconds}`,
      `http_request_duration_seconds_count{method="GET",path="/fhir/Patient"} ${ok + errors}`,
      "http_active_connections 3",
      "db_pool_connections_active 2",
      "db_pool_connections_total 50",
    ].join("\n"),
    at
  );
}

describe("performance metrics", () => {
  it("reads routes, quantiles and gauges from a scrape", () => {
    const snapshot = scrape(0, 10, 1, 0.05, 0);
    const route = snapshot.routes["GET /fhir/Patient"];
    expect(route.requests).toEqual({ ok: 10, client: 0, server: 1 });
    expect(route.quantiles).toEqual({ p50: 0.002, p95: null, p99: null });
    expect(snapshot.inFlight).toBe(3);
    expect(snapshot.db).toEqual({ active: 2, idle: 0, total: 50 });
  });

  it("turns counter increases into rates and interval means", () => {
    const points = intervalSeries([scrape(0, 10, 0, 0.1), scrape(2000, 30, 2, 0.32)]);
    expect(points).toHaveLength(1);
    expect(points[0].rate).toEqual({ ok: 10, client: 0, server: 1 });
    expect(points[0].meanMs).toBeCloseTo(10);
  });

  it("breaks the series across a long pause and survives a restart", () => {
    const points = intervalSeries([scrape(0, 10, 0, 0.1), scrape(60_000, 20, 0, 0.2), scrape(62_000, 4, 0, 0.04)]);
    expect(points[0].rate).toBeNull();
    expect(points[1].rate?.ok).toBe(2);
  });

  it("summarises a route over the window", () => {
    const [stats] = routeStats([scrape(0, 10, 0, 0.1), scrape(4000, 50, 4, 0.54)], 0);
    expect(stats.rate).toBe(11);
    expect(stats.requests).toBe(44);
    expect(stats.serverErrors).toBe(4);
    expect(stats.meanMs).toBeCloseTo(10);
    expect(stats.p99).toBeCloseTo(4);
  });

  it("falls back to counters since start with a single scrape", () => {
    const [stats] = routeStats([scrape(0, 10, 0, 0.1)], 0);
    expect(stats.rate).toBeNull();
    expect(stats.requests).toBe(10);
  });
});
