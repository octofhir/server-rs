import { describe, expect, it, vi } from "vitest";

vi.mock("@/shared/monaco/config", () => ({ monaco: {} }));

const { completionContext } = await import("./language");

describe("PromQL completion context", () => {
  it("offers metrics and functions at an expression", () => {
    expect(completionContext("sum(rat")).toEqual({ kind: "expression", prefix: "rat" });
  });

  it("offers label names inside a selector", () => {
    expect(completionContext("octofhir_http_requests_total{sta")).toEqual({
      kind: "labelName",
      metric: "octofhir_http_requests_total",
      prefix: "sta",
    });
  });

  it("offers label values after a matcher operator", () => {
    expect(completionContext('up{instance=~"pod')).toMatchObject({
      kind: "labelValue",
      metric: "up",
      label: "instance",
      prefix: "pod",
      quoted: true,
    });
    expect(completionContext("up{job=")).toMatchObject({ kind: "labelValue", label: "job", quoted: false });
  });

  it("offers durations inside a range and labels inside by (…)", () => {
    expect(completionContext("rate(x[")).toEqual({ kind: "duration", prefix: "" });
    expect(completionContext("sum by (sta")).toEqual({ kind: "grouping", prefix: "sta" });
  });

  it("closes selectors and ranges", () => {
    expect(completionContext('rate(x{a="b"}[5m]) + su')).toEqual({ kind: "expression", prefix: "su" });
  });
});
