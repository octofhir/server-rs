import { IconSearch, IconTable } from "@tabler/icons-react";
import { useMemo, useState } from "react";
import { useDbTables } from "@/shared/api/hooks";
import { Badge, Box, Group, Loader, ScrollArea, Stack, Text, TextInput } from "@/shared/ui";
import { TableDetailView } from "./TableDetailView";

interface SelectedTable {
  schema: string;
  name: string;
}

function TableListItem({
  schema,
  name,
  tableType,
  rowEstimate,
  onClick,
}: {
  schema: string;
  name: string;
  tableType: string;
  rowEstimate?: number;
  onClick: () => void;
}) {
  return (
    <Box
      onClick={onClick}
      style={{
        padding: "6px 12px",
        cursor: "pointer",
        borderBottom: "1px solid var(--octo-border-subtle)",
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLElement).style.backgroundColor = "var(--octo-surface-2)";
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLElement).style.backgroundColor = "transparent";
      }}
    >
      <Group gap={6} wrap="nowrap">
        <IconTable size={12} style={{ flexShrink: 0, opacity: 0.4 }} />
        <Text size="xs" ff="monospace" truncate style={{ flex: 1 }}>
          {schema !== "public" ? `${schema}.` : ""}
          {name}
        </Text>
        {tableType === "VIEW" && (
          <Badge size="xs" variant="light" color="deep">
            view
          </Badge>
        )}
      </Group>
      {rowEstimate != null && rowEstimate > 0 && (
        <Text size="xs" c="dimmed" ml={18}>
          ~{rowEstimate.toLocaleString()} rows
        </Text>
      )}
    </Box>
  );
}

export function TablesTab() {
  const { data, isLoading } = useDbTables();
  const [selected, setSelected] = useState<SelectedTable | null>(null);
  const [search, setSearch] = useState("");

  const tables = data?.tables ?? [];

  const filtered = useMemo(() => {
    if (!search.trim()) return tables;
    const q = search.toLowerCase();
    return tables.filter(
      (t) => t.name.toLowerCase().includes(q) || t.schema.toLowerCase().includes(q)
    );
  }, [tables, search]);

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
          leftSection={<IconSearch size={14} />}
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
        {!isLoading && filtered.length === 0 && (
          <Text size="xs" c="dimmed" ta="center" py="xl">
            {search ? "No tables matching filter" : "No tables found"}
          </Text>
        )}
        {filtered.map((t) => (
          <TableListItem
            key={`${t.schema}.${t.name}`}
            schema={t.schema}
            name={t.name}
            tableType={t.tableType}
            rowEstimate={t.rowEstimate}
            onClick={() => setSelected({ schema: t.schema, name: t.name })}
          />
        ))}
      </ScrollArea>
    </Stack>
  );
}
