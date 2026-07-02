import { Badge, Button, EmptyState, Spin } from "@octofhir/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { NotebookPen, Plus, Upload } from "lucide-react";
import { useRef } from "react";
import { useNavigate } from "react-router-dom";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import {
  createNotebook,
  type ImportFormat,
  importNotebook,
  listNotebooks,
} from "./api/notebookApi";
import classes from "./NotebookList.module.css";

/** Detect import format from a parsed document. */
function detectFormat(doc: unknown): ImportFormat {
  const d = doc as { resourceType?: string; cells?: Array<{ cell_type?: string }> };
  if (d.resourceType === "Bundle") return "bundle";
  if (Array.isArray(d.cells) && d.cells.some((c) => "cell_type" in c)) return "ipynb";
  return "fhirnb";
}

export function NotebookListPage() {
  const navigate = useNavigate();
  const fileRef = useRef<HTMLInputElement>(null);
  const { data, isLoading, refetch } = useQuery({
    queryKey: ["notebooks"],
    queryFn: listNotebooks,
  });

  const handleImport = async (file: File) => {
    const text = await file.text();
    const doc = JSON.parse(text);
    const imported = await importNotebook(doc, detectFormat(doc));
    const saved = await createNotebook(imported);
    await refetch();
    if (saved.id) navigate(`/notebooks/${saved.id}`);
  };

  return (
    <ToolWorkspaceLayout
      title="Notebooks"
      description="Interactive multi-engine notebooks — markdown, FHIRPath, SQL-on-FHIR, SQL, charts"
      className="page-enter"
      actions={
        <div className={classes.headerActions}>
          <input
            ref={fileRef}
            type="file"
            accept=".json,.ipynb,application/json"
            style={{ display: "none" }}
            onChange={(e) => {
              const f = e.currentTarget.files?.[0];
              if (f) handleImport(f);
              e.currentTarget.value = "";
            }}
          />
          <Button
            variant="subtle"
            leftSection={<Upload size={15} />}
            onClick={() => fileRef.current?.click()}
          >
            Import
          </Button>
          <Button leftSection={<Plus size={15} />} onClick={() => navigate("/notebooks/new")}>
            New notebook
          </Button>
        </div>
      }
    >
      {isLoading ? (
        <div className={classes.loading}>
          <Spin />
        </div>
      ) : !data || data.length === 0 ? (
        <EmptyState
          title="No notebooks yet"
          description="Create your first notebook to mix prose, queries, and charts in one document."
          actions={[
            {
              text: "New notebook",
              onClick: () => navigate("/notebooks/new"),
            },
          ]}
        />
      ) : (
        <div className={classes.scrollArea}>
          <div className={classes.grid}>
            {data.map((nb) => (
              <button
                type="button"
                key={nb.id}
                className={classes.card}
                onClick={() => navigate(`/notebooks/${nb.id}`)}
              >
                <div className={classes.cardIcon}>
                  <NotebookPen size={18} />
                </div>
                <div className={classes.cardTitle}>{nb.title}</div>
                {nb.description && <div className={classes.cardDesc}>{nb.description}</div>}
                <div className={classes.cardMeta}>
                  <span>
                    {nb.cellCount} cell{nb.cellCount === 1 ? "" : "s"}
                  </span>
                  {nb.fhirVersion && <span>· {nb.fhirVersion}</span>}
                </div>
                {nb.tags && nb.tags.length > 0 && (
                  <div className={classes.cardTags}>
                    {nb.tags.map((t) => (
                      <Badge key={t} variant="light" size="sm">
                        {t}
                      </Badge>
                    ))}
                  </div>
                )}
              </button>
            ))}
          </div>
        </div>
      )}
    </ToolWorkspaceLayout>
  );
}
