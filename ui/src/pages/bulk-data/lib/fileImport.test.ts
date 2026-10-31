import { describe, expect, it } from "vitest";
import { batchEntry, lines } from "./fileImport";

describe("file import", () => {
  it("keeps ids with PUT and creates the rest", () => {
    expect(batchEntry({ resourceType: "Patient", id: "p1" }).request).toEqual({ method: "PUT", url: "Patient/p1" });
    expect(batchEntry({ resourceType: "Patient" }).request).toEqual({ method: "POST", url: "Patient" });
  });

  it("splits a stream into lines across chunk boundaries", async () => {
    const file = new Blob(['{"a":1}\n{"b"', ':2}\n', "{\"c\":3}"]);
    const out: string[] = [];
    for await (const line of lines(file)) out.push(line);
    expect(out).toEqual(['{"a":1}', '{"b":2}', '{"c":3}']);
  });
});
