import { describe, expect, it } from "vitest";
import { type BuilderQuery, builderFor, emptyBuilder, fromPromql, toPromql } from "./builder";

const q = (patch: Partial<BuilderQuery>): BuilderQuery => ({ ...emptyBuilder("octofhir_http_requests_total"), ...patch });

describe("query builder", () => {
  it("renders selectors, functions and aggregations", () => {
    expect(toPromql(q({}))).toBe("octofhir_http_requests_total");
    expect(
      toPromql(
        q({
          filters: [
            { label: "status_class", op: "=", value: "5xx" },
            { label: "instance", op: "=~", value: "$instance" },
          ],
          fn: "rate",
          aggregation: "sum",
          by: ["path"],
        })
      )
    ).toBe('sum by (path) (rate(octofhir_http_requests_total{status_class="5xx", instance=~"$instance"}[$__rate_interval]))');
    expect(toPromql(q({ fn: "increase", range: "$__range", aggregation: "sum", topk: 5 }))).toBe(
      "topk(5, sum(increase(octofhir_http_requests_total[$__range])))"
    );
  });

  it("renders histogram quantiles grouped by le", () => {
    const query = builderFor("octofhir_http_request_duration_seconds", "histogram");
    expect(toPromql({ ...query, by: ["path"] })).toBe(
      "histogram_quantile(0.95, sum by (le, path) (rate(octofhir_http_request_duration_seconds_bucket[$__rate_interval])))"
    );
  });

  it("escapes values", () => {
    expect(toPromql(q({ filters: [{ label: "path", op: "=", value: 'a"b\\c' }] }))).toBe(
      'octofhir_http_requests_total{path="a\\"b\\\\c"}'
    );
  });

  it("reads back what it writes", () => {
    const cases: Partial<BuilderQuery>[] = [
      {},
      { filters: [{ label: "path", op: "!~", value: "/api/.*" }], fn: "rate", aggregation: "sum", by: ["method", "path"] },
      { fn: "max_over_time", range: "5m", aggregation: "max" },
      { fn: "increase", range: "$__range", aggregation: "sum", topk: 10 },
      { metric: "x_bucket", fn: "histogram_quantile", quantile: 0.99, by: ["instance"] },
      { filters: [{ label: "a", op: "=", value: 'with "quotes", and commas' }] },
    ];
    for (const patch of cases) {
      const written = toPromql(q(patch));
      const read = fromPromql(written);
      expect(read, written).not.toBeNull();
      expect(toPromql(read as BuilderQuery)).toBe(written);
    }
  });

  it("leaves other queries to the code editor", () => {
    expect(fromPromql("sum(rate(a[5m])) / sum(rate(b[5m]))")).toBeNull();
    expect(fromPromql('label_replace(up, "x", "$1", "y", "(.*)")')).toBeNull();
  });

  it("starts counters with a rate", () => {
    expect(builderFor("octofhir_http_requests_total", "counter")).toMatchObject({ fn: "rate", aggregation: "sum" });
    expect(builderFor("octofhir_process_threads", "gauge").fn).toBe("none");
  });
});
