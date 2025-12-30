import { useState, useCallback, useEffect } from "react";
import { Box, Group, Title, Button, Loader, Alert, Stack } from "@mantine/core";
import {
  IconPlus,
  IconPlayerPlay,
  IconDeviceFloppy,
  IconTrash,
  IconAlertCircle,
} from "@tabler/icons-react";
import { useSettings } from "../lib/useSettings";
import {
  useViewDefinitions,
  useRunViewDefinition,
  useSaveViewDefinition,
  useDeleteViewDefinition,
  useGenerateSql,
  type ViewDefinition,
  type RunResult,
  type SqlResult,
} from "../lib/useViewDefinition";
import { useResourceTypes } from "../lib/useResourceTypes";
import { Sidebar } from "./components/Sidebar";
import { EditorPanel } from "./components/EditorPanel";
import { PreviewPanel } from "./components/PreviewPanel";
import classes from "./ViewDefinitionPage.module.css";

// Feature disabled banner
function FeatureDisabledBanner() {
  return (
    <Alert
      icon={<IconAlertCircle size={16} />}
      title="SQL on FHIR Disabled"
      color="yellow"
      variant="light"
    >
      SQL on FHIR must be enabled in server configuration:
      <pre style={{ marginTop: 8 }}>
        [sql_on_fhir]
        enabled = true
      </pre>
    </Alert>
  );
}

// Empty state for new ViewDefinition
function createEmptyViewDefinition(): ViewDefinition {
  return {
    resourceType: "ViewDefinition",
    name: "",
    status: "draft",
    resource: "Patient",
    select: [{ column: [{ name: "id", path: "id", _id: crypto.randomUUID() }] }],
  };
}

// Main page component
export function ViewDefinitionPage() {
  const { data: settings, isLoading: settingsLoading } = useSettings();
  const { data: viewDefinitions, isLoading: listLoading } = useViewDefinitions();
  const { data: resourceTypes = [] } = useResourceTypes();
  const runMutation = useRunViewDefinition();
  const saveMutation = useSaveViewDefinition();
  const deleteMutation = useDeleteViewDefinition();
  const sqlMutation = useGenerateSql();

  const [current, setCurrent] = useState<ViewDefinition>(createEmptyViewDefinition());
  const [runResult, setRunResult] = useState<RunResult | null>(null);
  const [sqlResult, setSqlResult] = useState<SqlResult | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  // Generate SQL explicitly (called when user clicks SQL tab or Run button)
  const handleGenerateSql = useCallback(() => {
    if (current.select[0]?.column && current.select[0].column.length > 0) {
      sqlMutation.mutate(current, {
        onSuccess: (result) => setSqlResult(result),
        onError: () => setSqlResult(null),
      });
    } else {
      setSqlResult(null);
    }
  }, [sqlMutation, current]);

  const handleRun = useCallback(async () => {
    // Generate SQL first
    handleGenerateSql();

    try {
      const res = await runMutation.mutateAsync(current);
      setRunResult(res);
    } catch {
      // Error handled by mutation
    }
  }, [runMutation, current, handleGenerateSql]);

  const handleSave = useCallback(async () => {
    try {
      const saved = await saveMutation.mutateAsync(current);
      setCurrent(saved);
      setSelectedId(saved.id || null);
    } catch {
      // Error handled by mutation
    }
  }, [saveMutation, current]);

  const handleNew = useCallback(() => {
    setCurrent(createEmptyViewDefinition());
    setSelectedId(null);
    setRunResult(null);
    setSqlResult(null);
  }, []);

  const handleSelect = useCallback((viewDef: ViewDefinition) => {
    // Ensure all columns have stable IDs for React keys
    const viewDefWithIds = {
      ...viewDef,
      select: viewDef.select.map((s) => ({
        ...s,
        column: s.column?.map((c) => ({ ...c, _id: c._id || crypto.randomUUID() })),
      })),
      where: viewDef.where?.map((w) => ({ ...w, _id: w._id || crypto.randomUUID() })),
      constant: viewDef.constant?.map((c) => ({ ...c, _id: c._id || crypto.randomUUID() })),
    };
    setCurrent(viewDefWithIds);
    setSelectedId(viewDef.id || null);
    setRunResult(null);
  }, []);

  const handleDelete = useCallback(async () => {
    if (selectedId) {
      await deleteMutation.mutateAsync(selectedId);
      setCurrent(createEmptyViewDefinition());
      setSelectedId(null);
      setRunResult(null);
      setSqlResult(null);
    }
  }, [deleteMutation, selectedId]);

  // Check if feature is enabled
  if (settingsLoading) {
    return (
      <Box p="xl" ta="center">
        <Loader />
      </Box>
    );
  }

  if (!settings?.features.sqlOnFhir) {
    return (
      <Box p="xl" className="page-enter">
        <Stack gap="lg">
          <Title order={2}>ViewDefinition Editor</Title>
          <FeatureDisabledBanner />
        </Stack>
      </Box>
    );
  }

  return (
    <Box className={`${classes.container} page-enter`}>
      {/* Header */}
      <Group justify="space-between" p="md" className={classes.header}>
        <Title order={3}>ViewDefinition Editor</Title>
        <Group gap="xs">
          <Button
            variant="subtle"
            size="xs"
            leftSection={<IconPlus size={14} />}
            onClick={handleNew}
          >
            New
          </Button>
          <Button
            variant="light"
            size="xs"
            leftSection={<IconDeviceFloppy size={14} />}
            onClick={handleSave}
            loading={saveMutation.isPending}
            disabled={!current.name}
          >
            Save
          </Button>
          {selectedId && (
            <Button
              variant="light"
              color="red"
              size="xs"
              leftSection={<IconTrash size={14} />}
              onClick={handleDelete}
              loading={deleteMutation.isPending}
            >
              Delete
            </Button>
          )}
          <Button
            variant="filled"
            size="xs"
            leftSection={<IconPlayerPlay size={14} />}
            onClick={handleRun}
            loading={runMutation.isPending}
          >
            Run
          </Button>
        </Group>
      </Group>

      {/* 3-panel layout */}
      <Box className={classes.content}>
        {/* Left sidebar - Saved views list */}
        <Sidebar
          viewDefinitions={viewDefinitions}
          selectedId={selectedId}
          isLoading={listLoading}
          onSelect={handleSelect}
        />

        {/* Center panel - Editor */}
        <Box className={classes.editor}>
          <EditorPanel
            viewDef={current}
            resourceTypes={resourceTypes}
            onChange={setCurrent}
          />
        </Box>

        {/* Right panel - Preview */}
        <Box className={classes.preview}>
          <PreviewPanel
            sqlResult={sqlResult}
            sqlLoading={sqlMutation.isPending}
            sqlError={sqlMutation.error}
            runResult={runResult}
            onGenerateSql={handleGenerateSql}
          />
        </Box>
      </Box>

      {/* Error displays */}
      {runMutation.isError && (
        <Box p="md">
          <Alert icon={<IconAlertCircle size={16} />} color="red" title="Error">
            {runMutation.error?.message || "An error occurred"}
          </Alert>
        </Box>
      )}

      {saveMutation.isError && (
        <Box p="md">
          <Alert icon={<IconAlertCircle size={16} />} color="red" title="Save Error">
            {saveMutation.error?.message || "Failed to save"}
          </Alert>
        </Box>
      )}
    </Box>
  );
}
