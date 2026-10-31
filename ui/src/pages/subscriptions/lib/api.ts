import { fhirClient } from "@/shared/api/fhirClient";
import type { FhirResource } from "@/shared/api/types";

export interface SubscriptionResource extends FhirResource {
  status?: string;
  reason?: string;
  /** R4 Backport: the topic URL. */
  criteria?: string;
  /** R5: the topic URL. */
  topic?: string;
  error?: string;
  end?: string;
  channel?: { type?: string | { coding?: { code?: string }[] }; endpoint?: string };
  channelType?: { code?: string };
  endpoint?: string;
}

export interface TopicResource extends FhirResource {
  url?: string;
  title?: string;
  status?: string;
  resourceTrigger?: { resource?: string; supportedInteraction?: string[] }[];
}

export interface EventLogEntry {
  id: string;
  eventType: "handshake" | "heartbeat" | "event-notification";
  eventNumber: number;
  focusResourceType?: string | null;
  focusResourceId?: string | null;
  focusEvent?: string | null;
  status: "pending" | "processing" | "delivered" | "failed" | "expired";
  attempts: number;
  error?: string | null;
  httpStatus?: number | null;
  responseTimeMs?: number | null;
  createdAt: string;
  deliveredAt?: string | null;
  nextRetryAt?: string | null;
}

export interface EventLog {
  events: EventLogEntry[];
  eventsSinceSubscriptionStart: number;
  errorCount: number;
  lastErrorMessage?: string | null;
  lastErrorAt?: string | null;
  lastDeliveryAt?: string | null;
}

export function topicUrl(subscription: SubscriptionResource): string {
  return subscription.topic ?? subscription.criteria ?? "";
}

/** R4 `channel.type` is a code, R5 `channelType` a Coding, the backport a CodeableConcept. */
export function channelOf(subscription: SubscriptionResource): { type: string; endpoint?: string } {
  const raw = subscription.channel?.type;
  const type =
    subscription.channelType?.code ??
    (typeof raw === "string" ? raw : raw?.coding?.[0]?.code) ??
    "unknown";
  return { type, endpoint: subscription.endpoint ?? subscription.channel?.endpoint };
}

export async function listSubscriptions(): Promise<SubscriptionResource[]> {
  const bundle = await fhirClient.search<SubscriptionResource>("Subscription", { _count: 200, _sort: "-_lastUpdated" });
  return (bundle.entry ?? []).flatMap((entry) => (entry.resource ? [entry.resource] : []));
}

export async function listTopics(): Promise<TopicResource[]> {
  const bundle = await fhirClient.search<TopicResource>("SubscriptionTopic", { _count: 200 });
  return (bundle.entry ?? []).flatMap((entry) => (entry.resource ? [entry.resource] : []));
}

export async function fetchEventLog(id: string): Promise<EventLog> {
  const response = await fhirClient.rawRequest("GET", `/api/subscriptions/${encodeURIComponent(id)}/events?limit=100`);
  return response.data as EventLog;
}

export async function ping(id: string): Promise<void> {
  await fhirClient.rawRequest("POST", `/api/subscriptions/${encodeURIComponent(id)}/ping`);
}

/**
 * Turning a subscription off stops notifications; turning it back on asks the server to
 * activate it again (`requested`), which re-checks the topic and channel and re-sends the handshake.
 */
export async function setEnabled(subscription: SubscriptionResource, enabled: boolean): Promise<SubscriptionResource> {
  const { error: _error, ...rest } = subscription;
  return fhirClient.update<SubscriptionResource>({ ...rest, status: enabled ? "requested" : "off" });
}
