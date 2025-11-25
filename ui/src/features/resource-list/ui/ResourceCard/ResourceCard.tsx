import { type Component, Show, createMemo } from "solid-js";
import { setSelectedResource } from "@/entities/fhir";
import type { FhirResource } from "@/shared/api/types";
import { formatRelativeTime } from "@/shared/lib/time";
import { IconEye, IconCopy, IconTrash, IconDots } from "@/shared/ui/Icon";
import { useToast } from "@/shared/ui/Toast";
import styles from "./ResourceCard.module.css";

interface ResourceCardProps {
  resource: FhirResource;
  isSelected?: boolean;
  onDelete?: (resource: FhirResource) => void;
  class?: string;
}

export const ResourceCard: Component<ResourceCardProps> = (props) => {
  const toast = useToast();

  // Extract key information from resource
  const getResourceTitle = (resource: FhirResource): string => {
    switch (resource.resourceType) {
      case "Patient": {
        const patient = resource as any;
        if (patient.name?.[0]) {
          const name = patient.name[0];
          return (
            `${name.given?.join(" ") || ""} ${name.family || ""}`.trim() || `Patient ${resource.id}`
          );
        }
        return `Patient ${resource.id}`;
      }

      case "Practitioner": {
        const practitioner = resource as any;
        if (practitioner.name?.[0]) {
          const name = practitioner.name[0];
          return (
            `${name.given?.join(" ") || ""} ${name.family || ""}`.trim() ||
            `Practitioner ${resource.id}`
          );
        }
        return `Practitioner ${resource.id}`;
      }

      case "Organization": {
        const org = resource as any;
        return org.name || `Organization ${resource.id}`;
      }

      case "Observation": {
        const obs = resource as any;
        return obs.code?.coding?.[0]?.display || obs.code?.text || `Observation ${resource.id}`;
      }

      case "Condition": {
        const condition = resource as any;
        return (
          condition.code?.coding?.[0]?.display || condition.code?.text || `Condition ${resource.id}`
        );
      }

      default:
        return `${resource.resourceType} ${resource.id}`;
    }
  };

  const getResourceStatus = (resource: FhirResource): { status?: string; variant?: "success" | "error" | "warning" | "neutral" } => {
    const res = resource as any;

    if (res.status) {
      const status = res.status.toLowerCase();
      switch (status) {
        case "active":
          return { status: "Active", variant: "success" };
        case "inactive":
        case "retired":
          return { status: "Inactive", variant: "neutral" };
        case "draft":
          return { status: "Draft", variant: "warning" };
        case "final":
          return { status: "Final", variant: "success" };
        case "cancelled":
        case "rejected":
          return { status: "Cancelled", variant: "error" };
        case "entered-in-error":
          return { status: "Error", variant: "error" };
        default:
          return { status: status.charAt(0).toUpperCase() + status.slice(1), variant: "neutral" };
      }
    }

    return {};
  };

  const handleSelect = () => {
    setSelectedResource(props.resource);
  };

  const handleCopyId = async (e: MouseEvent) => {
    e.stopPropagation();

    if (!props.resource.id) {
      toast.error("Resource has no ID", "Copy failed");
      return;
    }

    try {
      await navigator.clipboard.writeText(props.resource.id);
      toast.success(`Resource ID copied: ${props.resource.id}`, "Copied");
    } catch (error) {
      toast.error("Failed to copy resource ID", "Copy failed");
    }
  };

  const handleDelete = (e: MouseEvent) => {
    e.stopPropagation();
    props.onDelete?.(props.resource);
  };

  const title = createMemo(() => getResourceTitle(props.resource));
  const statusInfo = createMemo(() => getResourceStatus(props.resource));
  const lastUpdated = () => props.resource.meta?.lastUpdated;

  return (
    <div
      class={`${styles.card} ${props.isSelected ? styles.selected : ""} ${props.class || ""}`}
      onClick={handleSelect}
    >
      <div class={styles.content}>
        <div class={styles.header}>
          <span class={styles.title}>{title()}</span>
          <Show when={statusInfo().status}>
            <span class={`${styles.badge} ${styles[statusInfo().variant || "neutral"]}`}>
              {statusInfo().status}
            </span>
          </Show>
        </div>

        <div class={styles.meta}>
          <span class={styles.metaItem}>{props.resource.resourceType}</span>
          <Show when={props.resource.id}>
            <span class={styles.metaSeparator}>•</span>
            <span class={styles.metaItem}>{props.resource.id}</span>
          </Show>
          <Show when={lastUpdated()}>
            <span class={styles.metaSeparator}>•</span>
            <span class={styles.metaItem} title={new Date(lastUpdated()!).toLocaleString()}>
              {formatRelativeTime(lastUpdated()!)}
            </span>
          </Show>
        </div>
      </div>

      <div class={styles.actions}>
        <button class={styles.actionButton} onClick={handleSelect} title="View details">
          <IconEye size={16} />
        </button>
        <button class={styles.actionButton} onClick={handleCopyId} title="Copy ID">
          <IconCopy size={16} />
        </button>
        <Show when={props.onDelete}>
          <button class={`${styles.actionButton} ${styles.danger}`} onClick={handleDelete} title="Delete">
            <IconTrash size={16} />
          </button>
        </Show>
      </div>
    </div>
  );
};
