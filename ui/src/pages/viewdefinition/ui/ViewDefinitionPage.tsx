import { useState, useCallback, useEffect } from "react";
import { Box, Flex, Text, Button, Spin, Alert } from "@/shared/ui";
import {
  IconPlus,
  IconPlayerPlay,
  IconDeviceFloppy,
  IconTrash,
  IconAlertCircle,
} from "@octofhir/ui-kit";
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
      color="warning"
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

  const handleDelete = useCallback(
    async (id: string) => {
      if (confirm("Are you sure you want to delete this ViewDefinition?")) {
        try {
          await deleteMutation.mutateAsync(id);
          if (selectedId === id) {
            setSelectedId(null);
            setCurrent(createEmptyViewDefinition());
          }
        } catch {
          // Error handled by mutation
        }
      }
    },
    [deleteMutation, selectedId]
  );

  const handleSelect = useCallback(
    (id: string | null) => {
      setSelectedId(id);
      const found = viewDefinitions?.find((v) => v.id === id);
      if (found) {
        // Ensure stable IDs for list items
        const viewDefWithIds = {
          ...found,
          select: found.select.map((s) => ({
            ...s,
            column: s.column?.map((c) => ({ ...c, _id: c._id || crypto.randomUUID() })),
          })),
          where: found.where?.map((w) => ({ ...w, _id: w._id || crypto.randomUUID() })),
          constant: found.constant?.map((c) => ({ ...c, _id: c._id || crypto.randomUUID() })),
        };
        setCurrent(viewDefWithIds);
        setRunResult(null);
        setSqlResult(null);
      } else {
        setCurrent(createEmptyViewDefinition());
        setRunResult(null);
        setSqlResult(null);
      }
    },
    [viewDefinitions]
  );

  const handleUpdate = useCallback((val: ViewDefinition) => {
    setCurrent(val);
  }, []);

  if (settingsLoading) {
    return (
      <Flex grow alignItems="center" justifyContent="center" style={{ height: "100%" }}>
        <Spin size="l" />
      </Flex>
    );
  }

  const isDisabled = settings?.sqlOnFhir?.enabled === false;

  return (
    <Box className={classes.container}>
      <Flex direction="column" gap="0" style={{ height: "100%" }}>
        {/* Header */}
        <Flex justifyContent="space-between" alignItems="center" px="4" py="2" style={{ borderBottom: "1px solid var(--g-color-line-base)" }}>
          <Flex gap="4" alignItems="center">
            <Text variant="header-2">ViewDefinition Editor</Text>
            {listLoading && <Spin size="s" />}
          </Flex>

          <Flex gap="2">
            <Button
              view="action"
              size="m"
              loading={runMutation.isPending}
              onClick={handleRun}
            >
              <Button.Icon>
                <IconPlayerPlay size={14} />
              </Button.Icon>
              Run
            </Button>
            <Button
              view="normal"
              size="m"
              loading={saveMutation.isPending}
              onClick={handleSave}
            >
              <Button.Icon>
                <IconDeviceFloppy size={14} />
              </Button.Icon>
              Save
            </Button>
            {selectedId && (
              <Button
                view="flat-danger"
                size="m"
                loading={deleteMutation.isPending}
                onClick={() => handleDelete(selectedId)}
              >
                <Button.Icon>
                  <IconTrash size={14} />
                </Button.Icon>
              </Button>
            )}
            <Button
              view="flat"
              size="m"
              onClick={() => handleSelect(null)}
            >
              <Button.Icon>
                <IconPlus size={14} />
              </Button.Icon>
              New
            </Button>
          </Flex>
        </Flex>

        {isDisabled && (
          <Box px="4" py="2">
            <FeatureDisabledBanner />
          </Box>
        )}

        <Flex gap="0" style={{ flex: 1, minHeight: 0 }}>
          {/* Sidebar */}
          <Sidebar
            items={viewDefinitions || []}
            selectedId={selectedId}
            onSelect={handleSelect}
          />

          {/* Main Editor / Preview Split */}
          <Flex gap="0" style={{ flex: 1 }}>
            <Box className={classes.editorSection}>
              <EditorPanel
                value={current}
                onChange={handleUpdate}
                resourceTypes={resourceTypes}
              />
            </Box>
            <Box className={classes.previewSection}>
              <PreviewPanel
                runResult={runResult}
                sqlResult={sqlResult}
                onRefreshSql={handleGenerateSql}
                isLoading={runMutation.isPending || sqlMutation.isPending}
              />
            </Box>
          </Flex>
        </Flex>
      </Flex>
    </Box>
  );
}
