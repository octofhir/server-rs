import { ActionIcon, Tabs, Tooltip } from "@octofhir/ui-kit";
import { Pulse, Clock, SquareListUl, Xmark } from "@gravity-ui/icons";
import { ActiveQueriesTab } from "./ActiveQueriesTab";
import { HistoryTab } from "./HistoryTab";
import { TablesTab } from "./TablesTab";
import classes from "../DbConsolePage.module.css";

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
      className={classes.leftTabs}
    >
      <Tabs.List grow className={classes.leftTabsList}>
        <Tabs.Tab value="history" leftSection={<Clock size={14} />}>
          History
        </Tabs.Tab>
        <Tabs.Tab value="tables" leftSection={<SquareListUl size={14} />}>
          Tables
        </Tabs.Tab>
        <Tabs.Tab value="queries" leftSection={<Pulse size={14} />}>
          Queries
        </Tabs.Tab>
        <Tooltip label="Close sidebar (Ctrl+B)">
          <ActionIcon variant="subtle" size="xs" c="dimmed" onClick={onClose} ml="auto">
            <Xmark size={14} />
          </ActionIcon>
        </Tooltip>
      </Tabs.List>

      <div className={classes.leftTabsBody}>
        <Tabs.Panel value="history" className={classes.leftTabsPanel}>
          <HistoryTab onSelectQuery={onSelectQuery} />
        </Tabs.Panel>
        <Tabs.Panel value="tables" className={classes.leftTabsPanel}>
          <TablesTab />
        </Tabs.Panel>
        <Tabs.Panel value="queries" className={classes.leftTabsPanel}>
          <ActiveQueriesTab isActive={activeTab === "queries"} />
        </Tabs.Panel>
      </div>
    </Tabs>
  );
}
