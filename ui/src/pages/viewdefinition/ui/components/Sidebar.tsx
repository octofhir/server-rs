import { Paper, Text, Divider, Stack, Button, Loader } from "@mantine/core";
import { IconCheck } from "@tabler/icons-react";
import type { ViewDefinition } from "../../lib/useViewDefinition";

interface SidebarProps {
  viewDefinitions: ViewDefinition[] | undefined;
  selectedId: string | null;
  isLoading: boolean;
  onSelect: (viewDef: ViewDefinition) => void;
}

export function Sidebar({ viewDefinitions, selectedId, isLoading, onSelect }: SidebarProps) {
  return (
    <Paper withBorder style={{ width: 220, height: "100%", display: "flex", flexDirection: "column" }}>
      <Text size="sm" fw={500} p="xs" c="dimmed">
        Saved Views
      </Text>
      <Divider />
      <Stack gap={0} p="xs" style={{ flex: 1, overflow: "auto" }}>
        {isLoading ? (
          <Loader size="sm" />
        ) : viewDefinitions?.length === 0 ? (
          <Text size="xs" c="dimmed" ta="center" py="md">
            No saved views
          </Text>
        ) : (
          viewDefinitions?.map((vd) => (
            <Button
              key={vd.id}
              variant={selectedId === vd.id ? "light" : "subtle"}
              size="xs"
              justify="flex-start"
              fullWidth
              onClick={() => onSelect(vd)}
              leftSection={
                vd.status === "active" ? (
                  <IconCheck size={12} color="green" />
                ) : null
              }
            >
              {vd.name || "Untitled"}
            </Button>
          ))
        )}
      </Stack>
    </Paper>
  );
}
