import { IconActivity, IconClock, IconTable, IconX } from "@tabler/icons-react";
import { ActionIcon, Box, Tabs, Tooltip } from "@/shared/ui";
import { ActiveQueriesTab } from "./ActiveQueriesTab";
import { HistoryTab } from "./HistoryTab";
import { TablesTab } from "./TablesTab";

interface LeftPanelProps {
  activeTab: string;
  onTabChange: (tab: string) => void;
  onSelectQuery: (query: string) => void;
  onClose: () => void;
}

export function LeftPanel({ activeTab, onTabChange, onSelectQuery, onClose }: LeftPanelProps) {
  return (
    <Tabs
      value={activeTab}
      onChange={(v) => onTabChange(v ?? "history")}
      variant="outline"
      style={{ display: "flex", flexDirection: "column", height: "100%" }}
    >
      <Tabs.List grow style={{ flexShrink: 0 }}>
        <Tabs.Tab value="history" leftSection={<IconClock size={14} />}>
          History
        </Tabs.Tab>
        <Tabs.Tab value="tables" leftSection={<IconTable size={14} />}>
          Tables
        </Tabs.Tab>
        <Tabs.Tab value="queries" leftSection={<IconActivity size={14} />}>
          Queries
        </Tabs.Tab>
        <Tooltip label="Close sidebar (Ctrl+B)">
          <ActionIcon variant="subtle" size="xs" c="dimmed" onClick={onClose} ml="auto">
            <IconX size={14} />
          </ActionIcon>
        </Tooltip>
      </Tabs.List>

      <Box style={{ flex: 1, overflow: "hidden" }}>
        <Tabs.Panel value="history" style={{ height: "100%" }}>
          <HistoryTab onSelectQuery={onSelectQuery} />
        </Tabs.Panel>
        <Tabs.Panel value="tables" style={{ height: "100%" }}>
          <TablesTab />
        </Tabs.Panel>
        <Tabs.Panel value="queries" style={{ height: "100%" }}>
          <ActiveQueriesTab isActive={activeTab === "queries"} />
        </Tabs.Panel>
      </Box>
    </Tabs>
  );
}
