import { Badge, Text, TextInput } from "@octofhir/ui-kit";
import { PanelLeftClose, PanelLeftOpen, Search, Table2 } from "lucide-react";
import { useMemo, useState } from "react";
import type { ViewDefinition } from "../../lib/useViewDefinition";
import classes from "./Sidebar.module.css";

interface SidebarProps {
  items: ViewDefinition[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

const STATUS_COLOR: Record<string, string> = {
  active: "var(--octo-accent-primary, #10b981)",
  draft: "var(--octo-accent-warm, #f59e0b)",
  retired: "var(--octo-text-muted, #9aa1ad)",
  unknown: "var(--octo-text-muted, #9aa1ad)",
};

export function Sidebar({ items, selectedId, onSelect }: SidebarProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((vd) => (vd.name || "").toLowerCase().includes(q));
  }, [items, query]);

  if (collapsed) {
    return (
      <aside className={classes.rail}>
        <button
          type="button"
          className={classes.railBtn}
          onClick={() => setCollapsed(false)}
          aria-label="Expand saved views"
          title="Saved views"
        >
          <PanelLeftOpen size={16} />
        </button>
        <span className={classes.railCount}>{items.length}</span>
      </aside>
    );
  }

  return (
    <aside className={classes.sidebar}>
      <div className={classes.header}>
        <span className={classes.headTitle}>
          <Table2 size={14} />
          <Text size="sm" fw={600}>
            Saved views
          </Text>
          <Badge size="xs" variant="light">
            {items.length}
          </Badge>
        </span>
        <button
          type="button"
          className={classes.collapseBtn}
          onClick={() => setCollapsed(true)}
          aria-label="Collapse saved views"
        >
          <PanelLeftClose size={15} />
        </button>
      </div>

      <div className={classes.searchRow}>
        <TextInput
          size="xs"
          value={query}
          onChange={setQuery}
          placeholder="Filter views…"
          leftSection={<Search size={13} />}
          aria-label="Filter saved views"
        />
      </div>

      <div className={classes.list}>
        {filtered.length === 0 ? (
          <div className={classes.emptyState}>
            <Text size="xs" c="dimmed">
              {items.length === 0 ? "No saved views yet" : "No matches"}
            </Text>
          </div>
        ) : (
          filtered.map((vd) => (
            <button
              key={vd.id}
              type="button"
              className={classes.item}
              data-selected={selectedId === vd.id ? "true" : undefined}
              onClick={() => vd.id && onSelect(vd.id)}
            >
              <span
                className={classes.statusDot}
                style={{ background: STATUS_COLOR[vd.status] ?? STATUS_COLOR.unknown }}
              />
              <span className={classes.itemLabel}>{vd.name || "Untitled"}</span>
              <span className={classes.itemResource}>{vd.resource}</span>
            </button>
          ))
        )}
      </div>
    </aside>
  );
}
