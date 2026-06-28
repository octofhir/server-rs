import { Drawer, ScrollArea, Text } from "@octofhir/ui-kit";
import { Clock } from "lucide-react";
import classes from "../DbConsolePage.module.css";
import type { StreamEntry } from "../types";

interface HistoryDrawerProps {
  open: boolean;
  onClose: () => void;
  entries: StreamEntry[];
  activeId: string | null;
  onSelect: (entry: StreamEntry) => void;
}

function formatTime(date: Date): string {
  return date.toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function HistoryDrawer({ open, onClose, entries, activeId, onSelect }: HistoryDrawerProps) {
  // Newest first.
  const ordered = [...entries].reverse();

  return (
    <Drawer open={open} onClose={onClose} title="Query history" size={420}>
      <ScrollArea className={classes.historyScroll}>
        {ordered.length === 0 ? (
          <div className={classes.historyEmpty}>
            <Clock size={28} strokeWidth={1.2} />
            <Text size="sm" c="dimmed">
              No queries yet
            </Text>
          </div>
        ) : (
          <div className={classes.historyList}>
            {ordered.map((entry) => (
              <button
                key={entry.id}
                type="button"
                className={[
                  classes.historyRow,
                  entry.id === activeId ? classes.historyRowActive : undefined,
                ]
                  .filter(Boolean)
                  .join(" ")}
                onClick={() => {
                  onSelect(entry);
                  onClose();
                }}
              >
                <span
                  className={[
                    classes.historyDot,
                    entry.status === "error"
                      ? classes.historyDotError
                      : entry.status === "pending"
                        ? classes.historyDotPending
                        : classes.historyDotOk,
                  ].join(" ")}
                />
                <span className={classes.historyMain}>
                  <span className={classes.historyQuery}>{entry.query}</span>
                  <span className={classes.historyMetaRow}>
                    <span>{formatTime(entry.timestamp)}</span>
                    {entry.result && <span>· {entry.result.rowCount} rows</span>}
                    {entry.executionTimeMs != null && <span>· {entry.executionTimeMs}ms</span>}
                    {entry.fromHistory && <span>· saved</span>}
                  </span>
                </span>
              </button>
            ))}
          </div>
        )}
      </ScrollArea>
    </Drawer>
  );
}
