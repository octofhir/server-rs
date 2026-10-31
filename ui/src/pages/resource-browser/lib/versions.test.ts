import { describe, expect, it } from "vitest";
import type { FhirBundle, FhirResource } from "@/shared/api/types";
import { changedPaths, toDiffText, toVersions } from "./versions";

describe("resource versions", () => {
  it("reads history entries newest first with their action", () => {
    const bundle = {
      resourceType: "Bundle",
      type: "history",
      entry: [
        { response: { status: "204", etag: 'W/"9"', lastModified: "2026-01-03" }, request: { method: "DELETE", url: "Patient/a" } },
        {
          resource: { resourceType: "Patient", id: "a", meta: { versionId: "5", lastUpdated: "2026-01-02" } },
          request: { method: "PUT", url: "Patient/a" },
        },
        {
          resource: { resourceType: "Patient", id: "a", meta: { versionId: "1", lastUpdated: "2026-01-01" } },
          request: { method: "POST", url: "Patient" },
        },
      ],
    } as FhirBundle;
    expect(toVersions(bundle).map((version) => [version.versionId, version.action, version.at])).toEqual([
      ["9", "deleted", "2026-01-03"],
      ["5", "updated", "2026-01-02"],
      ["1", "created", "2026-01-01"],
    ]);
  });

  it("lists changed paths at their shallowest point", () => {
    const before = { active: false, name: [{ family: "Doe" }], telecom: [{ value: "1" }] };
    const after = { active: true, name: [{ family: "Doe" }], telecom: [{ value: "2" }, { value: "3" }], gender: "f" };
    expect(changedPaths(before, after)).toEqual(["active", "telecom[0].value", "telecom[1]", "gender"]);
  });

  it("leaves version bookkeeping out of the diff text", () => {
    const text = toDiffText({
      resourceType: "Patient",
      id: "a",
      meta: { versionId: "5", lastUpdated: "x", tag: [{ code: "demo" }] },
    } as FhirResource);
    expect(text).not.toContain("versionId");
    expect(text).not.toContain("lastUpdated");
    expect(text).toContain("demo");
  });
});
