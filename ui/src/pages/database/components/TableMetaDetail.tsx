import { Loader, modals, notifications } from "@octofhir/ui-kit";
import {
  Database,
  Fingerprint,
  Hash,
  Key,
  RefreshCw,
  Trash2 as TrashBin,
  Wand2,
} from "lucide-react";
import { useCallback, useMemo } from "react";
import { getDbColumnViews, getDbIndexViews } from "@/entities/db-schema";
import type { DbSchemaTableView } from "@/entities/db-schema";
import { useDropIndex, useRunMaintenance, useTableDetail } from "@/shared/api/hooks";
import type { MaintenanceOp } from "@/shared/api/types";
import classes from "./TableMetaDetail.module.css";

interface TableMetaDetailProps {
  table: DbSchemaTableView;
}

interface MaintenanceAction {
  op: MaintenanceOp;
  label: string;
  /** Heavy ops take exclusive locks / rewrite — require confirmation. */
  heavy: boolean;
  hint: string;
}

const ACTIONS: MaintenanceAction[] = [
  { op: "vacuum", label: "Vacuum", heavy: false, hint: "Reclaim dead tuples (online)" },
  { op: "analyze", label: "Analyze", heavy: false, hint: "Refresh planner statistics" },
  { op: "reindex", label: "Reindex", heavy: true, hint: "Rebuild all indexes (locks table)" },
  {
    op: "vacuum_full",
    label: "Vacuum Full",
    heavy: true,
    hint: "Rewrite table, reclaim disk (locks table)",
  },
];

