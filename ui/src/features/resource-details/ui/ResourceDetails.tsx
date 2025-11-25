import { type Component, Show, createSignal, createMemo } from "solid-js";
import { useUnit } from "effector-solid";
import { $selectedResource, deleteResourceFx, fetchResourceFx } from "@/entities/fhir";
import type { FhirResource } from "@/shared/api/types";
import { formatRelativeTime } from "@/shared/lib/time";
import { JsonViewer, Button, Modal } from "@/shared/ui";
import { IconCopy, IconEdit, IconRefresh, IconTrash } from "@/shared/ui/Icon";
import { useToast } from "@/shared/ui/Toast";
import styles from "./ResourceDetails.module.css";

interface ResourceDetailsProps {
  class?: string;
  onEdit?: (resource: FhirResource) => void;
}

export const ResourceDetails: Component<ResourceDetailsProps> = (props) => {
  const selectedResource = useUnit($selectedResource);
  const toast = useToast();
  const [rawView, setRawView] = createSignal(false);
  const [loading, setLoading] = createSignal(false);
  const [showDeleteModal, setShowDeleteModal] = createSignal(false);

  const handleCopyId = async () => {
    const resource = selectedResource();
    if (!resource?.id) {
      toast.error("Resource has no ID", "Copy failed");
      return;
    }

    try {
      await navigator.clipboard.writeText(resource.id);
      toast.success(`Resource ID copied: ${resource.id}`, "Copied");
    } catch (error) {
      toast.error("Failed to copy resource ID", "Copy failed");
    }
  };

  const handleCopyJson = async () => {
    const resource = selectedResource();
    if (!resource) {
      return;
    }

    try {
      const json = JSON.stringify(resource, null, 2);
      await navigator.clipboard.writeText(json);
      toast.success("Resource JSON copied to clipboard", "Copied");
    } catch (error) {
      toast.error("Failed to copy resource JSON", "Copy failed");
    }
  };

  const handleDelete = () => {
    const resource = selectedResource();
    if (!resource?.id || !resource.resourceType) {
      return;
    }
    setShowDeleteModal(true);
  };

  const confirmDelete = async () => {
    const resource = selectedResource();
    if (!resource?.id || !resource.resourceType) {
      return;
    }

    try {
      await deleteResourceFx({
        resourceType: resource.resourceType,
        id: resource.id,
      });
      toast.success("Resource deleted successfully", "Deleted");
      setShowDeleteModal(false);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to delete resource", "Delete failed");
    }
  };

  const handleRefresh = async () => {
    const resource = selectedResource();
    if (!resource?.id || !resource.resourceType) {
      return;
    }

    setLoading(true);
    try {
      await fetchResourceFx({
        resourceType: resource.resourceType,
        resourceId: resource.id,
      });
      toast.success("Resource data refreshed", "Refreshed");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to refresh resource", "Refresh failed");
    } finally {
      setLoading(false);
    }
  };

  const handleEdit = () => {
    const resource = selectedResource();
    if (resource) {
      props.onEdit?.(resource);
    }
  };

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
        default:
          return { status: status.charAt(0).toUpperCase() + status.slice(1), variant: "neutral" };
      }
    }

    return {};
  };

  const title = createMemo(() => {
    const resource = selectedResource();
    return resource ? getResourceTitle(resource) : "";
  });

  const statusInfo = createMemo(() => {
    const resource = selectedResource();
    return resource ? getResourceStatus(resource) : {};
  });

  const lastUpdated = () => selectedResource()?.meta?.lastUpdated;

  return (
    <div class={`${styles.container} ${props.class || ""}`}>
      <Show
        when={selectedResource()}
        fallback={
          <div class={styles.emptyState}>
            <span class={styles.emptyText}>Select a resource to view details</span>
          </div>
        }
      >
        <div class={styles.header}>
          <div class={styles.titleSection}>
            <div class={styles.titleRow}>
              <span class={styles.title}>{title()}</span>
              <Show when={statusInfo().status}>
                <span class={`${styles.badge} ${styles[statusInfo().variant || "neutral"]}`}>
                  {statusInfo().status}
                </span>
              </Show>
            </div>
            <div class={styles.meta}>
              <span class={styles.metaItem}>{selectedResource()?.resourceType}</span>
              <Show when={selectedResource()?.id}>
                <span class={styles.metaSeparator}>•</span>
                <span class={`${styles.metaItem} ${styles.resourceId}`}>
                  {selectedResource()?.id}
                </span>
              </Show>
              <Show when={lastUpdated()}>
                <span class={styles.metaSeparator}>•</span>
                <span
                  class={styles.metaItem}
                  title={new Date(lastUpdated()!).toLocaleString()}
                >
                  {formatRelativeTime(lastUpdated()!)}
                </span>
              </Show>
            </div>
          </div>

          <div class={styles.actions}>
            <button
              class={`${styles.actionButton} ${rawView() ? styles.active : ""}`}
              onClick={() => setRawView(!rawView())}
              title="Toggle raw/formatted view"
            >
              {rawView() ? "Formatted" : "Raw"}
            </button>

            <button
              class={styles.iconButton}
              onClick={handleRefresh}
              disabled={loading()}
              title="Refresh"
            >
              <IconRefresh size={16} class={loading() ? styles.spinning : ""} />
            </button>

            <button
              class={styles.iconButton}
              onClick={handleCopyId}
              title="Copy ID"
            >
              <IconCopy size={16} />
            </button>

            <button
              class={styles.iconButton}
              onClick={handleCopyJson}
              title="Copy JSON"
            >
              <IconCopy size={16} />
            </button>

            <Show when={props.onEdit}>
              <button
                class={`${styles.iconButton} ${styles.primary}`}
                onClick={handleEdit}
                title="Edit"
              >
                <IconEdit size={16} />
              </button>
            </Show>

            <button
              class={`${styles.iconButton} ${styles.danger}`}
              onClick={handleDelete}
              title="Delete"
            >
              <IconTrash size={16} />
            </button>
          </div>
        </div>

        <div class={styles.content}>
          <Show
            when={!rawView()}
            fallback={
              <div class={styles.rawJson}>
                <pre>
                  <code>{JSON.stringify(selectedResource(), null, 2)}</code>
                </pre>
              </div>
            }
          >
            <JsonViewer
              data={selectedResource()}
              expanded={false}
              maxHeight={600}
              searchable={true}
              copyable={false}
            />
          </Show>
        </div>
      </Show>

      {/* Delete Confirmation Modal */}
      <Modal
        open={showDeleteModal()}
        onClose={() => setShowDeleteModal(false)}
        title="Delete Resource"
        size="sm"
      >
        <div class={styles.modalContent}>
          <p class={styles.modalText}>
            Are you sure you want to delete this resource? This action cannot be undone.
          </p>
          <div class={styles.deletePreview}>
            <span class={styles.deletePreviewTitle}>
              {selectedResource()?.resourceType}/{selectedResource()?.id}
            </span>
            <Show when={selectedResource()?.meta?.lastUpdated}>
              <span class={styles.deletePreviewMeta}>
                Last updated: {formatRelativeTime(selectedResource()!.meta!.lastUpdated!)}
              </span>
            </Show>
          </div>
          <div class={styles.modalActions}>
            <Button variant="secondary" onClick={() => setShowDeleteModal(false)}>
              Cancel
            </Button>
            <Button variant="danger" onClick={confirmDelete}>
              Delete
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};
