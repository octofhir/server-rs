import { useState } from "react";
import {
  ActionIcon,
  Badge,
  Button,
  Divider,
  Group,
  MultiSelect,
  Paper,
  Select,
  Stack,
  Text,
  Textarea,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconPlus, IconTrash, IconBolt, IconClock, IconHandClick } from "@tabler/icons-react";
import { useAddTrigger, useDeleteTrigger } from "../../lib/useAutomations";
import type { AutomationTrigger, AutomationTriggerType, CreateTriggerRequest } from "@/shared/api/types";
import { useResourceTypes } from "@/shared/api/hooks/useSystemQueries";

interface TriggerConfigProps {
  automationId: string;
  triggers: AutomationTrigger[];
}

const triggerTypeOptions = [
  { value: "resource_event", label: "Resource Event" },
  { value: "cron", label: "Scheduled (Cron)" },
  { value: "manual", label: "Manual" },
];

const eventTypeOptions = [
  { value: "created", label: "Created" },
  { value: "updated", label: "Updated" },
  { value: "deleted", label: "Deleted" },
];

const triggerTypeIcons: Record<AutomationTriggerType, React.ReactNode> = {
  resource_event: <IconBolt size={16} />,
  cron: <IconClock size={16} />,
  manual: <IconHandClick size={16} />,
};

