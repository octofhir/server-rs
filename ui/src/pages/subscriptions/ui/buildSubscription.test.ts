import { describe, expect, it } from "vitest";
import { buildSubscription } from "./NewSubscriptionDialog";

const draft = {
  reason: "CRM sync",
  topic: "http://example.org/topics/patient-change",
  channel: "rest-hook" as const,
  endpoint: " https://crm.example.org/hook ",
  headers: "Authorization: Bearer abc\nnot a header",
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
