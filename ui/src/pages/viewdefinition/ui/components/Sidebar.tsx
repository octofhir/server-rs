import { Badge, Text, TextInput } from "@octofhir/ui-kit";
import { Database, PanelLeftClose, PanelLeftOpen, Search, Table2 } from "lucide-react";
import { useMemo, useState } from "react";
import classes from "./Sidebar.module.css";

/** A saved ViewDefinition or SQL query (SQLQuery / SQLView Library). */
export interface SidebarItem {
  key: string;
  kind: "view" | "sql";
  id: string;
  name: string;
  status: string;
  /** Resource type of a view, profile of a query. */
  meta: string;
}

interface SidebarProps {
  items: SidebarItem[];
  selectedKey: string | null;
  onSelect: (item: SidebarItem) => void;
}

const STATUS_COLOR: Record<string, string> = {
  active: "var(--octo-accent-primary)",
  draft: "var(--octo-border-strong)",
  retired: "var(--octo-text-muted)",
  unknown: "var(--octo-text-muted)",
};

const GROUPS = [
  { kind: "view", title: "Views", icon: Table2 },
  { kind: "sql", title: "SQL queries", icon: Database },
] as const;

export function Sidebar({ items, selectedKey, onSelect }: SidebarProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((item) => (item.name || "").toLowerCase().includes(q));
  }, [items, query]);

  if (collapsed) {
    return (
      <aside className={classes.rail}>
        <button
          type="button"
          className={classes.railBtn}
          onClick={() => setCollapsed(false)}
          aria-label="Expand saved views and queries"
          title="Saved"
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
            Saved
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
          placeholder="Filter saved…"
          leftSection={<Search size={13} />}
          aria-label="Filter saved views"
        />
      </div>

      <div className={classes.list}>
        {filtered.length === 0 ? (
          <div className={classes.emptyState}>
            <Text size="xs" c="dimmed">
              {items.length === 0 ? "Nothing saved yet" : "No matches"}
            </Text>
          </div>
        ) : (
          GROUPS.map((group) => {
            const members = filtered.filter((item) => item.kind === group.kind);
            if (members.length === 0) return null;
            const Icon = group.icon;
            return (
              <div key={group.kind} className={classes.group}>
                <span className={classes.groupTitle}>
                  <Icon size={12} />
                  {group.title}
                </span>
                {members.map((item) => (
                  <button
                    key={item.key}
                    type="button"
                    className={classes.item}
                    data-selected={selectedKey === item.key ? "true" : undefined}
                    onClick={() => onSelect(item)}
                  >
                    <span
                      className={classes.statusDot}
                      style={{ background: STATUS_COLOR[item.status] ?? STATUS_COLOR.unknown }}
                    />
                    <span className={classes.itemLabel}>{item.name || "Untitled"}</span>
                    <span className={classes.itemResource}>{item.meta}</span>
                  </button>
                ))}
              </div>
            );
          })
        )}
      </div>
    </aside>
  );
}
