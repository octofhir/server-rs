import { Drawer } from "@/shared/ui";
import {
  ActionIcon,
  Badge,
  Button,
  Card,
  Group,
  Menu,
  Stack,
  Text,
  TextInput,
  Tooltip,
} from "@/shared/ui";
import {
  IconClock,
  IconDots,
  IconDownload,
  IconPin,
  IconPinFilled,
  IconSearch,
  IconTrash,
} from "@tabler/icons-react";
import { useUnit } from "effector-react";
import { useState } from "react";
import type { HistoryEntry } from "../db/historyDatabase";
import { useHistory } from "../hooks/useHistory";
import { historyService } from "../services/historyService";
import { setBody, setMethod, setMode, setRawPath } from "../state/consoleStore";

interface HistoryPanelProps {
  opened: boolean;
  onClose: () => void;
}

export function HistoryPanel({ opened, onClose }: HistoryPanelProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const { entries, isLoading, togglePin, deleteEntry, clearAll } = useHistory();
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
    setMethodEvent(entry.method as any);
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
    <Drawer opened={opened} onClose={onClose} title="Request History" position="right" size="lg">
      <Stack gap="md">
        {/* Search */}
        <TextInput
          placeholder="Search by path or method..."
          leftSection={<IconSearch size={16} />}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
        />

        {/* Actions */}
        <Group justify="space-between">
          <Text size="sm" c="dimmed">
            {filteredEntries.length} entries
          </Text>
          <Group gap="xs">
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
          </Group>
        </Group>

        {/* Entry list */}
        <Stack gap="xs">
          {filteredEntries.map((entry) => (
            <HistoryEntryCard
              key={entry.id}
              entry={entry}
              onRestore={handleRestore}
              onTogglePin={() => togglePin(entry.id)}
              onDelete={() => deleteEntry(entry.id)}
            />
          ))}
        </Stack>

        {filteredEntries.length === 0 && (
          <Text size="sm" c="dimmed" ta="center" py="xl">
            No history entries
          </Text>
        )}
      </Stack>
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
      style={{ cursor: "pointer", backgroundColor: "var(--octo-surface-1)" }}
      onClick={() => onRestore(entry)}
    >
      <Stack gap="xs">
        <Group justify="space-between">
          <Group gap="xs">
            <Badge size="sm" variant="light">
              {entry.method}
            </Badge>
            {entry.responseStatus && (
              <Badge size="sm" color={isSuccess ? "primary" : isError ? "fire" : "warm"}>
                {entry.responseStatus}
              </Badge>
            )}
          </Group>

          <Group gap="xs">
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
          </Group>
        </Group>

        <Text size="sm" truncate>
          {entry.path}
        </Text>

        <Group gap="xs">
          <Group gap={4}>
            <IconClock size={12} />
            <Text size="xs" c="dimmed">
              {new Date(entry.requestedAt).toLocaleString()}
            </Text>
          </Group>

          {entry.responseDurationMs && (
            <Text size="xs" c="dimmed">
              {entry.responseDurationMs}ms
            </Text>
          )}
        </Group>
      </Stack>
    </Card>
  );
}
