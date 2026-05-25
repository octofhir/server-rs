import { ActionIcon, Badge, Button, Card, Drawer, Menu, Text, TextInput } from "@octofhir/ui-kit";
import {
  IconClock,
  IconDots,
  IconDownload,
  IconPin,
  IconPinFilled,
  IconSearch,
  IconTrash,
} from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { useState } from "react";
import { isHttpMethod } from "@/shared/api";
import type { HistoryEntry } from "../db/historyDatabase";
import { useHistory } from "../hooks/useHistory";
import { historyService } from "../services/historyService";
import { setBody, setMethod, setMode, setRawPath } from "../state/consoleStore";
import styles from "./HistoryPanel.module.css";

interface HistoryPanelProps {
  opened: boolean;
  onClose: () => void;
}

export function HistoryPanel({ opened, onClose }: HistoryPanelProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const { entries, togglePin, deleteEntry, clearAll } = useHistory();
  const {
    setRawPath: setRawPathEvent,
    setMethod: setMethodEvent,
    setBody: setBodyEvent,
    setMode: setModeEvent,
  } = useUnit({
    setRawPath,
    setMethod,
    setBody,
    setMode,
  });

  const filteredEntries = searchQuery
    ? entries.filter(
        (e) =>
          e.path.toLowerCase().includes(searchQuery.toLowerCase()) ||
          e.method.toLowerCase().includes(searchQuery.toLowerCase())
      )
    : entries;

  const handleRestore = (entry: HistoryEntry) => {
    // Always restore to "pro" mode (the only active mode now)
    setModeEvent("pro");
    setMethodEvent(isHttpMethod(entry.method) ? entry.method : "GET");
    setRawPathEvent(entry.path);
    if (entry.body) {
      setBodyEvent(entry.body);
    }
    onClose();
  };

  const handleExport = async () => {
    const json = await historyService.exportAll();
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `rest-console-history-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <Drawer open={opened} onOpenChange={(nextOpen) => !nextOpen && onClose()} placement="right">
      <div className={styles.root}>
        <h2 className={styles.title}>Request History</h2>

        {/* Search */}
        <TextInput
          placeholder="Search by path or method..."
          leftSection={<IconSearch size={16} />}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
        />

        {/* Actions */}
        <div className={styles.summary}>
          <Text size="sm" c="dimmed">
            {filteredEntries.length} entries
          </Text>
          <div className={styles.actions}>
            <Button
              size="xs"
              variant="light"
              onClick={handleExport}
              leftSection={<IconDownload size={14} />}
            >
              Export
            </Button>
            <Button size="xs" variant="light" color="fire" onClick={() => clearAll()}>
              Clear All
            </Button>
          </div>
        </div>

        {/* Entry list */}
        <div className={styles.list}>
          {filteredEntries.map((entry) => (
            <HistoryEntryCard
              key={entry.id}
              entry={entry}
              onRestore={handleRestore}
              onTogglePin={() => togglePin(entry.id)}
              onDelete={() => deleteEntry(entry.id)}
            />
          ))}
        </div>

        {filteredEntries.length === 0 && (
          <Text size="sm" c="dimmed" className={styles.empty}>
            No history entries
          </Text>
        )}
      </div>
    </Drawer>
  );
}

function HistoryEntryCard({
  entry,
  onRestore,
  onTogglePin,
  onDelete,
}: {
  entry: HistoryEntry;
  onRestore: (entry: HistoryEntry) => void;
  onTogglePin: () => void;
  onDelete: () => void;
}) {
  const isSuccess =
    entry.responseStatus && entry.responseStatus >= 200 && entry.responseStatus < 300;
  const isError = entry.responseStatus && entry.responseStatus >= 400;

  return (
    <Card
      p="sm"
      className={styles.card}
      onClick={() => onRestore(entry)}
    >
      <div className={styles.cardContent}>
        <div className={styles.cardHeader}>
          <div className={styles.badges}>
            <Badge size="sm" variant="light">
              {entry.method}
            </Badge>
            {entry.responseStatus && (
              <Badge size="sm" color={isSuccess ? "primary" : isError ? "fire" : "warm"}>
                {entry.responseStatus}
              </Badge>
            )}
          </div>

          <div className={styles.cardActions}>
            <ActionIcon
              size="sm"
              variant="subtle"
              onClick={(e) => {
                e.stopPropagation();
                onTogglePin();
              }}
            >
              {entry.isPinned ? <IconPinFilled size={14} /> : <IconPin size={14} />}
            </ActionIcon>

            <Menu position="bottom-end">
              <Menu.Target>
                <ActionIcon size="sm" variant="subtle" onClick={(e) => e.stopPropagation()}>
                  <IconDots size={14} />
                </ActionIcon>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Item
                  color="fire"
                  leftSection={<IconTrash size={14} />}
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete();
                  }}
                >
                  Delete
                </Menu.Item>
              </Menu.Dropdown>
            </Menu>
          </div>
        </div>

        <Text size="sm" truncate>
          {entry.path}
        </Text>

        <div className={styles.meta}>
          <div className={styles.metaItem}>
            <IconClock size={12} />
            <Text size="xs" c="dimmed">
              {new Date(entry.requestedAt).toLocaleString()}
            </Text>
          </div>

          {entry.responseDurationMs && (
            <Text size="xs" c="dimmed">
              {entry.responseDurationMs}ms
            </Text>
          )}
        </div>
      </div>
    </Card>
  );
}
