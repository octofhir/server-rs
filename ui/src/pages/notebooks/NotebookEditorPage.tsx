import { Button, Spin } from "@octofhir/ui-kit";
import { ArrowLeft, Plus, Save } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import { readNotebook, saveNotebook } from "./api/notebookApi";
import { CellFrame } from "./components/CellFrame";
import { runCell } from "./model/execution";
import {
  type Cell,
  type CellStatus,
  type CellType,
  defaultCell,
  emptyNotebook,
  type Notebook,
  newCellId,
  type Scope,
} from "./model/notebook";
import classes from "./NotebookEditor.module.css";

export function NotebookEditorPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const [notebook, setNotebook] = useState<Notebook | null>(null);
  const [statuses, setStatuses] = useState<Record<string, CellStatus>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    (async () => {
      try {
        if (id && id !== "new") {
          const nb = await readNotebook(id);
          if (alive) setNotebook(nb);
        } else if (alive) {
          setNotebook(emptyNotebook());
        }
      } finally {
        if (alive) setLoading(false);
      }
    })();
    return () => {
      alive = false;
    };
  }, [id]);

  // Reactive scope: variables + named-cell outputs.
  const scope: Scope = useMemo(() => {
    const s: Scope = {};
    for (const v of notebook?.variables ?? []) s[v.name] = v.value;
    for (const c of notebook?.cells ?? []) {
      if (!c.name || !c.outputs?.length) continue;
      const out = c.outputs[0];
      if (out.kind === "table") s[c.name] = { columns: out.columns, rows: out.rows };
      else if (out.kind === "value") s[c.name] = out.data;
    }
    return s;
  }, [notebook]);

  const namedCells = useMemo(
    () =>
      (notebook?.cells ?? [])
        .filter((c) => c.name && (c.type === "sql" || c.type === "sql-on-fhir"))
        .map((c) => ({ id: c.id, label: `${c.name} (${c.type})` })),
    [notebook]
  );

  const patchCell = useCallback((cellId: string, next: Cell) => {
    setNotebook((nb) =>
      nb ? { ...nb, cells: nb.cells.map((c) => (c.id === cellId ? next : c)) } : nb
    );
  }, []);

  const handleRun = useCallback(
    async (cell: Cell) => {
      setStatuses((s) => ({ ...s, [cell.id]: "running" }));
      const output = await runCell(cell, scope);
      setStatuses((s) => ({ ...s, [cell.id]: output.kind === "error" ? "error" : "ok" }));
      setNotebook((nb) =>
        nb
          ? {
              ...nb,
              cells: nb.cells.map((c) =>
                c.id === cell.id
                  ? { ...c, outputs: [output], execCount: (c.execCount ?? 0) + 1 }
                  : c
              ),
            }
          : nb
      );
    },
    [scope]
  );

  const runAll = useCallback(async () => {
    if (!notebook) return;
    for (const c of notebook.cells) {
      if (c.type !== "markdown") await handleRun(c);
    }
  }, [notebook, handleRun]);

  const addCell = useCallback((type: CellType = "markdown") => {
    setNotebook((nb) => (nb ? { ...nb, cells: [...nb.cells, defaultCell(type)] } : nb));
  }, []);

  const changeType = useCallback((cellId: string, type: CellType) => {
    setNotebook((nb) => {
      if (!nb) return nb;
      return {
        ...nb,
        cells: nb.cells.map((c) => {
          if (c.id !== cellId) return c;
          const fresh = defaultCell(type);
          return { ...fresh, id: c.id, name: c.name };
        }),
      };
    });
  }, []);

  const moveCell = useCallback((cellId: string, dir: -1 | 1) => {
    setNotebook((nb) => {
      if (!nb) return nb;
      const i = nb.cells.findIndex((c) => c.id === cellId);
      const j = i + dir;
      if (i < 0 || j < 0 || j >= nb.cells.length) return nb;
      const cells = [...nb.cells];
      [cells[i], cells[j]] = [cells[j], cells[i]];
      return { ...nb, cells };
    });
  }, []);

  const duplicateCell = useCallback((cellId: string) => {
    setNotebook((nb) => {
      if (!nb) return nb;
      const i = nb.cells.findIndex((c) => c.id === cellId);
      if (i < 0) return nb;
      const copy: Cell = { ...nb.cells[i], id: newCellId(), name: undefined, outputs: undefined };
      const cells = [...nb.cells];
      cells.splice(i + 1, 0, copy);
      return { ...nb, cells };
    });
  }, []);

  const deleteCell = useCallback((cellId: string) => {
    setNotebook((nb) => (nb ? { ...nb, cells: nb.cells.filter((c) => c.id !== cellId) } : nb));
  }, []);

  const toggleCollapse = useCallback((cellId: string) => {
    setNotebook((nb) =>
      nb
        ? {
            ...nb,
            cells: nb.cells.map((c) =>
              c.id === cellId ? { ...c, collapsed: !(c.collapsed ?? false) } : c
            ),
          }
        : nb
    );
  }, []);

  const handleSave = useCallback(async () => {
    if (!notebook) return;
    setSaving(true);
    try {
      const saved = await saveNotebook(notebook);
      setNotebook(saved);
      if (saved.id && (!id || id === "new")) navigate(`/notebooks/${saved.id}`, { replace: true });
    } finally {
      setSaving(false);
    }
  }, [notebook, id, navigate]);

  if (loading || !notebook) {
    return (
      <div className={classes.loading}>
        <Spin />
      </div>
    );
  }

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
          <Button variant="light" onClick={runAll}>
            Run all
          </Button>
          <Button
            variant="light"
            leftSection={<Plus size={15} />}
            onClick={() => addCell("markdown")}
          >
            Add cell
          </Button>
          <Button leftSection={<Save size={15} />} loading={saving} onClick={handleSave}>
            Save
          </Button>
        </div>
      }
    >
      <div className={classes.editorRoot}>
        <div className={classes.titleRow}>
          <input
            className={classes.titleInput}
            value={notebook.title}
            onChange={(e) => setNotebook({ ...notebook, title: e.currentTarget.value })}
            placeholder="Notebook title"
          />
        </div>

        <div className={classes.scrollArea}>
          <div className={classes.cellStack}>
            {notebook.cells.map((cell, i) => (
              <CellFrame
                key={cell.id}
                cell={cell}
                status={statuses[cell.id] ?? "idle"}
                scope={scope}
                namedCells={namedCells}
                isFirst={i === 0}
                isLast={i === notebook.cells.length - 1}
                onChange={(next) => patchCell(cell.id, next)}
                onChangeType={(t) => changeType(cell.id, t)}
                onRun={() => handleRun(cell)}
                onDelete={() => deleteCell(cell.id)}
                onDuplicate={() => duplicateCell(cell.id)}
                onMove={(d) => moveCell(cell.id, d)}
                onToggleCollapse={() => toggleCollapse(cell.id)}
              />
            ))}
          </div>

          <div className={classes.addRow}>
            <span className={classes.addRowLabel}>Add cell</span>
            <Button
              variant="subtle"
              leftSection={<Plus size={15} />}
              onClick={() => addCell("markdown")}
            >
              Markdown
            </Button>
            <Button
              variant="subtle"
              leftSection={<Plus size={15} />}
              onClick={() => addCell("fhirpath")}
            >
              FHIRPath
            </Button>
            <Button
              variant="subtle"
              leftSection={<Plus size={15} />}
              onClick={() => addCell("sql")}
            >
              SQL
            </Button>
            <Button
              variant="subtle"
              leftSection={<Plus size={15} />}
              onClick={() => addCell("sql-on-fhir")}
            >
              SQL-on-FHIR
            </Button>
            <Button
              variant="subtle"
              leftSection={<Plus size={15} />}
              onClick={() => addCell("chart")}
            >
              Chart
            </Button>
          </div>
        </div>
      </div>
    </ToolWorkspaceLayout>
  );
}
