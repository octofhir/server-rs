import { describe, expect, it } from "vitest";
import { groupIssues } from "./run";

describe("validation issue groups", () => {
  it("groups by message and path shape, errors first, most affected first", () => {
    const groups = groupIssues([
      {
        id: "a",
        issues: [
          { severity: "warning", message: "No narrative", expression: "Patient.text" },
          { severity: "error", message: "Bad date", expression: "Patient.name[0].period.start" },
        ],
      },
      { id: "b", issues: [{ severity: "error", message: "Bad date", expression: "Patient.name[3].period.start" }] },
      { id: "c", issues: [{ severity: "warning", message: "No narrative", expression: "Patient.text" }] },
      { id: "d", issues: [] },
    ]);
    expect(groups.map((group) => [group.severity, group.expression, group.resourceIds])).toEqual([
      ["error", "Patient.name[n].period.start", ["a", "b"]],
      ["warning", "Patient.text", ["a", "c"]],
    ]);
  });
});
