import { useState } from "react";
import { Modal, TextInput, Textarea, Button, Stack, Group } from "@/shared/ui";
import { notifications } from "@octofhir/ui-kit";
import { useNavigate } from "react-router-dom";
import { useCreateAutomation } from "../lib/useAutomations";

interface CreateAutomationModalProps {
  opened: boolean;
  onClose: () => void;
}

const DEFAULT_SOURCE_CODE = `/**
 * Automation Script
 *
 * Available global APIs:
 * - fhir: FHIR client (create, read, update, delete, search, patch)
 * - fetch: Native fetch API for HTTP requests
 * - execution: Structured logging (log, info, debug, warn, error)
 *
 * @param ctx - Context with the triggering event
 * @param ctx.event - Event: { type, resource, previous?, timestamp }
 */
export default async function(ctx: AutomationContext) {
  const { event } = ctx;

  execution.log("Processing event", { type: event.type, resourceType: event.resource.resourceType });

  if (event.type === "created") {
    const resource = event.resource;
    execution.info("Resource created", { id: resource.id, type: resource.resourceType });

    // Example: Create a follow-up task
    // const task = fhir.create({
    //   resourceType: "Task",
    //   status: "requested",
    //   intent: "order",
    //   description: \`Follow-up for \${resource.resourceType}/\${resource.id}\`,
    //   for: { reference: \`\${resource.resourceType}/\${resource.id}\` }
    // });
    // execution.info("Created task", { taskId: task.id });
  }
}
`;

export function CreateAutomationModal({ opened, onClose }: CreateAutomationModalProps) {
  const navigate = useNavigate();
  const createMutation = useCreateAutomation();

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  const handleCreate = async () => {
    if (!name.trim()) {
      notifications.show({
        title: "Validation Error",
        message: "Name is required",
        color: "red",
      });
      return;
    }

    try {
      const automation = await createMutation.mutateAsync({
        name: name.trim(),
        description: description.trim() || undefined,
        source_code: DEFAULT_SOURCE_CODE,
        timeout_ms: 5000,
      });

      notifications.show({
        title: "Automation Created",
        message: `Successfully created "${automation.name}"`,
        color: "green",
      });

      onClose();
      setName("");
      setDescription("");

      // Navigate to editor
      navigate(`/automations/${automation.id}`);
    } catch (error) {
      notifications.show({
        title: "Error",
        message: error instanceof Error ? error.message : "Failed to create automation",
        color: "red",
      });
    }
  };

  const handleClose = () => {
    setName("");
    setDescription("");
    onClose();
  };

  return (
    <Modal opened={opened} onClose={handleClose} title="Create Automation" size="md">
      <Stack gap="md">
        <TextInput
          label="Name"
          placeholder="My Automation"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
          data-autofocus
        />

        <Textarea
          label="Description"
          placeholder="Optional description of what this automation does"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={3}
        />

        <Group justify="flex-end" mt="md">
          <Button variant="default" onClick={handleClose}>
            Cancel
          </Button>
          <Button onClick={handleCreate} loading={createMutation.isPending}>
            Create
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
