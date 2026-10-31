import { describe, expect, it } from "vitest";
import { buildSubscription } from "./NewSubscriptionDialog";

const draft = {
  reason: "CRM sync",
  topic: "http://example.org/topics/patient-change",
  channel: "rest-hook" as const,
  endpoint: " https://crm.example.org/hook ",
  headers: "Authorization: Bearer abc\nnot a header",
  filters: {},
};

describe("buildSubscription", () => {
  it("uses the backport shape on R4", () => {
    expect(buildSubscription(draft, "R4")).toEqual({
      resourceType: "Subscription",
      status: "requested",
      reason: "CRM sync",
      criteria: "http://example.org/topics/patient-change",
      channel: {
        type: "rest-hook",
        endpoint: "https://crm.example.org/hook",
        payload: "application/fhir+json",
        header: ["Authorization: Bearer abc"],
      },
    });
  });

  it("keeps channel fields on the resource on R5", () => {
    const subscription = buildSubscription(draft, "R5") as unknown as Record<string, unknown>;
    expect(subscription.topic).toBe("http://example.org/topics/patient-change");
    expect(subscription.channelType).toEqual({ code: "rest-hook" });
    expect(subscription.parameter).toEqual([{ name: "Authorization", value: "Bearer abc" }]);
    expect(subscription.channel).toBeUndefined();
  });
});

describe("subscription filters", () => {
  const filtered = { ...draft, filters: { patient: "Patient/1", status: " " } };
  it("go on the topic URL in the backport", () => {
    expect(buildSubscription(filtered, "R4").criteria).toBe(
      "http://example.org/topics/patient-change?patient=Patient%2F1"
    );
  });
  it("go to filterBy in R5", () => {
    const subscription = buildSubscription(filtered, "R5") as unknown as Record<string, unknown>;
    expect(subscription.filterBy).toEqual([{ filterParameter: "patient", value: "Patient/1" }]);
  });
});

describe("buildTopic", () => {
  it("defaults the canonical URL to the title under this origin", async () => {
    const { buildTopic } = await import("./NewTopicDialog");
    const topic = buildTopic(
      {
        title: "Patient changes",
        url: "",
        resource: "Patient",
        interactions: ["create", "update"],
        criteria: "active = true",
        filters: [{ name: "org", expression: "managingOrganization.reference" }],
      },
      "https://fhir.example.org"
    );
    expect(topic.url).toBe("https://fhir.example.org/topics/patient-changes");
    expect(topic.resourceTrigger).toEqual([
      { resource: "Patient", supportedInteraction: ["create", "update"], fhirPathCriteria: "active = true" },
    ]);
    expect(topic.canFilterBy).toEqual([
      { resource: "Patient", filterParameter: "org", filterDefinition: "managingOrganization.reference" },
    ]);
  });
});
