import { Box, Text, Divider, Flex, Button } from "@/shared/ui";
import { IconCheck } from "@octofhir/ui-kit";
import type { ViewDefinition } from "../../lib/useViewDefinition";

interface SidebarProps {
  items: ViewDefinition[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

export function Sidebar({ items, selectedId, onSelect }: SidebarProps) {
  return (
    <Box
      style={{
        width: 240,
        height: "100%",
        display: "flex",
        flexDirection: "column",
        borderRight: "1px solid var(--g-color-line-base)",
        backgroundColor: "var(--g-color-base-generic-ultralight)",
      }}
    >
      <Box px="4" py="2">
        <Text variant="body-1" color="secondary" style={{ fontWeight: 600 }}>
          Saved Views
        </Text>
      </Box>
      <Divider />
      <Box style={{ flex: 1, overflow: "auto" }} py="2">
        {items.length === 0 ? (
          <Box px="4" py="4">
            <Text variant="body-1" color="secondary" style={{ textAlign: "center" }}>
              No saved views
            </Text>
          </Box>
        ) : (
          <Flex direction="column" gap="1">
            {items.map((vd) => (
              <Button
                key={vd.id}
                view={selectedId === vd.id ? "flat-action" : "flat"}
                size="m"
                style={{
                  justifyContent: "flex-start",
                  margin: "0 8px",
                  borderRadius: "6px",
                  backgroundColor: selectedId === vd.id ? "var(--g-color-base-brand-light)" : undefined,
                }}
                onClick={() => vd.id && onSelect(vd.id)}
              >
                {vd.status === "active" && (
                   <Button.Icon><IconCheck size={14} style={{ color: "var(--g-color-text-success)" }} /></Button.Icon>
                )}
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {vd.name || "Untitled"}
                </span>
              </Button>
            ))}
          </Flex>
        )}
      </Box>
    </Box>
  );
}