export function TriggerConfig({ automationId, triggers }: TriggerConfigProps) {
  const [isAdding, setIsAdding] = useState(false);
  const [newTrigger, setNewTrigger] = useState<Partial<CreateTriggerRequest>>({
    trigger_type: "resource_event",
  });

  const addMutation = useAddTrigger();
  const deleteMutation = useDeleteTrigger();
  const { data: resourceTypes } = useResourceTypes();

  const resourceTypeOptions = (resourceTypes || []).map((rt) => ({
    value: rt,
    label: rt,
  }));

  const handleAdd = async () => {
    if (!newTrigger.trigger_type) {
      notifications.show({
        title: "Validation Error",
        message: "Select a trigger type",
        color: "red",
      });
      return;
    }

    if (newTrigger.trigger_type === "resource_event") {
      if (!newTrigger.resource_type) {
        notifications.show({
          title: "Validation Error",
          message: "Select a resource type",
          color: "red",
        });
        return;
      }
      if (!newTrigger.event_types || newTrigger.event_types.length === 0) {
        notifications.show({
          title: "Validation Error",
          message: "Select at least one event type",
          color: "red",
        });
        return;
      }
    }

    if (newTrigger.trigger_type === "cron" && !newTrigger.cron_expression) {
      notifications.show({
        title: "Validation Error",
        message: "Enter a cron expression",
        color: "red",
      });
      return;
    }

    try {
      await addMutation.mutateAsync({
        automationId,
        trigger: newTrigger as CreateTriggerRequest,
      });

      notifications.show({
        title: "Trigger Added",
        message: "Trigger has been added successfully",
        color: "green",
      });

      setIsAdding(false);
      setNewTrigger({ trigger_type: "resource_event" });
    } catch (error) {
      notifications.show({
        title: "Error",
        message: error instanceof Error ? error.message : "Failed to add trigger",
        color: "red",
      });
    }
  };

  const handleDelete = async (triggerId: string) => {
    try {
      await deleteMutation.mutateAsync({ automationId, triggerId });
      notifications.show({
        title: "Trigger Deleted",
        message: "Trigger has been removed",
        color: "green",
      });
    } catch (error) {
      notifications.show({
        title: "Error",
        message: error instanceof Error ? error.message : "Failed to delete trigger",
        color: "red",
      });
    }
  };

  const formatTriggerDescription = (trigger: AutomationTrigger) => {
    switch (trigger.trigger_type) {
      case "resource_event": {
        const base = `${trigger.resource_type} - ${trigger.event_types?.join(", ")}`;
        return trigger.fhirpath_filter ? `${base} (filtered)` : base;
      }
      case "cron":
        return trigger.cron_expression;
      case "manual":
        return "Execute via API or playground";
      default:
        return "";
    }
  };

  return (
    <Stack gap="md">
      <Group justify="space-between">
        <Text fw={500}>Triggers</Text>
        {!isAdding && (
          <Button
            size="xs"
            variant="light"
            leftSection={<IconPlus size={14} />}
            onClick={() => setIsAdding(true)}
          >
            Add Trigger
          </Button>
        )}
      </Group>

      {/* Existing triggers */}
      {triggers.length === 0 && !isAdding ? (
        <Text c="dimmed" size="sm">
          No triggers configured. Add a trigger to automatically run this automation.
        </Text>
      ) : (
        <Stack gap="xs">
          {triggers.map((trigger) => (
            <Paper key={trigger.id} withBorder p="sm" radius="md">
              <Group justify="space-between">
                <Group gap="sm">
                  {triggerTypeIcons[trigger.trigger_type]}
                  <div>
                    <Badge size="sm" variant="light">
                      {trigger.trigger_type === "resource_event"
                        ? "Resource Event"
                        : trigger.trigger_type === "cron"
                          ? "Scheduled"
                          : "Manual"}
                    </Badge>
                    <Text size="sm" c="dimmed" mt={4}>
                      {formatTriggerDescription(trigger)}
                    </Text>
                    {trigger.fhirpath_filter && (
                      <Tooltip label={trigger.fhirpath_filter} multiline w={300}>
                        <Text size="xs" c="blue" mt={2} style={{ cursor: "help" }}>
                          Filter: {trigger.fhirpath_filter.length > 40
                            ? `${trigger.fhirpath_filter.slice(0, 40)}...`
                            : trigger.fhirpath_filter}
                        </Text>
                      </Tooltip>
                    )}
                  </div>
                </Group>
                <ActionIcon
                  variant="subtle"
                  color="red"
                  onClick={() => handleDelete(trigger.id)}
                  loading={deleteMutation.isPending}
                >
                  <IconTrash size={16} />
                </ActionIcon>
              </Group>
            </Paper>
          ))}
        </Stack>
      )}

      {/* Add new trigger form */}
      {isAdding && (
        <>
          <Divider />
          <Paper withBorder p="md" radius="md" bg="var(--mantine-color-gray-light)">
            <Stack gap="md">
              <Text fw={500} size="sm">
                New Trigger
              </Text>

              <Select
                label="Trigger Type"
                data={triggerTypeOptions}
                value={newTrigger.trigger_type}
                onChange={(value) =>
                  setNewTrigger({
                    trigger_type: value as AutomationTriggerType,
                  })
                }
                required
              />

              {newTrigger.trigger_type === "resource_event" && (
                <>
                  <Select
                    label="Resource Type"
                    data={resourceTypeOptions}
                    value={newTrigger.resource_type}
                    onChange={(value) =>
                      setNewTrigger({ ...newTrigger, resource_type: value || undefined })
                    }
                    searchable
                    required
                    placeholder="Select resource type"
                  />
                  <MultiSelect
                    label="Event Types"
                    data={eventTypeOptions}
                    value={newTrigger.event_types || []}
                    onChange={(value) => setNewTrigger({ ...newTrigger, event_types: value })}
                    required
                    placeholder="Select events to trigger on"
                  />
                  <Textarea
                    label="FHIRPath Filter (optional)"
                    placeholder="e.g., active = true or name.exists()"
                    value={newTrigger.fhirpath_filter || ""}
                    onChange={(e) =>
                      setNewTrigger({
                        ...newTrigger,
                        fhirpath_filter: e.target.value || undefined,
                      })
                    }
                    description="Only trigger when this FHIRPath expression evaluates to true"
                    autosize
                    minRows={1}
                    maxRows={3}
                  />
                </>
              )}

              {newTrigger.trigger_type === "cron" && (
                <TextInput
                  label="Cron Expression"
                  placeholder="*/5 * * * *"
                  value={newTrigger.cron_expression || ""}
                  onChange={(e) =>
                    setNewTrigger({ ...newTrigger, cron_expression: e.target.value })
                  }
                  description="e.g., '0 * * * *' for every hour, '*/5 * * * *' for every 5 minutes"
                  required
                />
              )}

              {newTrigger.trigger_type === "manual" && (
                <Text size="sm" c="dimmed">
                  This automation can only be triggered manually via the API or playground.
                </Text>
              )}

              <Group justify="flex-end" mt="sm">
                <Button
                  variant="default"
                  size="xs"
                  onClick={() => {
                    setIsAdding(false);
                    setNewTrigger({ trigger_type: "resource_event" });
                  }}
                >
                  Cancel
                </Button>
                <Button size="xs" onClick={handleAdd} loading={addMutation.isPending}>
                  Add Trigger
                </Button>
              </Group>
            </Stack>
          </Paper>
        </>
      )}
    </Stack>
  );
}
