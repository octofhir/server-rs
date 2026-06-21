import { Search as Magnifier, ClipboardList as SquareListUl } from "lucide-react";
import { useMemo, useState } from "react";
import {
  filterDbSchemaTables,
  getDbSchemaTableViews,
} from "@/entities/db-schema";
import { useDbTables } from "@/shared/api/hooks";
import {
  Loader,
  RecordList,
  ScrollArea,
  Text,
  TextInput,
} from "@octofhir/ui-kit";
import { TableDetailView } from "./TableDetailView";
import classes from "../DbConsolePage.module.css";

interface SelectedTable {
  schema: string;
  name: string;
}

export function TablesTab() {
  const { data, isLoading } = useDbTables();
  const [selected, setSelected] = useState<SelectedTable | null>(null);
  const [search, setSearch] = useState("");

  const tables = data?.tables ?? [];

  const filtered = useMemo(() => {
    return filterDbSchemaTables(tables, search);
  }, [tables, search]);

  const tableViews = useMemo(() => getDbSchemaTableViews(filtered), [filtered]);

  const tableItems = useMemo(
    () =>
      tableViews.map((table) => ({
        id: table.id,
        title: table.displayName,
        subtitle: table.kind,
        description: table.rowEstimateLabel,
        leading: <SquareListUl size={14} />,
        meta:
          table.isView
            ? [{ id: "view", label: "view", tone: "info" as const }]
            : undefined,
      })),
    [tableViews],
  );

  if (selected) {
    return (
      <TableDetailView
        schema={selected.schema}
        table={selected.name}
        onBack={() => setSelected(null)}
      />
    );
  }

  return (
    <div className={classes.sideTabRoot}>
      <div className={classes.sideTabSearch}>
        <TextInput
          size="xs"
          placeholder="Search tables..."
          leftSection={<Magnifier size={14} />}
          value={search}
          onChange={(e) => setSearch(e.currentTarget.value)}
        />
      </div>
      <div className={classes.sideTabCount}>
        <Text size="xs" c="dimmed">
          {filtered.length}
          {search ? ` / ${tables.length}` : ""} tables
        </Text>
      </div>
      <ScrollArea className={classes.sideTabScroll}>
        {isLoading && (
          <div className={classes.centeredLoader}>
            <Loader size="sm" />
          </div>
        )}
        {!isLoading && (
          <div className={classes.sideTabPadding}>
            <RecordList
              density="compact"
              items={tableItems}
              emptyText={search ? "No tables matching filter" : "No tables found"}
              onSelect={(item) => {
                const table = tableViews.find((candidate) => candidate.id === item.id);
                if (table) {
                  setSelected({ schema: table.schema, name: table.name });
                }
              }}
            />
          </div>
        )}
      </ScrollArea>
    </div>
  );
}
