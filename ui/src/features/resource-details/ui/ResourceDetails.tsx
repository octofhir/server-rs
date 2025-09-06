import { ActionIcon, Badge, Box, Button, Group, Text, Tooltip } from "@mantine/core";
import { modals } from "@mantine/modals";
import { notifications } from "@mantine/notifications";
import { IconCopy, IconEdit, IconRefresh, IconTrash } from "@tabler/icons-react";
import { useUnit } from "effector-react";
import type React from "react";
import { useCallback, useState } from "react";
import { $selectedResource, deleteResourceFx, fetchResourceFx } from "@/entities/fhir";
import type { FhirResource } from "@/shared/api/types";
import { formatRelativeTime } from "@/shared/lib/time";
import { JsonViewer } from "@/shared/ui/JsonViewer";
import styles from "./ResourceDetails.module.css";

interface ResourceDetailsProps {
  className?: string;
  onEdit?: (resource: FhirResource) => void;
}

export const ResourceDetails: React.FC<ResourceDetailsProps> = ({ className, onEdit }) => {
  const selectedResource = useUnit($selectedResource);
  const [rawView, setRawView] = useState(false);
  const [loading, setLoading] = useState(false);

  const handleCopyId = useCallback(async () => {
    if (!selectedResource?.id) {
      notifications.show({
        title: "Copy failed",
        message: "Resource has no ID",
        color: "red",
      });
      return;
    }

    try {
      await navigator.clipboard.writeText(selectedResource.id);
      notifications.show({
        title: "Copied",
        message: `Resource ID copied: ${selectedResource.id}`,
        color: "green",
      });
    } catch (error) {
      notifications.show({
        title: "Copy failed",
        message: "Failed to copy resource ID",
        color: "red",
      });
    }
  }, [selectedResource?.id]);

  const handleCopyJson = useCallback(async () => {
    if (!selectedResource) {
      return;
    }

    try {
      const json = JSON.stringify(selectedResource, null, 2);
      await navigator.clipboard.writeText(json);
      notifications.show({
        title: "Copied",
        message: "Resource JSON copied to clipboard",
        color: "green",
      });
    } catch (error) {
      notifications.show({
        title: "Copy failed",
        message: "Failed to copy resource JSON",
        color: "red",
      });
    }
  }, [selectedResource]);

  const handleDelete = useCallback(() => {
    if (!selectedResource?.id || !selectedResource.resourceType) {
      return;
    }

    modals.openConfirmModal({
      title: "Delete Resource",
      children: (
        <Box>
          <Text size="sm">
            Are you sure you want to delete this resource? This action cannot be undone.
          </Text>
          <Box mt="md" p="md" className={styles.deletePreview}>
            <Text size="sm" fw={500}>
              {selectedResource.resourceType}/{selectedResource.id}
            </Text>
            <Text size="xs" c="dimmed">
              {selectedResource.meta?.lastUpdated &&
                `Last updated: ${formatRelativeTime(selectedResource.meta.lastUpdated)}`}
            </Text>
          </Box>
        </Box>
      ),
      labels: { confirm: "Delete", cancel: "Cancel" },
      confirmProps: { color: "red" },
      onConfirm: () => {
        deleteResourceFx({
          resourceType: selectedResource.resourceType,
          id: selectedResource.id!,
        });
      },
    });
  }, [selectedResource]);

  const handleRefresh = useCallback(async () => {
    if (!selectedResource?.id || !selectedResource.resourceType) {
      return;
    }

    setLoading(true);
    try {
      await fetchResourceFx({
        resourceType: selectedResource.resourceType,
        resourceId: selectedResource.id,
      });
      notifications.show({
        title: "Refreshed",
        message: "Resource data refreshed",
        color: "green",
      });
    } catch (error) {
      notifications.show({
        title: "Refresh failed",
        message: error instanceof Error ? error.message : "Failed to refresh resource",
        color: "red",
      });
    } finally {
      setLoading(false);
    }
  }, [selectedResource]);

  const handleEdit = useCallback(() => {
    if (selectedResource) {
      onEdit?.(selectedResource);
    }
  }, [selectedResource, onEdit]);

  const getResourceTitle = useCallback((resource: FhirResource): string => {
    // Try to get a meaningful title based on resource type
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
  }, []);

  const getResourceStatus = useCallback(
    (resource: FhirResource): { status?: string; color?: string } => {
      const res = resource as any;

      if (res.status) {
        const status = res.status.toLowerCase();
        switch (status) {
          case "active":
            return { status: "Active", color: "green" };
          case "inactive":
          case "retired":
            return { status: "Inactive", color: "gray" };
          case "draft":
            return { status: "Draft", color: "blue" };
          case "final":
            return { status: "Final", color: "green" };
          case "cancelled":
          case "rejected":
            return { status: "Cancelled", color: "red" };
          default:
            return { status: status.charAt(0).toUpperCase() + status.slice(1), color: "gray" };
        }
      }

      return {};
    },
    []
  );

  if (!selectedResource) {
    return (
      <Box className={`${styles.container} ${className || ""}`}>
        <Box className={styles.emptyState}>
          <Text size="sm" c="dimmed" ta="center">
            Select a resource to view details
          </Text>
        </Box>
      </Box>
    );
  }

  const title = getResourceTitle(selectedResource);
  const { status, color } = getResourceStatus(selectedResource);

  return (
    <Box className={`${styles.container} ${className || ""}`}>
      <Box className={styles.header}>
        <Box className={styles.titleSection}>
          <Group gap="xs">
            <Text size="sm" fw={600} className={styles.title}>
              {title}
            </Text>
            {status && (
              <Badge size="sm" color={color} variant="light">
                {status}
              </Badge>
            )}
          </Group>
          <Group gap="xs" mt="xs">
            <Text size="xs" c="dimmed">
              {selectedResource.resourceType}
            </Text>
            {selectedResource.id && (
              <>
                <Text size="xs" c="dimmed">
                  •
                </Text>
                <Text size="xs" c="dimmed" className={styles.resourceId}>
                  {selectedResource.id}
                </Text>
              </>
            )}
            {selectedResource.meta?.lastUpdated && (
              <>
                <Text size="xs" c="dimmed">
                  •
                </Text>
                <Text
                  size="xs"
                  c="dimmed"
                  title={new Date(selectedResource.meta.lastUpdated).toLocaleString()}
                >
                  {formatRelativeTime(selectedResource.meta.lastUpdated)}
                </Text>
              </>
            )}
          </Group>
        </Box>

        <Group gap="xs">
          <Tooltip label="Toggle raw/formatted view">
            <Button
              size="xs"
              variant={rawView ? "filled" : "light"}
              onClick={() => setRawView(!rawView)}
            >
              {rawView ? "Formatted" : "Raw"}
            </Button>
          </Tooltip>

          <Tooltip label="Refresh">
            <ActionIcon size="sm" variant="subtle" onClick={handleRefresh} loading={loading}>
              <IconRefresh size={16} />
            </ActionIcon>
          </Tooltip>

          <Tooltip label="Copy ID">
            <ActionIcon size="sm" variant="subtle" onClick={handleCopyId}>
              <IconCopy size={16} />
            </ActionIcon>
          </Tooltip>

          <Tooltip label="Copy JSON">
            <ActionIcon size="sm" variant="subtle" onClick={handleCopyJson}>
              <IconCopy size={16} />
            </ActionIcon>
          </Tooltip>

          {onEdit && (
            <Tooltip label="Edit">
              <ActionIcon size="sm" variant="subtle" color="blue" onClick={handleEdit}>
                <IconEdit size={16} />
              </ActionIcon>
            </Tooltip>
          )}

          <Tooltip label="Delete">
            <ActionIcon size="sm" variant="subtle" color="red" onClick={handleDelete}>
              <IconTrash size={16} />
            </ActionIcon>
          </Tooltip>
        </Group>
      </Box>

      <Box className={styles.content}>
        {rawView ? (
          <Box className={styles.rawJson}>
            <pre>
              <code>{JSON.stringify(selectedResource, null, 2)}</code>
            </pre>
          </Box>
        ) : (
          <JsonViewer
            data={selectedResource}
            expanded={false}
            maxHeight={600}
            searchable={true}
            copyable={false}
          />
        )}
      </Box>
    </Box>
  );
};
