import {
  Badge,
  DataTable,
  type DataTableColumn,
  EmptyState,
  PageHeader,
  Text,
  TextInput,
} from "@octofhir/ui-kit";
import { Database, RefreshCw, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { getDbSchemaTableViews } from "@/entities/db-schema";
import type { DbSchemaTableView } from "@/entities/db-schema";
import { useDbTables, useResourceTypesCategorized } from "@/shared/api/hooks";
import { TableMetaDetail } from "./components/TableMetaDetail";
import classes from "./DatabasePage.module.css";

type TableCategory = "fhir" | "custom" | "system";
type ViewMode = "all" | TableCategory;

function isViewMode(value: string): value is ViewMode {
  return value === "all" || value === "fhir" || value === "custom" || value === "system";
}

/**
 * Classify a table by origin using the resource-type category map:
 * - FHIR: a base-spec resource type
 * - Custom: a resource type contributed by an installed/system IG
 * - System: infra tables (auth, canonical manager, migrations, `_`-prefixed)
 */
function categorize(
  view: DbSchemaTableView,
  categoryMap: Map<string, TableCategory>
): TableCategory {
  if (view.schema !== "public") return "system";
  if (view.name.startsWith("_")) return "system";
  const base = view.name.replace(/_history$/, "");
  return categoryMap.get(base) ?? "system";
}

export function DatabasePage() {
  const { data, isLoading, refetch, isFetching } = useDbTables();
  const { data: categorized } = useResourceTypesCategorized();
  const [search, setSearch] = useState("");
  const [viewMode, setViewMode] = useState<ViewMode>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const allViews = useMemo(() => getDbSchemaTableViews(data?.tables ?? []), [data?.tables]);

  const categoryMap = useMemo(() => {
    const map = new Map<string, TableCategory>();
    for (const t of categorized?.types ?? []) {
      // Resource table names are the lowercased resource type.
      map.set(
        t.name.toLowerCase(),
        t.category === "custom" ? "custom" : t.category === "fhir" ? "fhir" : "system"
      );
    }
    return map;
  }, [categorized]);

  const categoryById = useMemo(() => {
    const map = new Map<string, TableCategory>();
    for (const v of allViews) map.set(v.id, categorize(v, categoryMap));
    return map;
  }, [allViews, categoryMap]);

  const counts = useMemo(() => {
    const c = { all: allViews.length, fhir: 0, custom: 0, system: 0 };
    for (const v of allViews) c[categoryById.get(v.id) ?? "system"] += 1;
    return c;
  }, [allViews, categoryById]);

  const rows = useMemo(() => {
    const q = search.trim().toLowerCase();
    const filtered = allViews.filter((v) => {
      if (viewMode !== "all" && categoryById.get(v.id) !== viewMode) return false;
      if (!q) return true;
      return v.name.toLowerCase().includes(q) || v.schema.toLowerCase().includes(q);
    });
    // Default order: largest tables first.
    return filtered.sort((a, b) => (b.totalSizeBytes ?? -1) - (a.totalSizeBytes ?? -1));
  }, [allViews, viewMode, search, categoryById]);

  const selected = useMemo(
    () => allViews.find((v) => v.id === selectedId) ?? null,
    [allViews, selectedId]
  );

  // Stable ref — passing a fresh array every render makes react-table loop.
  const selectedRowIds = useMemo(() => (selectedId ? [selectedId] : []), [selectedId]);

  const columns: DataTableColumn<DbSchemaTableView>[] = useMemo(
    () => [
      {
        id: "name",
        header: "Table",
        sortable: true,
        width: "46%",
        accessor: (row) => row.displayName,
        cell: (row) => (
          <div className={classes.nameCell}>
            <Text size="xs" ff="monospace" fw={500} className={classes.nameText}>
              {row.displayName}
            </Text>
            {row.isView && (
              <Badge size="xs" variant="light" color="deep">
                view
              </Badge>
            )}
          </div>
        ),
      },
      {
        id: "rows",
        header: "Rows",
        sortable: true,
        align: "right",
        width: "13%",
        accessor: (row) => row.rowEstimate ?? -1,
        cell: (row) => (
          <Text size="xs" c="dimmed">
            {row.rowEstimate != null ? row.rowEstimate.toLocaleString() : "—"}
          </Text>
        ),
      },
      {
        id: "dead",
        header: "Dead",
        sortable: true,
        align: "right",
        width: "12%",
        accessor: (row) => row.deadRatio ?? -1,
        cell: (row) =>
          row.deadRatio != null ? (
            <Text size="xs" c={row.deadRatio >= 0.2 ? "orange" : "dimmed"}>
              {(row.deadRatio * 100).toFixed(1)}%
            </Text>
          ) : (
            <Text size="xs" c="dimmed">
              —
            </Text>
          ),
      },
      {
        id: "totalSize",
        header: "Total size",
        sortable: true,
        align: "right",
        width: "15%",
        accessor: (row) => row.totalSizeBytes ?? -1,
        cell: (row) => (
          <Text size="xs" fw={500}>
            {row.totalSizeLabel ?? "—"}
          </Text>
        ),
      },
      {
        id: "vacuum",
        header: "Vacuumed",
        sortable: false,
        align: "right",
        width: "14%",
        cell: (row) => (
          <Text size="xs" c="dimmed">
            {row.lastVacuumLabel ?? "—"}
          </Text>
        ),
      },
    ],
    []
  );

  return (
    <div className={`${classes.container} page-enter`}>
      <PageHeader
        eyebrow="Maintenance"
        title="Database"
        description="Inspect table sizes, indexes and statistics. Run vacuum, analyze and reindex."
        actions={[
          {
            id: "refresh",
            label: isFetching ? "Refreshing…" : "Refresh",
            icon: <RefreshCw size={14} />,
            variant: "light",
            onClick: () => void refetch(),
          },
        ]}
      />

      <div className={classes.body}>
        <div className={classes.listPanel}>
          <div className={classes.listControls}>
            <TextInput
              size="sm"
              placeholder="Search tables…"
              leftSection={<Search size={14} />}
              value={search}
              onChange={(value) => setSearch(value)}
            />
            <div className={classes.tabs} role="tablist" aria-label="Table category">
              {(
                [
                  { value: "all", label: "All", count: counts.all },
                  { value: "fhir", label: "FHIR", count: counts.fhir },
                  { value: "custom", label: "Custom", count: counts.custom },
                  { value: "system", label: "System", count: counts.system },
                ] as const
              ).map((tab) => (
                <button
                  key={tab.value}
                  type="button"
                  role="tab"
                  aria-selected={viewMode === tab.value}
                  className={classes.tab}
                  data-active={viewMode === tab.value || undefined}
                  onClick={() => isViewMode(tab.value) && setViewMode(tab.value)}
                >
                  {tab.label}
                  <span className={classes.tabCount}>{tab.count}</span>
                </button>
              ))}
            </div>
          </div>
          <div className={classes.tableWrap}>
            <DataTable
              data={rows}
              columns={columns}
              getRowId={(row) => row.id}
              size="sm"
              fillHeight
              paginated
              pageSize={40}
              highlightOnHover
              stickyHeader
              loading={isLoading}
              onRowClick={(row) => setSelectedId(row.id)}
              selectedRowIds={selectedRowIds}
              emptyState={
                <EmptyState title="No tables" description="Nothing matches the current filters." />
              }
              aria-label="Database tables"
            />
          </div>
        </div>

        <div className={classes.detailPanel}>
          {selected ? (
            <TableMetaDetail key={selected.id} table={selected} />
          ) : (
            <div className={classes.detailEmpty}>
              <EmptyState
                image={<Database size={28} />}
                title="Select a table"
                description="Pick a table to view its columns, indexes, sizes and maintenance actions."
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
