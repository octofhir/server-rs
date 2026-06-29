import { Badge, Button, EmptyState, Spin } from "@octofhir/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { NotebookPen, Plus } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import { listNotebooks } from "./api/notebookApi";
import classes from "./NotebookList.module.css";

export function NotebookListPage() {
  const navigate = useNavigate();
  const { data, isLoading } = useQuery({
    queryKey: ["notebooks"],
    queryFn: listNotebooks,
  });

  return (
    <ToolWorkspaceLayout
      title="Notebooks"
      description="Interactive multi-engine notebooks — markdown, FHIRPath, SQL-on-FHIR, SQL, charts"
      className="page-enter"
      actions={
        <Button leftSection={<Plus size={15} />} onClick={() => navigate("/notebooks/new")}>
          New notebook
        </Button>
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
