import { describe, expect, it } from "vitest";
import { closestPages, type PageRef, resourceAddress } from "./suggest";

const PAGES: PageRef[] = [
  { label: "Subscriptions", section: "System", path: "/subscriptions", aliases: [] },
  { label: "Users", section: "Access", path: "/auth/users", aliases: ["/users"] },
  { label: "Roles", section: "Access", path: "/auth/roles", aliases: [] },
  { label: "Performance", section: "System", path: "/performance", aliases: [] },
  { label: "SQL", section: "Query", path: "/db-console", aliases: [] },
];

describe("resourceAddress", () => {
  it("maps FHIR REST addresses to the resource browser", () => {
    expect(resourceAddress("/Patient/123")).toBe("/resources/Patient/123");
    expect(resourceAddress("/fhir/Observation")).toBe("/resources/Observation");
    expect(resourceAddress("/fhir/Patient/p-1/_history/2")).toBe("/resources/Patient/p-1");
  });

  it("ignores console paths", () => {
    expect(resourceAddress("/settingz")).toBeNull();
    expect(resourceAddress("/Patient/123/everything")).toBeNull();
    expect(resourceAddress("/")).toBeNull();
  });
});

describe("closestPages", () => {
  const paths = (path: string) => closestPages(path, PAGES).map((page) => page.path);

  it("catches typos", () => {
    expect(paths("/subscription")).toEqual(["/subscriptions"]);
    expect(paths("/perfomance")).toEqual(["/performance"]);
  });

  it("follows aliases and deeper paths", () => {
    expect(paths("/users/42")).toEqual(["/auth/users"]);
    expect(paths("/subscriptions/abc/def")).toEqual(["/subscriptions"]);
  });

  it("lists a section's pages for its prefix", () => {
    expect(paths("/auth")).toEqual(["/auth/roles", "/auth/users"]);
  });

  it("matches page names, not only addresses", () => {
    expect(paths("/sql")).toEqual(["/db-console"]);
  });

  it("stays quiet for unrelated paths", () => {
    expect(paths("/zzzzzzzz")).toEqual([]);
  });
});
