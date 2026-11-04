import {
  Alert,
  type AutocompleteOption,
  Button,
  IconAlertCircle,
  IconDeviceFloppy,
  IconPlayerPlay,
  IconPlus,
  IconTrash,
  Menu,
  Spin,
  useDebouncedValue,
} from "@octofhir/ui-kit";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Database, Table2 } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { fhirClient } from "@/shared/api/fhirClient";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import { operationMessage, type RunResult, sqlDescribe } from "../lib/sofApi";
import { createEmptySqlQuery, libraryFromQuery, type SqlQuery } from "../lib/sqlQuery";
import { useResourceTypes } from "../lib/useResourceTypes";
import { useSettings } from "../lib/useSettings";
import {
  runSqlQuery,
  runViewDefinition,
  type SqlResult,
  useDeleteSqlQuery,
  useDeleteViewDefinition,
  useGenerateSql,
  useSaveSqlQuery,
  useSaveViewDefinition,
  useSqlQueries,
  useViewDefinitions,
  type ViewDefinition,
} from "../lib/useViewDefinition";
import { DependencyReport } from "./components/DependencyReport";
import { EditorPanel } from "./components/EditorPanel";
import { PreviewPanel } from "./components/PreviewPanel";
import { type SidebarItem, Sidebar } from "./components/Sidebar";
import { SqlQueryEditor } from "./components/SqlQueryEditor";
import { ViewSqlPlan } from "./components/SQLPreview";
import classes from "./ViewDefinitionPage.module.css";

