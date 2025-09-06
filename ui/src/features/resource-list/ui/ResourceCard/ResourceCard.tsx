import { ActionIcon, Badge, Box, Card, Group, Menu, Text, Tooltip } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconCopy, IconDots, IconEye, IconTrash } from "@tabler/icons-react";
import type React from "react";
import { useCallback } from "react";
import { setSelectedResource } from "@/entities/fhir";
import type { FhirResource } from "@/shared/api/types";
import { formatRelativeTime } from "@/shared/lib/time";
import styles from "./ResourceCard.module.css";

interface ResourceCardProps {
  resource: FhirResource;
  isSelected?: boolean;
  onDelete?: (resource: FhirResource) => void;
  className?: string;
}

export const ResourceCard: React.FC<ResourceCardProps> = ({
  resource,
  isSelected = false,
  onDelete,
  className,
}) => {
  // Extract key information from resource
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
          case "entered-in-error":
            return { status: "Error", color: "red" };
          default:
            return { status: status.charAt(0).toUpperCase() + status.slice(1), color: "gray" };
        }
      }

      return {};
    },
    []
  );

  const handleSelect = useCallback(() => {
    setSelectedResource(resource);
  }, [resource]);

  const handleCopyId = useCallback(
    async (e: React.MouseEvent) => {
      e.stopPropagation();

      if (!resource.id) {
        notifications.show({
          title: "Copy failed",
          message: "Resource has no ID",
          color: "red",
        });
        return;
      }

      try {
        await navigator.clipboard.writeText(resource.id);
        notifications.show({
          title: "Copied",
          message: `Resource ID copied: ${resource.id}`,
          color: "green",
        });
      } catch (error) {
        notifications.show({
          title: "Copy failed",
          message: "Failed to copy resource ID",
          color: "red",
        });
      }
    },
    [resource.id]
  );

  const handleDelete = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onDelete?.(resource);
    },
    [onDelete, resource]
  );

  const title = getResourceTitle(resource);
  const { status, color } = getResourceStatus(resource);
  const lastUpdated = resource.meta?.lastUpdated;

  return (
    <Card
      className={`${styles.card} ${isSelected ? styles.selected : ""} ${className || ""}`}
      onClick={handleSelect}
      withBorder
      padding="sm"
    >
      <Group justify="space-between" wrap="nowrap">
        <Box className={styles.content}>
          <Group gap="xs" wrap="nowrap">
            <Text size="sm" fw={500} className={styles.title}>
              {title}
            </Text>
            {status && (
              <Badge size="xs" color={color} variant="light">
                {status}
              </Badge>
            )}
          </Group>

          <Group gap="xs" mt="xs">
            <Text size="xs" c="dimmed">
              {resource.resourceType}
            </Text>
            {resource.id && (
              <>
                <Text size="xs" c="dimmed">
                  •
                </Text>
                <Text size="xs" c="dimmed" className={styles.resourceId}>
                  {resource.id}
                </Text>
              </>
            )}
            {lastUpdated && (
              <>
                <Text size="xs" c="dimmed">
                  •
                </Text>
                <Text size="xs" c="dimmed" title={new Date(lastUpdated).toLocaleString()}>
                  {formatRelativeTime(lastUpdated)}
                </Text>
              </>
            )}
          </Group>
        </Box>

        <Group gap="xs" wrap="nowrap">
          <Tooltip label="View details">
            <ActionIcon size="sm" variant="subtle" color="blue" onClick={handleSelect}>
              <IconEye size={16} />
            </ActionIcon>
          </Tooltip>

          <Menu position="bottom-end" withinPortal>
            <Menu.Target>
              <ActionIcon size="sm" variant="subtle" onClick={(e) => e.stopPropagation()}>
                <IconDots size={16} />
              </ActionIcon>
            </Menu.Target>

            <Menu.Dropdown>
              <Menu.Item leftSection={<IconEye size={14} />} onClick={handleSelect}>
                View Details
              </Menu.Item>
              <Menu.Item leftSection={<IconCopy size={14} />} onClick={handleCopyId}>
                Copy ID
              </Menu.Item>
              {onDelete && (
                <>
                  <Menu.Divider />
                  <Menu.Item
                    leftSection={<IconTrash size={14} />}
                    color="red"
                    onClick={handleDelete}
                  >
                    Delete
                  </Menu.Item>
                </>
              )}
            </Menu.Dropdown>
          </Menu>
        </Group>
      </Group>
    </Card>
  );
};
