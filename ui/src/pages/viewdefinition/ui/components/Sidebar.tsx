import { Box, Text, Divider, Flex, Button } from "@/shared/ui";
import { IconCheck } from "@octofhir/ui-kit";
import type { ViewDefinition } from "../../lib/useViewDefinition";
import classes from "./Sidebar.module.css";

interface SidebarProps {
  items: ViewDefinition[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

export function Sidebar({ items, selectedId, onSelect }: SidebarProps) {
  return (
    <Box className={classes.sidebar}>
      <Box px="4" py="2">
        <Text variant="body-1" color="secondary" className={classes.title}>
          Saved Views
        </Text>
      </Box>
      <Divider />
      <Box className={classes.list} py="2">
        {items.length === 0 ? (
          <Box px="4" py="4">
            <Text variant="body-1" color="secondary" className={classes.empty}>
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
                className={classes.item}
                data-selected={selectedId === vd.id ? "true" : undefined}
                onClick={() => vd.id && onSelect(vd.id)}
              >
                {vd.status === "active" && (
                   <Button.Icon><IconCheck size={14} className={classes.activeIcon} /></Button.Icon>
                )}
                <span className={classes.itemLabel}>
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
