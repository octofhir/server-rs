import { Button, Menu, SegmentedRadioGroup, Spin, Switch, Tooltip } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { ArrowLeft, Download, Plus, Save, Sliders, Zap } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import { type ExportFormat, exportNotebook, readNotebook, saveNotebook } from "./api/notebookApi";
import { CellFrame } from "./components/CellFrame";
import { DataflowGraph } from "./components/DataflowGraph";
import { VariablesPanel } from "./components/VariablesPanel";
import { type CellType, emptyNotebook, stripOutputs } from "./model/notebook";
import {
  $namedCells,
  $notebook,
  $scope,
  $statuses,
  cellAdded,
  cellChanged,
  cellCollapseToggled,
  cellDeleted,
  cellDuplicated,
  cellMoved,
  cellTypeChanged,
  notebookLoaded,
  runAll,
  runBelow,
  runOne,
  runStale,
  titleChanged,
} from "./model/store";
import classes from "./NotebookEditor.module.css";

export function NotebookEditorPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const notebook = useUnit($notebook);
  const statuses = useUnit($statuses);
  const scope = useUnit($scope);
  const namedCells = useUnit($namedCells);

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [varsOpen, setVarsOpen] = useState(false);
  const [clearOnSave, setClearOnSave] = useState(false);
  const [view, setView] = useState<"cells" | "graph">("cells");

  useEffect(() => {
    let alive = true;
    setLoading(true);
    (async () => {
      try {
        if (id && id !== "new") {
          const nb = await readNotebook(id);
          if (alive) notebookLoaded(nb);
        } else if (alive) {
          notebookLoaded(emptyNotebook());
        }
      } finally {
        if (alive) setLoading(false);
      }
    })();
    return () => {
      alive = false;
    };
  }, [id]);

  const handleSave = useCallback(async () => {
    const nb = $notebook.getState();
    if (!nb) return;
    setSaving(true);
    try {
      const saved = await saveNotebook(clearOnSave ? stripOutputs(nb) : nb);
      notebookLoaded(saved);
      if (saved.id && (!id || id === "new")) navigate(`/notebooks/${saved.id}`, { replace: true });
    } finally {
      setSaving(false);
    }
  }, [clearOnSave, id, navigate]);

  if (loading || !notebook) {
    return (
      <div className={classes.loading}>
        <Spin />
      </div>
    );
  }

  const addButtons: CellType[] = [
    "markdown",
    "fhirpath",
    "sql",
    "sql-on-fhir",
    "cql",
    "graphql",
    "rest",
    "pipeline",
    "chart",
    "input",
  ];
  const addLabels: Record<CellType, string> = {
    markdown: "Markdown",
    fhirpath: "FHIRPath",
    sql: "SQL",
    "sql-on-fhir": "SQL-on-FHIR",
    chart: "Chart",
    input: "Input",
    cql: "CQL",
    graphql: "GraphQL",
    rest: "REST",
    pipeline: "Pipeline",
  };

  return (
    <ToolWorkspaceLayout
      title={notebook.title}
      description={notebook.description ?? "Interactive multi-engine notebook"}
      className="page-enter"
      actions={
        <div className={classes.headerActions}>
          <Button
            variant="subtle"
            leftSection={<ArrowLeft size={15} />}
            onClick={() => navigate("/notebooks")}
          >
            Notebooks
          </Button>
          <Tooltip label="Variables & inputs">
            <Button
              variant="subtle"
              leftSection={<Sliders size={15} />}
              onClick={() => setVarsOpen(true)}
            >
              Variables
            </Button>
          </Tooltip>
          <Button variant="light" onClick={() => runStale()}>
            Run stale
          </Button>
          <Button variant="light" leftSection={<Zap size={15} />} onClick={() => runAll()}>
            Run all
          </Button>
          <Button
            variant="light"
            leftSection={<Plus size={15} />}
            onClick={() => cellAdded("markdown")}
          >
            Add cell
          </Button>
          {notebook.id && (
            <Menu position="bottom-end">
              <Menu.Target>
                <Button variant="subtle" leftSection={<Download size={15} />}>
                  Export
                </Button>
              </Menu.Target>
              <Menu.Dropdown>
                {(
                  [
                    ["fhirnb", "Notebook (.fhirnb.json)"],
                    ["ipynb", "Jupyter (.ipynb)"],
                    ["bundle", "FHIR Bundle"],
                    ["markdown", "Markdown"],
                    ["html", "HTML report"],
                  ] as [ExportFormat, string][]
                ).map(([fmt, label]) => (
                  <Menu.Item
                    key={fmt}
                    onClick={() => notebook.id && exportNotebook(notebook.id, fmt)}
                  >
                    {label}
                  </Menu.Item>
                ))}
              </Menu.Dropdown>
            </Menu>
          )}
          <Button leftSection={<Save size={15} />} loading={saving} onClick={handleSave}>
            Save
          </Button>
        </div>
      }
    >
      <VariablesPanel opened={varsOpen} onClose={() => setVarsOpen(false)} />

      <div className={classes.editorRoot}>
        <div className={classes.titleRow}>
          <input
            className={classes.titleInput}
            value={notebook.title}
            onChange={(e) => titleChanged(e.currentTarget.value)}
            placeholder="Notebook title"
          />
          <SegmentedRadioGroup
            size="sm"
            value={view}
            onChange={(v) => setView(v as "cells" | "graph")}
            options={[
              { value: "cells", label: "Notebook" },
              { value: "graph", label: "Graph" },
            ]}
          />
          <Switch
            size="sm"
            checked={clearOnSave}
            onChange={setClearOnSave}
            label="Clear outputs on save"
          />
        </div>

        {view === "graph" ? (
          <div className={classes.scrollArea}>
            <DataflowGraph notebook={notebook} statuses={statuses} />
          </div>
        ) : (
          <div className={classes.scrollArea}>
            <div className={classes.cellStack}>
              {notebook.cells.map((cell, i) => (
                <CellFrame
                  key={cell.id}
                  cell={cell}
                  status={statuses[cell.id] ?? "idle"}
                  scope={scope}
                  namedCells={namedCells}
                  variables={notebook.variables ?? []}
                  isFirst={i === 0}
                  isLast={i === notebook.cells.length - 1}
                  onChange={(next) => cellChanged({ id: cell.id, next })}
                  onChangeType={(t) => cellTypeChanged({ id: cell.id, type: t })}
                  onRun={() => runOne(cell)}
                  onRunBelow={() => runBelow(cell.id)}
                  onDelete={() => cellDeleted(cell.id)}
                  onDuplicate={() => cellDuplicated(cell.id)}
                  onMove={(d) => cellMoved({ id: cell.id, dir: d })}
                  onToggleCollapse={() => cellCollapseToggled(cell.id)}
                />
              ))}
            </div>

            <div className={classes.addRow}>
              <span className={classes.addRowLabel}>Add cell</span>
              {addButtons.map((t) => (
                <Button
                  key={t}
                  variant="subtle"
                  leftSection={<Plus size={15} />}
                  onClick={() => cellAdded(t)}
                >
                  {addLabels[t]}
                </Button>
              ))}
            </div>
          </div>
        )}
      </div>
    </ToolWorkspaceLayout>
  );
}