export function TableMetaDetail({ table }: TableMetaDetailProps) {
  const { schema, name } = table;
  const { data, isLoading } = useTableDetail(schema, name);
  const dropIndexMutation = useDropIndex();
  const maintenanceMutation = useRunMaintenance();

  const columnViews = useMemo(() => getDbColumnViews(data?.columns ?? []), [data?.columns]);
  const indexViews = useMemo(() => getDbIndexViews(data?.indexes ?? []), [data?.indexes]);

  const runMaintenance = useCallback(
    (action: MaintenanceAction) => {
      const fire = () => {
        maintenanceMutation.mutate(
          { schema, table: name, request: { op: action.op } },
          {
            onSuccess: (res) => {
              notifications.show({
                title: `${action.label} completed`,
                message: `${schema}.${name}${res.executionTimeMs != null ? ` (${res.executionTimeMs} ms)` : ""}`,
                color: "green",
              });
            },
            onError: (err) => {
              notifications.show({
                title: `${action.label} failed`,
                message: err.message,
                color: "red",
              });
            },
          }
        );
      };

      if (action.heavy) {
        modals.openConfirmModal({
          title: `${action.label} ${schema}.${name}?`,
          children: (
            <span>
              {action.hint}. This takes an exclusive lock and may block reads and writes for large
              tables. Continue?
            </span>
          ),
          labels: { confirm: action.label, cancel: "Cancel" },
          confirmProps: { color: "red" },
          onConfirm: fire,
        });
      } else {
        fire();
      }
    },
    [maintenanceMutation, schema, name]
  );

  const handleDropIndex = useCallback(
    (indexName: string) => {
      modals.openConfirmModal({
        title: "Drop index",
        children: (
          <span>
            Drop index <strong>{indexName}</strong>? This cannot be undone.
          </span>
        ),
        labels: { confirm: "Drop index", cancel: "Cancel" },
        confirmProps: { color: "red" },
        onConfirm: () => {
          dropIndexMutation.mutate(
            { schema, indexName },
            {
              onSuccess: () =>
                notifications.show({
                  title: "Index dropped",
                  message: `${schema}.${indexName} removed`,
                  color: "green",
                }),
              onError: (err) =>
                notifications.show({
                  title: "Failed to drop index",
                  message: err.message,
                  color: "red",
                }),
            }
          );
        },
      });
    },
    [dropIndexMutation, schema]
  );

  const bloated = table.deadRatio != null && table.deadRatio >= 0.2;

  const stats: { label: string; value: string; accent?: boolean }[] = [
    { label: "Total size", value: table.totalSizeLabel ?? "—", accent: true },
    { label: "Table", value: table.tableSizeLabel ?? "—" },
    { label: "Indexes", value: table.indexesSizeLabel ?? "—" },
    {
      label: "Rows",
      value: table.rowEstimate != null ? table.rowEstimate.toLocaleString() : "—",
    },
    {
      label: "Dead rows",
      value:
        table.deadRows != null
          ? `${table.deadRows.toLocaleString()}${table.deadRatio != null ? ` · ${(table.deadRatio * 100).toFixed(1)}%` : ""}`
          : "—",
    },
    { label: "Vacuumed", value: table.lastVacuumLabel ?? "—" },
    { label: "Analyzed", value: table.lastAnalyzeLabel ?? "—" },
  ];

  return (
    <div className={classes.root}>
      <div className={classes.header}>
        <div className={classes.titleRow}>
          <span className={classes.titleIcon}>
            <Database size={16} />
          </span>
          <span className={classes.title}>
            {schema}.{name}
          </span>
          <span className={`${classes.chip} ${classes.chipNeutral}`}>{table.kind}</span>
          {bloated && (
            <span
              className={`${classes.chip} ${classes.chipWarn}`}
              title="High dead-tuple ratio — consider vacuum"
            >
              bloated
            </span>
          )}
        </div>
        {!table.isView && (
          <div className={classes.maintenanceBar}>
            {ACTIONS.map((action) => (
              <button
                key={action.op}
                type="button"
                title={action.hint}
                className={`${classes.actionBtn} ${action.heavy ? classes.actionBtnHeavy : ""}`}
                onClick={() => runMaintenance(action)}
                disabled={maintenanceMutation.isPending}
              >
                {action.op === "reindex" ? <RefreshCw size={13} /> : <Wand2 size={13} />}
                {action.label}
              </button>
            ))}
          </div>
        )}
      </div>

      <div className={classes.statGrid}>
        {stats.map((stat) => (
          <div
            key={stat.label}
            className={`${classes.statCard} ${stat.accent ? classes.statCardAccent : ""}`}
          >
            <span className={classes.statLabel}>{stat.label}</span>
            <span className={`${classes.statValue} ${stat.accent ? classes.statValueAccent : ""}`}>
              {stat.value}
            </span>
          </div>
        ))}
      </div>

      <div className={classes.scroll}>
        {isLoading && (
          <div className={classes.centeredLoader}>
            <Loader size="sm" />
          </div>
        )}

        {data && (
          <div className={classes.sections}>
            <section>
              <div className={classes.sectionHead}>
                <span className={classes.sectionTitle}>Columns</span>
                <span className={classes.sectionCount}>{data.columns.length}</span>
              </div>
              <table className={classes.colTable}>
                <thead>
                  <tr>
                    <th style={{ width: "48%" }}>Name</th>
                    <th style={{ width: "40%" }}>Type</th>
                    <th style={{ width: "12%" }}>Null</th>
                  </tr>
                </thead>
                <tbody>
                  {columnViews.map((column) => (
                    <tr key={column.id}>
                      <td className={classes.colName}>{column.name}</td>
                      <td className={classes.colType}>{column.dataType}</td>
                      <td>
                        {column.nullability === "required" ? (
                          <span className={classes.nnTag}>NN</span>
                        ) : null}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>

            <section>
              <div className={classes.sectionHead}>
                <span className={classes.sectionTitle}>Indexes</span>
                <span className={classes.sectionCount}>{data.indexes.length}</span>
              </div>
              {indexViews.length === 0 ? (
                <div className={classes.empty}>No indexes</div>
              ) : (
                <div className={classes.indexList}>
                  {indexViews.map((index) => (
                    <div key={index.id} className={classes.indexCard}>
                      <div className={classes.indexTop}>
                        <span className={classes.indexIcon}>
                          {index.isPrimary ? (
                            <Key size={14} />
                          ) : index.isUnique ? (
                            <Fingerprint size={14} />
                          ) : (
                            <Hash size={14} />
                          )}
                        </span>
                        <span className={classes.indexName}>{index.name}</span>
                        <span className={classes.indexBadges}>
                          <span className={`${classes.tag} ${classes.tagType}`}>
                            {index.indexType}
                          </span>
                          {index.isPrimary && (
                            <span className={`${classes.tag} ${classes.tagPk}`}>PK</span>
                          )}
                          {index.isUnique && !index.isPrimary && (
                            <span className={`${classes.tag} ${classes.tagUnique}`}>unique</span>
                          )}
                          {index.sizeLabel && (
                            <span className={`${classes.tag} ${classes.tagSize}`}>
                              {index.sizeLabel}
                            </span>
                          )}
                          <button
                            type="button"
                            className={classes.dropBtn}
                            title={index.isPrimary ? "Primary key can't be dropped" : "Drop index"}
                            disabled={index.isPrimary || dropIndexMutation.isPending}
                            onClick={() => handleDropIndex(index.name)}
                          >
                            <TrashBin size={13} />
                          </button>
                        </span>
                      </div>
                      <pre className={classes.indexDef}>{index.definition ?? index.columnList}</pre>
                    </div>
                  ))}
                </div>
              )}
            </section>
          </div>
        )}
      </div>
    </div>
  );
}
