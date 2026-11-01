import { describe, expect, it } from "vitest";
import {
  ALL_VALUE,
  compact,
  joinTable,
  type Panel,
  rangeSeconds,
  seriesName,
  resolveWindow,
  thresholdColor,
  variableValues,
  visibleLayout,
} from "./dashboard";
import { formatValue } from "./format";

function panel(id: string, x: number, y: number, w: number, h: number): Panel {
  return { id, title: id, type: "timeseries", gridPos: { x, y, w, h }, targets: [{ refId: "A", expr: "up" }] };
}

describe("dashboard model", () => {
  it("reads Grafana relative ranges", () => {
    expect(rangeSeconds("now-15m")).toBe(900);
    expect(rangeSeconds("now-7d")).toBe(604_800);
    expect(rangeSeconds("yesterday")).toBe(3600);
  });

  it("names series from legend templates", () => {
    expect(seriesName("{{status_class}} on {{instance}}", { status_class: "5xx", instance: "a" })).toBe("5xx on a");
    expect(seriesName(undefined, { __name__: "up", job: "x" })).toBe('job="x"');
    expect(seriesName(undefined, {})).toBe("value");
  });

  it("turns variable selections into regexes", () => {
    const variables = [{ name: "instance", labelName: "instance", multi: true, includeAll: true }];
    expect(variableValues(variables, {})).toEqual({ instance: ".*" });
    expect(variableValues(variables, { instance: [ALL_VALUE] })).toEqual({ instance: ".*" });
    expect(variableValues(variables, { instance: ["a.local:8888", "b"] })).toEqual({ instance: "a\\.local:8888|b" });
  });

  it("joins table queries on their labels", () => {
    const { labelColumns, rows } = joinTable([
      { refId: "A", samples: [{ labels: { path: "/a", method: "GET" }, value: 1 }] },
      {
        refId: "B",
        samples: [
          { labels: { method: "GET", path: "/a" }, value: 2 },
          { labels: { path: "/b", method: "GET" }, value: 3 },
        ],
      },
    ]);
    expect(labelColumns).toEqual(["method", "path"]);
    expect(rows).toHaveLength(2);
    expect(rows[0].values).toEqual({ A: 1, B: 2 });
  });

  it("pushes panels out of a moved panel's way and floats the rest up", () => {
    const moved = compact([panel("a", 0, 0, 12, 4), panel("b", 0, 2, 12, 4), panel("c", 12, 9, 12, 4)], "b");
    const byId = Object.fromEntries(moved.map((p) => [p.id, p.gridPos]));
    expect(byId.b.y).toBe(2);
    expect(byId.a.y).toBe(6);
    expect(byId.c.y).toBe(0);
  });

  it("picks the highest threshold reached", () => {
    const thresholds = [
      { value: 0.05, color: "error" as const },
      { value: 0.01, color: "warning" as const },
    ];
    expect(thresholdColor(thresholds, 0.001)).toBeNull();
    expect(thresholdColor(thresholds, 0.02)).toBe("warning");
    expect(thresholdColor(thresholds, 0.5)).toBe("error");
  });

  it("formats Grafana units", () => {
    expect(formatValue(0.0123, "s")).toBe("12.3 ms");
    expect(formatValue(0.0042, "percentunit")).toBe("0.42%");
    expect(formatValue(1520, "reqps")).toBe("1,520 req/s");
    expect(formatValue(15_200, "reqps")).toBe("15.2 K req/s");
    expect(formatValue(null, "s")).toBe("—");
  });
});

describe("layout and time", () => {
  const row = (id: string, y: number): Panel => ({ id, title: id, type: "row", gridPos: { x: 0, y, w: 24, h: 1 }, targets: [] });

  it("hides a collapsed row's panels and moves the rest up", () => {
    const panels = [row("r1", 0), panel("a", 0, 1, 12, 8), row("r2", 9), panel("b", 0, 10, 12, 8)];
    const shown = visibleLayout(panels, new Set(["r1"]));
    expect(shown.map((p) => p.id)).toEqual(["r1", "r2", "b"]);
    expect(shown.find((p) => p.id === "r2")?.gridPos.y).toBe(1);
    expect(shown.find((p) => p.id === "b")?.gridPos.y).toBe(2);
    expect(visibleLayout(panels, new Set(["r2"])).map((p) => p.id)).toEqual(["r1", "a", "r2"]);
  });

  it("resolves relative and absolute windows", () => {
    expect(resolveWindow("now-6h", null)).toEqual({ range: 21_600, end: null });
    expect(resolveWindow("1700000000000", "1700000600000")).toEqual({ range: 600, end: 1_700_000_600 });
    expect(resolveWindow("1700000600000", "1700000000000")).toEqual({ range: 3600, end: null });
  });
});
