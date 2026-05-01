import { Magnifier, SquareListUl } from "@gravity-ui/icons";
import { useMemo, useState } from "react";
import {
  filterDbSchemaTables,
  getDbSchemaTableViews,
} from "@/entities/db-schema";
import { useDbTables } from "@/shared/api/hooks";
import {
  Box,
  Group,
  Loader,
  RecordList,
  ScrollArea,
  Stack,
  Text,
  TextInput,
} from "@/shared/ui";
import { TableDetailView } from "./TableDetailView";

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
    <Stack gap={0} h="100%">
      <Box px="xs" py={6} style={{ flexShrink: 0 }}>
        <TextInput
          size="xs"
          placeholder="Search tables..."
          leftSection={<Magnifier size={14} />}
          value={search}
          onChange={(e) => setSearch(e.currentTarget.value)}
        />
      </Box>
      <Group gap={4} px="sm" pb={4} style={{ flexShrink: 0 }}>
        <Text size="xs" c="dimmed">
          {filtered.length}
          {search ? ` / ${tables.length}` : ""} tables
        </Text>
      </Group>
      <ScrollArea style={{ flex: 1 }}>
        {isLoading && (
          <Box ta="center" py="xl">
            <Loader size="sm" />
          </Box>
        )}
        {!isLoading && (
          <Box p="xs">
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
          </Box>
        )}
      </ScrollArea>
    </Stack>
  );
}
