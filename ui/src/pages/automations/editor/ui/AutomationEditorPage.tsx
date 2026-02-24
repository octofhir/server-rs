import { useState, useEffect, useCallback } from "react";
import { useParams, useNavigate, useSearchParams } from "react-router-dom";
import {
  Stack,
  Group,
  Title,
  Button,
  TextInput,
  Textarea,
  NumberInput,
  Switch,
  Tabs,
  Loader,
  Center,
  Text,
  ActionIcon,
  Tooltip,
  Divider,
  Box,
} from "@/shared/ui";
import { notifications } from "@octofhir/ui-kit";
import {
  IconArrowLeft,
  IconDeviceFloppy,
  IconRocket,
  IconPlayerPlay,
  IconSettings,
  IconBolt,
  IconHistory,
} from "@tabler/icons-react";
import { useAutomation, useUpdateAutomation, useDeployAutomation } from "../../lib/useAutomations";
import { AutomationScriptEditor } from "@/shared/monaco/AutomationScriptEditor";
import { TriggerConfig } from "./TriggerConfig";
import { PlaygroundPanel } from "./PlaygroundPanel";
import { ExecutionHistory } from "./ExecutionHistory";
import classes from "./AutomationEditorPage.module.css";

export function AutomationEditorPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();

  const { data: automation, isLoading, error } = useAutomation(id);
  const updateMutation = useUpdateAutomation();
  const deployMutation = useDeployAutomation();

  // Local state for editing
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [sourceCode, setSourceCode] = useState("");
  const [timeoutMs, setTimeoutMs] = useState<number>(5000);
  const [isDirty, setIsDirty] = useState(false);

  // Tab state
  const activeTab = searchParams.get("tab") || "settings";
  const setActiveTab = (tab: string | null) => {
    if (tab) {
      setSearchParams({ tab });
    }
  };

  // Initialize state from automation data
  useEffect(() => {
    if (automation) {
      setName(automation.name);
      setDescription(automation.description || "");
      setSourceCode(automation.source_code);
      setTimeoutMs(automation.timeout_ms);
      setIsDirty(false);
    }
  }, [automation]);

  // Track changes
  const handleNameChange = (value: string) => {
    setName(value);
    setIsDirty(true);
  };

  const handleDescriptionChange = (value: string) => {
    setDescription(value);
    setIsDirty(true);
  };

  const handleCodeChange = (value: string) => {
    setSourceCode(value);
    setIsDirty(true);
  };

  const handleTimeoutChange = (value: number | string) => {
    setTimeoutMs(typeof value === "number" ? value : 5000);
    setIsDirty(true);
  };

  // Save handler
  const handleSave = useCallback(async () => {
    if (!id) return;

    try {
      await updateMutation.mutateAsync({
        id,
        data: {
          name,
          description: description || undefined,
          source_code: sourceCode,
          timeout_ms: timeoutMs,
        },
      });

      setIsDirty(false);
      notifications.show({
        title: "Saved",
        message: "Automation saved successfully",
        color: "green",
      });
    } catch (error) {
      notifications.show({
        title: "Save Failed",
        message: error instanceof Error ? error.message : "Failed to save automation",
        color: "red",
      });
    }
  }, [id, name, description, sourceCode, timeoutMs, updateMutation]);

  // Deploy handler
  const handleDeploy = async () => {
    if (!id) return;

    // Save first if dirty
    if (isDirty) {
      await handleSave();
    }

    try {
      await deployMutation.mutateAsync(id);
      notifications.show({
        title: "Deployed",
        message: "Automation deployed and activated successfully",
        color: "green",
      });
    } catch (error) {
      notifications.show({
        title: "Deploy Failed",
        message: error instanceof Error ? error.message : "Failed to deploy automation",
        color: "red",
      });
    }
  };

  // Execute handler (switches to playground tab)
  const handleExecute = () => {
    setActiveTab("playground");
  };

  if (isLoading) {
    return (
      <Center h="100%">
        <Loader />
      </Center>
    );
  }

  if (error || !automation) {
    return (
      <Center h="100%">
        <Stack align="center" gap="md">
          <Text c="red">Failed to load automation</Text>
          <Button variant="light" onClick={() => navigate("/automations")}>
            Back to list
          </Button>
        </Stack>
      </Center>
    );
  }

  return (
    <Stack className={`page-enter ${classes.container}`} gap={0} h="100%">
      {/* Header */}
      <Group className={classes.header} justify="space-between" p="md" pb="sm">
        <Group gap="md">
          <ActionIcon variant="subtle" onClick={() => navigate("/automations")}>
            <IconArrowLeft size={20} />
          </ActionIcon>
          <Title order={3}>{name || "Untitled Automation"}</Title>
          {isDirty && (
            <Text size="xs" c="dimmed">
              (unsaved changes)
            </Text>
          )}
        </Group>
        <Group gap="sm">
          <Tooltip label={isDirty ? "Save changes (Ctrl+S)" : "No unsaved changes"}>
            <Button
              variant={isDirty ? "filled" : "default"}
              color={isDirty ? "blue" : undefined}
              leftSection={<IconDeviceFloppy size={16} />}
              onClick={handleSave}
              loading={updateMutation.isPending}
            >
              Save
            </Button>
          </Tooltip>
          <Tooltip label="Deploy and activate">
            <Button
              variant="light"
              color="blue"
              leftSection={<IconRocket size={16} />}
              onClick={handleDeploy}
              loading={deployMutation.isPending}
            >
              Deploy
            </Button>
          </Tooltip>
          <Tooltip label="Test (Ctrl+Enter)">
            <Button
              color="green"
              leftSection={<IconPlayerPlay size={16} />}
              onClick={handleExecute}
            >
              Test
            </Button>
          </Tooltip>
        </Group>
      </Group>

      <Divider />

      {/* Editor */}
      <Box className={classes.editorContainer}>
        <AutomationScriptEditor
          value={sourceCode}
          onChange={handleCodeChange}
          onSave={handleSave}
          onExecute={handleExecute}
          height="100%"
        />
      </Box>

      <Divider />

      {/* Bottom panel with tabs */}
      <Box className={classes.bottomPanel}>
        <Tabs value={activeTab} onChange={setActiveTab} h="100%">
          <Tabs.List>
            <Tabs.Tab value="settings" leftSection={<IconSettings size={14} />}>
              Settings
            </Tabs.Tab>
            <Tabs.Tab value="triggers" leftSection={<IconBolt size={14} />}>
              Triggers ({automation.triggers?.length || 0})
            </Tabs.Tab>
            <Tabs.Tab value="playground" leftSection={<IconPlayerPlay size={14} />}>
              Playground
            </Tabs.Tab>
            <Tabs.Tab value="history" leftSection={<IconHistory size={14} />}>
              History
            </Tabs.Tab>
          </Tabs.List>

          <Box className={classes.tabContent}>
            <Tabs.Panel value="settings" h="100%" p="md">
              <Stack gap="md" maw={600}>
                <TextInput
                  label="Name"
                  value={name}
                  onChange={(e) => handleNameChange(e.target.value)}
                  required
                />
                <Textarea
                  label="Description"
                  value={description}
                  onChange={(e) => handleDescriptionChange(e.target.value)}
                  rows={2}
                />
                <NumberInput
                  label="Timeout (ms)"
                  value={timeoutMs}
                  onChange={handleTimeoutChange}
                  min={100}
                  max={60000}
                  step={100}
                  description="Maximum execution time in milliseconds"
                />
                <Group>
                  <Text size="sm">Status:</Text>
                  <Switch
                    checked={automation.status === "active"}
                    label={automation.status === "active" ? "Active" : "Inactive"}
                    disabled
                  />
                  <Text size="xs" c="dimmed">
                    (Deploy to change status)
                  </Text>
                </Group>
              </Stack>
            </Tabs.Panel>

            <Tabs.Panel value="triggers" h="100%" p="md">
              <TriggerConfig
                automationId={automation.id}
                triggers={automation.triggers || []}
              />
            </Tabs.Panel>

            <Tabs.Panel value="playground" h="100%" p="md">
              <PlaygroundPanel automationId={automation.id} sourceCode={sourceCode} />
            </Tabs.Panel>

            <Tabs.Panel value="history" h="100%" p="md">
              <ExecutionHistory automationId={automation.id} />
            </Tabs.Panel>
          </Box>
        </Tabs>
      </Box>
    </Stack>
  );
}
