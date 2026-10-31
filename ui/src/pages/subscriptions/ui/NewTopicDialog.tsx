import { Button, Checkbox, Combobox, Modal, notify, TextInput } from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { fhirClient } from "@/shared/api";
import { useResourceTypesCategorized } from "@/shared/api/hooks";
import type { TopicResource } from "../lib/api";
import classes from "./SubscriptionsPage.module.css";

const INTERACTIONS = ["create", "update", "delete"] as const;
type Interaction = (typeof INTERACTIONS)[number];

interface Draft {
  title: string;
  /** Left empty, it follows the title. */
  url: string;
  resource: string | null;
  interactions: Interaction[];
}

const EMPTY: Draft = { title: "", url: "", resource: null, interactions: ["create", "update"] };

function slug(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** A canonical URL under this server's origin, e.g. `https://fhir.example.org/topics/patient-changes`. */
export function suggestedTopicUrl(title: string, origin: string): string {
  return `${origin}/topics/${slug(title) || "topic"}`;
}

export function buildTopic(draft: Draft, origin: string): TopicResource {
  return {
    resourceType: "SubscriptionTopic",
    url: draft.url.trim() || suggestedTopicUrl(draft.title, origin),
    title: draft.title.trim(),
    status: "active",
    resourceTrigger: [{ resource: draft.resource ?? "", supportedInteraction: [...draft.interactions] }],
  };
}

export function NewTopicDialog({
  open,
  onClose,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: (topic: TopicResource) => void;
}) {
  const { data: catalog } = useResourceTypesCategorized();
  const typeNames = useMemo(
    () => (catalog?.types ?? []).filter((type) => type.category !== "system").map((type) => type.name).sort(),
    [catalog]
  );
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const update = (patch: Partial<Draft>) => setDraft((current) => ({ ...current, ...patch }));
  const origin = window.location.origin;
  const create = useMutation({
    mutationFn: () => fhirClient.create<TopicResource>(buildTopic(draft, origin)),
    onSuccess: (created) => {
      setDraft(EMPTY);
      onCreated(created);
    },
    onError: (error) =>
      notify({ theme: "danger", title: "Topic not created", content: error instanceof Error ? error.message : undefined }),
  });
  const ready = draft.title.trim() !== "" && draft.resource != null && draft.interactions.length > 0;
  const toggle = (interaction: Interaction, on: boolean) =>
    update({
      interactions: on
        ? INTERACTIONS.filter((item) => item === interaction || draft.interactions.includes(item))
        : draft.interactions.filter((item) => item !== interaction),
    });

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="New topic"
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
        <TextInput label="Title" placeholder="Patient changes" value={draft.title} onChange={(title) => update({ title })} />
        <TextInput
          label="Canonical URL"
          placeholder={suggestedTopicUrl(draft.title, origin)}
          description="What subscriptions point at. Leave empty to use the one shown."
          value={draft.url}
          onChange={(url) => update({ url })}
        />
        <Combobox
          label="Resource type"
          placeholder="Patient"
          data={typeNames}
          value={draft.resource}
          onChange={(resource) => update({ resource })}
        />
        <div className={classes.field}>
          <span className={classes.fieldLabel}>Notify on</span>
          <div className={classes.checks}>
            {INTERACTIONS.map((interaction) => (
              <Checkbox
                key={interaction}
                label={interaction}
                checked={draft.interactions.includes(interaction)}
                onChange={(on) => toggle(interaction, on)}
              />
            ))}
          </div>
        </div>
        <p className={classes.muted}>The topic is active at once; subscriptions can point at it right away.</p>
      </form>
    </Modal>
  );
}
