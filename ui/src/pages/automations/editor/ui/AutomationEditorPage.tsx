import { notifications } from "@octofhir/ui-kit";
import { useState, useEffect, useCallback } from "react";
import { useParams, useNavigate, useSearchParams } from "react-router-dom";
import {
  Button,
  TextInput,
  Textarea,
  NumberInput,
  Switch,
  Tabs,
  Loader,
  Text,
  ActionIcon,
  Tooltip,
  Divider,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { isAutomationFeatureUnavailableError } from "@/shared/api/automationsApi";
import { ArrowLeft, Save as FloppyDisk, Rocket, Play, Settings as Gear, Zap as Thunderbolt, History as ClockArrowRotateLeft } from "lucide-react";
import { useAutomation, useCreateAutomation, useUpdateAutomation, useDeployAutomation } from "../../lib/useAutomations";
import { AutomationScriptEditor } from "@/shared/monaco/AutomationScriptEditor";
import { DEFAULT_AUTOMATION_SOURCE_CODE } from "../../ui/CreateAutomationModal";
import { TriggerConfig } from "./TriggerConfig";
import { PlaygroundPanel } from "./PlaygroundPanel";
import { ExecutionHistory } from "./ExecutionHistory";
import classes from "./AutomationEditorPage.module.css";

export function AutomationEditorPage() {
  const { id } = useParams<{ id: string }>();
  const isNewAutomation = !id;
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();

  const { data: automation, isLoading, error } = useAutomation(id);
  const createMutation = useCreateAutomation();
  const updateMutation = useUpdateAutomation();
  const deployMutation = useDeployAutomation();

  // Local state for editing
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [sourceCode, setSourceCode] = useState(DEFAULT_AUTOMATION_SOURCE_CODE);
  const [timeoutMs, setTimeoutMs] = useState<number>(5000);
  const [isDirty, setIsDirty] = useState(false);

  // Tab state
  const activeTab = isNewAutomation ? "settings" : searchParams.get("tab") || "settings";
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
    } else if (isNewAutomation) {
      setName("");
      setDescription("");
      setSourceCode(DEFAULT_AUTOMATION_SOURCE_CODE);
      setTimeoutMs(5000);
      setIsDirty(false);
    }
  }, [automation, isNewAutomation]);

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

  const handleTimeoutChange = (value: number | null) => {
    setTimeoutMs(typeof value === "number" ? value : 5000);
    setIsDirty(true);
  };

  // Save handler
  const handleSave = useCallback(async () => {
    try {
      if (isNewAutomation) {
        const created = await createMutation.mutateAsync({
          name: name.trim() || "Untitled Automation",
          description: description || undefined,
          source_code: sourceCode,
          timeout_ms: timeoutMs,
        });

        setIsDirty(false);
        notifications.show({
          title: "Created",
          message: "Automation created successfully",
          color: "green",
        });
        navigate(`/automations/${created.id}`);
        return;
      }

      if (!id) return;

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
        title: isAutomationFeatureUnavailableError(error) ? "Automations Disabled" : "Save Failed",
        message: isAutomationFeatureUnavailableError(error)
          ? "The backend did not expose the automation API."
          : error instanceof Error ? error.message : "Failed to save automation",
        color: "red",
      });
    }
  }, [id, isNewAutomation, name, description, sourceCode, timeoutMs, createMutation, updateMutation, navigate]);

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

  if (isLoading && !isNewAutomation) {
    return (
      <div className={classes.stateContainer}>
        <Loader />
      </div>
    );
  }

  if (error || (!automation && !isNewAutomation)) {
    return (
      <div className={classes.stateContainer}>
        <div className={classes.errorState}>
          <Text c="red">Failed to load automation</Text>
          <Button variant="light" onClick={() => navigate("/automations")}>
            Back to list
          </Button>
        </div>
      </div>
    );
  }

  return (
    <WorkspacePageLayout
      title={name || "Untitled Automation"}
      description={
        isNewAutomation
          ? "Create an automation script"
          : isDirty
            ? "Unsaved changes"
            : "Edit source, triggers, playground, and execution history"
      }
      className="page-enter"
      bodyClassName={classes.body}
      contentClassName={classes.container}
      actions={
        <div className={classes.headerActions}>
          <ActionIcon variant="subtle" onClick={() => navigate("/automations")} aria-label="Back to automations">
            <ArrowLeft size={20} />
          </ActionIcon>
          <Tooltip label={isDirty ? "Save changes (Ctrl+S)" : "No unsaved changes"}>
            <Button
              variant={isDirty ? "filled" : "default"}
              color={isDirty ? "blue" : undefined}
              leftSection={<FloppyDisk size={16} />}
              onClick={handleSave}
              loading={updateMutation.isPending || createMutation.isPending}
            >
              {isNewAutomation ? "Create" : "Save"}
            </Button>
          </Tooltip>
          {!isNewAutomation && (
            <>
              <Tooltip label="Deploy and activate">
                <Button
                  variant="light"
                  color="blue"
                  leftSection={<Rocket size={16} />}
                  onClick={handleDeploy}
                  loading={deployMutation.isPending}
                >
                  Deploy
                </Button>
              </Tooltip>
              <Tooltip label="Test (Ctrl+Enter)">
                <Button
                  color="green"
                  leftSection={<Play size={16} />}
                  onClick={handleExecute}
                >
                  Test
                </Button>
              </Tooltip>
            </>
          )}
        </div>
      }
    >

      {/* Editor */}
      <div className={classes.editorContainer}>
        <AutomationScriptEditor
          value={sourceCode}
          onChange={handleCodeChange}
          onSave={handleSave}
          onExecute={handleExecute}
          height="100%"
        />
      </div>

      <Divider />

      {/* Bottom panel with tabs */}
      <div className={classes.bottomPanel}>
        <Tabs value={activeTab} onChange={setActiveTab} h="100%">
          <Tabs.List>
            <Tabs.Tab value="settings" leftSection={<Gear size={14} />}>
              Settings
            </Tabs.Tab>
            {!isNewAutomation && (
              <>
                <Tabs.Tab value="triggers" leftSection={<Thunderbolt size={14} />}>
                  Triggers ({automation?.triggers?.length || 0})
                </Tabs.Tab>
                <Tabs.Tab value="playground" leftSection={<Play size={14} />}>
                  Playground
                </Tabs.Tab>
                <Tabs.Tab value="history" leftSection={<ClockArrowRotateLeft size={14} />}>
                  History
                </Tabs.Tab>
              </>
            )}
          </Tabs.List>

          <div className={classes.tabContent}>
            <Tabs.Panel value="settings" className={classes.tabPanel}>
              <div className={classes.settingsForm}>
                <TextInput
                  label="Name"
                  value={name}
                  onChange={(value) => handleNameChange(value)}
                  required
                />
                <Textarea
                  label="Description"
                  value={description}
                  onChange={(value) => handleDescriptionChange(value)}
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
                <div className={classes.statusRow}>
                  <Text size="sm">Status:</Text>
                  <Switch
                    checked={automation?.status === "active"}
                    label={automation?.status === "active" ? "Active" : "Inactive"}
                    disabled
                  />
                  <Text size="xs" c="dimmed">
                    (Deploy to change status)
                  </Text>
                </div>
              </div>
            </Tabs.Panel>

            {!isNewAutomation && automation && (
              <>
                <Tabs.Panel value="triggers" className={classes.tabPanel}>
                  <TriggerConfig
                    automationId={automation.id}
                    triggers={automation.triggers || []}
                  />
                </Tabs.Panel>

                <Tabs.Panel value="playground" className={classes.tabPanel}>
                  <PlaygroundPanel automationId={automation.id} sourceCode={sourceCode} />
                </Tabs.Panel>

                <Tabs.Panel value="history" className={classes.tabPanel}>
                  <ExecutionHistory automationId={automation.id} />
                </Tabs.Panel>
              </>
            )}
          </div>
        </Tabs>
      </div>
    </WorkspacePageLayout>
  );
}