// Feature disabled banner
function FeatureDisabledBanner() {
  return (
    <Alert icon={<IconAlertCircle size={16} />} title="SQL on FHIR Disabled" color="warning">
      SQL on FHIR must be enabled in server configuration:
      <pre className={classes.configSnippet}>[sql_on_fhir] enabled = true</pre>
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

/** What the editor holds: a ViewDefinition or a SQL query. */
type Draft = { kind: "view"; view: ViewDefinition } | { kind: "sql"; query: SqlQuery };

const ROW_LIMIT = 1000;

/** Ensure stable editor ids for list items of a loaded ViewDefinition. */
function withIds(found: ViewDefinition): ViewDefinition {
  return {
    ...found,
    select: found.select.map((s) => ({
      ...s,
      column: s.column?.map((c) => ({ ...c, _id: c._id || crypto.randomUUID() })),
    })),
    where: found.where?.map((w) => ({ ...w, _id: w._id || crypto.randomUUID() })),
    constant: found.constant?.map((c) => ({ ...c, _id: c._id || crypto.randomUUID() })),
  };
}

/** Canonicals a dependency can name: saved views and SQL views, stored terminology. */
function useCanonicalSuggestions(views: ViewDefinition[], queries: SqlQuery[]) {
  const { data: terminology = [] } = useQuery({
    queryKey: ["sof-terminology-canonicals"],
    queryFn: async (): Promise<AutocompleteOption[]> => {
      const [valueSets, conceptMaps] = await Promise.all(
        ["ValueSet", "ConceptMap"].map((type) =>
          fhirClient.search(type, { _count: 100, _elements: "url,version,name,title" })
        )
      );
      return [valueSets, conceptMaps].flatMap((bundle, index) =>
        (bundle.entry ?? []).flatMap((entry) => {
          const r = entry.resource as { url?: string; version?: string; title?: string; name?: string };
          if (!r.url) return [];
          const canonical = r.version ? `${r.url}|${r.version}` : r.url;
          return [{ value: canonical, label: `${index === 0 ? "ValueSet" : "ConceptMap"} · ${canonical}` }];
        })
      );
    },
    staleTime: 60_000,
  });
  return useMemo<AutocompleteOption[]>(
    () => [
      ...views
        .filter((v) => v.url)
        .map((v) => ({ value: v.url as string, label: `ViewDefinition · ${v.url}` })),
      ...queries
        .filter((q) => q.kind === "sql-view" && q.url)
        .map((q) => ({ value: q.url as string, label: `SQLView · ${q.url}` })),
      ...terminology,
    ],
    [views, queries, terminology]
  );
}

// Main page component
export function ViewDefinitionPage() {
  const { data: settings, isLoading: settingsLoading } = useSettings();
  const { data: viewDefinitions = [], isLoading: viewsLoading } = useViewDefinitions();
  const { data: sqlQueries = [], isLoading: queriesLoading } = useSqlQueries();
  const { data: resourceTypes = [] } = useResourceTypes();
  const saveView = useSaveViewDefinition();
  const deleteView = useDeleteViewDefinition();
  const saveQuery = useSaveSqlQuery();
  const deleteQuery = useDeleteSqlQuery();
  const sqlMutation = useGenerateSql();

  const [draft, setDraft] = useState<Draft>(() => emptyDraft("view"));
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [runResult, setRunResult] = useState<RunResult | null>(null);
  const [sqlResult, setSqlResult] = useState<SqlResult | null>(null);

  const run = useMutation({
    mutationFn: (d: Draft) =>
      d.kind === "view" ? runViewDefinition(d.view, ROW_LIMIT) : runSqlQuery(d.query, ROW_LIMIT),
    onSuccess: setRunResult,
    onError: () => setRunResult(null),
  });

  // The SQL query's dependencies as `$sql-run` resolves them, refreshed while
  // editing (debounced; identical Libraries hit the query cache).
  const incomplete =
    draft.kind === "sql" && draft.query.dependencies.some((d) => !d.label.trim() || !d.canonical.trim());
  const libraryJson =
    draft.kind === "sql" && !incomplete ? JSON.stringify(libraryFromQuery(draft.query)) : null;
  const debouncedLibrary = useDebouncedValue(libraryJson, 700);
  const describe = useQuery({
    queryKey: ["sql-describe", debouncedLibrary],
    queryFn: () => sqlDescribe(JSON.parse(debouncedLibrary as string)),
    enabled: !!debouncedLibrary,
    retry: false,
    placeholderData: (previous) => previous,
    staleTime: 30_000,
  });
  const describeError = incomplete
    ? "Each dependency needs a label (its table name in the SQL) and a canonical."
    : describe.error
      ? operationMessage(describe.error)
      : undefined;

  const suggestions = useCanonicalSuggestions(viewDefinitions, sqlQueries);

  const reset = useCallback((next: Draft, key: string | null) => {
    setDraft(next);
    setSelectedKey(key);
    setRunResult(null);
    setSqlResult(null);
    run.reset();
  }, [run]);

  // Generate SQL explicitly (called when user opens the Plan tab or runs a view)
  const handleGenerateSql = useCallback(() => {
    if (draft.kind !== "view") return;
    if (draft.view.select[0]?.column && draft.view.select[0].column.length > 0) {
      sqlMutation.mutate(draft.view, {
        onSuccess: (result) => setSqlResult(result),
        onError: () => setSqlResult(null),
      });
    } else {
      setSqlResult(null);
    }
  }, [sqlMutation, draft]);

  const handleRun = useCallback(() => {
    if (draft.kind === "view") handleGenerateSql();
    run.mutate(draft);
  }, [draft, run, handleGenerateSql]);

  const handleSave = useCallback(async () => {
    try {
      if (draft.kind === "view") {
        const saved = await saveView.mutateAsync(draft.view);
        setDraft({ kind: "view", view: withIds(saved) });
        setSelectedKey(saved.id ? `view/${saved.id}` : null);
      } else {
        const saved = await saveQuery.mutateAsync(draft.query);
        setDraft({ kind: "sql", query: saved });
        setSelectedKey(saved.id ? `sql/${saved.id}` : null);
      }
    } catch {
      // Error handled by mutation
    }
  }, [draft, saveView, saveQuery]);

  const handleDelete = useCallback(async () => {
    const id = draft.kind === "view" ? draft.view.id : draft.query.id;
    if (!id) return;
    const what = draft.kind === "view" ? "ViewDefinition" : "SQL query";
    if (!confirm(`Are you sure you want to delete this ${what}?`)) return;
    try {
      if (draft.kind === "view") await deleteView.mutateAsync(id);
      else await deleteQuery.mutateAsync(id);
      reset(emptyDraft(draft.kind, viewDefinitions), null);
    } catch {
      // Error handled by mutation
    }
  }, [draft, deleteView, deleteQuery, reset, viewDefinitions]);

  const sidebarItems = useMemo<SidebarItem[]>(
    () => [
      ...viewDefinitions
        .filter((v) => v.id)
        .map((v) => ({
          key: `view/${v.id}`,
          kind: "view" as const,
          id: v.id as string,
          name: v.name,
          status: v.status,
          meta: v.resource,
        })),
      ...sqlQueries
        .filter((q) => q.id)
        .map((q) => ({
          key: `sql/${q.id}`,
          kind: "sql" as const,
          id: q.id as string,
          name: q.name,
          status: q.status,
          meta: q.kind === "sql-view" ? "view" : "query",
        })),
    ],
    [viewDefinitions, sqlQueries]
  );

  const handleSelect = useCallback(
    (item: SidebarItem) => {
      if (item.kind === "view") {
        const found = viewDefinitions.find((v) => v.id === item.id);
        if (found) reset({ kind: "view", view: withIds(found) }, item.key);
      } else {
        const found = sqlQueries.find((q) => q.id === item.id);
        if (found) reset({ kind: "sql", query: found }, item.key);
      }
    },
    [viewDefinitions, sqlQueries, reset]
  );

  // Sampled values from the first preview row, keyed by column name — surfaced
  // inline next to each column's FHIRPath for instant feedback.
  const sampleRow = useMemo<Record<string, string> | undefined>(() => {
    const first = runResult?.rows?.[0];
    if (!first || typeof first !== "object") return undefined;
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(first)) {
      out[k] = v == null ? "" : typeof v === "object" ? JSON.stringify(v) : String(v);
    }
    return out;
  }, [runResult]);

  if (settingsLoading) {
    return (
      <div className={classes.loadingState}>
        <Spin size="lg" />
      </div>
    );
  }

  const isDisabled = settings?.features?.sqlOnFhir === false;
  const currentId = draft.kind === "view" ? draft.view.id : draft.query.id;
  const saving = saveView.isPending || saveQuery.isPending;
  const saveError = saveView.error ?? saveQuery.error;

  return (
    <ToolWorkspaceLayout
      title="Views"
      description="SQL on FHIR: build ViewDefinitions and SQL queries over them, run them through $sql-run."
      className="page-enter"
      actions={
        <div className={classes.actions}>
          {(viewsLoading || queriesLoading) && <Spin size="sm" />}
          <Button variant="filled" size="md" loading={run.isPending} onClick={handleRun}>
            <Button.Icon>
              <IconPlayerPlay size={14} />
            </Button.Icon>
            Run
          </Button>
          <Button variant="default" size="md" loading={saving} onClick={handleSave}>
            <Button.Icon>
              <IconDeviceFloppy size={14} />
            </Button.Icon>
            Save
          </Button>
          {currentId && (
            <Button
              variant="subtle"
              color="red"
              size="md"
              loading={deleteView.isPending || deleteQuery.isPending}
              onClick={handleDelete}
              aria-label="Delete"
            >
              <Button.Icon>
                <IconTrash size={14} />
              </Button.Icon>
            </Button>
          )}
          <Menu position="bottom-end">
            <Menu.Target>
              <Button variant="subtle" size="md">
                <Button.Icon>
                  <IconPlus size={14} />
                </Button.Icon>
                New
              </Button>
            </Menu.Target>
            <Menu.Dropdown>
              <Menu.Item
                leftSection={<Table2 size={14} />}
                onClick={() => reset(emptyDraft("view"), null)}
              >
                ViewDefinition
              </Menu.Item>
              <Menu.Item
                leftSection={<Database size={14} />}
                onClick={() => reset(emptyDraft("sql", viewDefinitions), null)}
              >
                SQL query
              </Menu.Item>
            </Menu.Dropdown>
          </Menu>
        </div>
      }
    >
      {isDisabled && (
        <div className={classes.disabledBanner}>
          <FeatureDisabledBanner />
        </div>
      )}
      {saveError && (
        <div className={classes.disabledBanner}>
          <Alert icon={<IconAlertCircle size={16} />} title="Save failed" theme="danger">
            {operationMessage(saveError)}
          </Alert>
        </div>
      )}

      <div className={classes.content}>
        <Sidebar items={sidebarItems} selectedKey={selectedKey} onSelect={handleSelect} />

        <div className={classes.workspace}>
          <div className={classes.editorSection}>
            {draft.kind === "view" ? (
              <EditorPanel
                value={draft.view}
                onChange={(view) => setDraft({ kind: "view", view })}
                resourceTypes={resourceTypes}
                sampleRow={sampleRow}
              />
            ) : (
              <SqlQueryEditor
                value={draft.query}
                onChange={(query) => setDraft({ kind: "sql", query })}
                description={describe.data}
                describeError={describeError}
                suggestions={suggestions}
                onRun={handleRun}
              />
            )}
          </div>
          <div className={classes.previewSection}>
            <PreviewPanel
              runResult={runResult}
              runError={run.error ? operationMessage(run.error) : undefined}
              planLabel={draft.kind === "view" ? "Plan" : "Dependencies"}
              onPlanOpen={draft.kind === "view" ? handleGenerateSql : undefined}
              plan={
                draft.kind === "view" ? (
                  <ViewSqlPlan result={sqlResult} isLoading={sqlMutation.isPending} />
                ) : (
                  <DependencyReport
                    description={describe.data}
                    describeError={describeError}
                    terminology={runResult?.terminology ?? []}
                  />
                )
              }
              isLoading={run.isPending || sqlMutation.isPending}
            />
          </div>
        </div>
      </div>
    </ToolWorkspaceLayout>
  );
}

function emptyDraft(kind: Draft["kind"], views: ViewDefinition[] = []): Draft {
  return kind === "view"
    ? { kind: "view", view: createEmptyViewDefinition() }
    : { kind: "sql", query: createEmptySqlQuery(views.find((v) => v.url)) };
}
