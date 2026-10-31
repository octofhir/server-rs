import { Button, Combobox, Modal, notify, SegmentedRadioGroup, TextArea, TextInput } from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { fhirClient } from "@/shared/api";
import { useSettings } from "@/shared/api/hooks";
import type { SubscriptionResource, TopicResource } from "../lib/api";
import classes from "./SubscriptionsPage.module.css";

type Channel = "rest-hook" | "websocket" | "email";

interface Draft {
  reason: string;
  topic: string | null;
  channel: Channel;
  /** REST hook URL, or the recipient's address for email. */
  endpoint: string;
  /** `Name: value` per line. */
  headers: string;
}

const EMPTY: Draft = { reason: "", topic: null, channel: "rest-hook", endpoint: "", headers: "" };

function headerLines(text: string): [string, string][] {
  return text.split("\n").flatMap((line): [string, string][] => {
    const at = line.indexOf(":");
    if (at <= 0) return [];
    return [[line.slice(0, at).trim(), line.slice(at + 1).trim()]];
  });
}

/**
 * The Subscription to create, in the server's FHIR version: R5 keeps channel fields on the
 * resource, R4/R4B use the Subscriptions Backport shape (`criteria` + `channel`).
 */
export function buildSubscription(draft: Draft, fhirVersion: string | undefined): SubscriptionResource {
  const headers = headerLines(draft.headers);
  const endpoint =
    draft.channel === "rest-hook"
      ? draft.endpoint.trim()
      : draft.channel === "email"
        ? `mailto:${draft.endpoint.trim()}`
        : undefined;
  if (/^R[5-9]/i.test(fhirVersion ?? "")) {
    return {
      resourceType: "Subscription",
      status: "requested",
      reason: draft.reason.trim() || undefined,
      topic: draft.topic ?? "",
      channelType: { code: draft.channel },
      endpoint,
      contentType: "application/fhir+json",
      content: "full-resource",
      ...(headers.length > 0 ? { parameter: headers.map(([name, value]) => ({ name, value })) } : {}),
    } as SubscriptionResource;
  }
  return {
    resourceType: "Subscription",
    status: "requested",
    reason: draft.reason.trim() || "Subscription",
    criteria: draft.topic ?? "",
    channel: {
      type: draft.channel,
      endpoint,
      ...(draft.channel === "rest-hook" ? { payload: "application/fhir+json" } : {}),
      ...(headers.length > 0 ? { header: headers.map(([name, value]) => `${name}: ${value}`) } : {}),
    },
  } as SubscriptionResource;
}

export function NewSubscriptionDialog({
  open,
  topics,
  onClose,
  onCreated,
}: {
  open: boolean;
  topics: TopicResource[];
  onClose: () => void;
  onCreated: (subscription: SubscriptionResource) => void;
}) {
  const { data: settings } = useSettings();
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const update = (patch: Partial<Draft>) => setDraft((current) => ({ ...current, ...patch }));
  const create = useMutation({
    mutationFn: () => fhirClient.create<SubscriptionResource>(buildSubscription(draft, settings?.fhirVersion)),
    onSuccess: (created) => {
      setDraft(EMPTY);
      onCreated(created);
    },
    onError: (error) =>
      notify({
        theme: "danger",
        title: "Subscription not created",
        content: error instanceof Error ? error.message : undefined,
      }),
  });

  const topicOptions = topics
    .filter((topic) => topic.url)
    .map((topic) => ({ value: topic.url ?? "", label: topic.title ? `${topic.title} · ${topic.url}` : (topic.url ?? "") }));
  const ready =
    draft.topic != null &&
    (draft.channel === "websocket" ||
      (draft.channel === "email" && /^[^\s@]+@[^\s@]+$/.test(draft.endpoint.trim())) ||
      (draft.channel === "rest-hook" && /^https?:\/\//.test(draft.endpoint.trim())));

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="New subscription"
      size="md"
      footer={
        <>
          <Button variant="subtle" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="filled" disabled={!ready} loading={create.isPending} onClick={() => create.mutate()}>
            Create
          </Button>
        </>
      }
    >
      <form
        className={classes.form}
        onSubmit={(event) => {
          event.preventDefault();
          if (ready) create.mutate();
        }}
      >
        <TextInput label="Reason" placeholder="Sync patients to the CRM" value={draft.reason} onChange={(reason) => update({ reason })} />
        <Combobox
          label="Topic"
          placeholder={topicOptions.length === 0 ? "No active topics yet" : "Pick a topic"}
          data={topicOptions}
          value={draft.topic}
          onChange={(topic) => update({ topic })}
          emptyMessage="No topic matches"
        />
        <div className={classes.field}>
          <span className={classes.fieldLabel}>Channel</span>
          <SegmentedRadioGroup
            aria-label="Channel"
            value={draft.channel}
            onChange={(value) =>
              update({ channel: value === "websocket" || value === "email" ? value : "rest-hook", endpoint: "" })
            }
            options={[
              { value: "rest-hook", label: "REST hook" },
              { value: "websocket", label: "WebSocket" },
              { value: "email", label: "Email" },
            ]}
          />
        </div>
        {draft.channel === "rest-hook" ? (
          <>
            <TextInput
              label="Endpoint"
              placeholder="https://example.org/fhir-hook"
              value={draft.endpoint}
              onChange={(endpoint) => update({ endpoint })}
            />
            <TextArea
              label="Headers"
              placeholder="Authorization: Bearer …"
              description="One per line, sent with every notification."
              value={draft.headers}
              onChange={(headers) => update({ headers })}
              minRows={2}
            />
          </>
        ) : draft.channel === "email" ? (
          <TextInput
            label="Recipient"
            placeholder="care-team@example.org"
            description="Needs an email provider in the server's [subscriptions.email] config."
            value={draft.endpoint}
            onChange={(endpoint) => update({ endpoint })}
          />
        ) : (
          <p className={classes.muted}>
            Clients connect to <code>/fhir/Subscription/{"{id}"}/$events</code> with a WebSocket upgrade once the
            subscription is active.
          </p>
        )}
        <p className={classes.muted}>
          The server checks the topic and channel right away: the subscription becomes active, or shows why it
          can't be served.
        </p>
      </form>
    </Modal>
  );
}
