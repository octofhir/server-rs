import { describe, expect, it } from "vitest";
import { sameOriginPath } from "./fhirClient";

describe("sameOriginPath", () => {
  it("keeps only path and query of a server-built link", () => {
    expect(sameOriginPath("http://0.0.0.0:8888/fhir/Observation?_count=50&_offset=50")).toBe(
      "/fhir/Observation?_count=50&_offset=50"
    );
  });

  it("leaves relative paths alone", () => {
    expect(sameOriginPath("/fhir/_bulk-files/job/Patient.ndjson")).toBe("/fhir/_bulk-files/job/Patient.ndjson");
  });
});
