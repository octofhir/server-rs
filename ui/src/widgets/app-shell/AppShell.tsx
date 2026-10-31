import {
  ActionIcon,
  CommandPalette,
  type CommandPaletteItem,
  LocationBar,
  type LocationSegment,
  Shell,
  Sidebar,
  type SidebarNavGroup,
  type SidebarStatus,
  useColorScheme,
} from "@octofhir/ui-kit";
import {
  Activity,
  Boxes,
  Code2,
  Cpu,
  Database,
  FileCode2,
  FileText,
  FolderTree,
  Globe,
  KeyRound,
  LayoutGrid,
  LogOut,
  Monitor,
  Moon,
  NotebookPen,
  Receipt,
  Send,
  Settings,
  Shield,
  ShieldCheck,
  Sigma,
  SquareFunction,
  Sun,
  Table,
  Terminal,
  Users,
} from "lucide-react";
import { useUnit } from "effector-react";
import { type ComponentType, useCallback, useEffect, useMemo, useState } from "react";
import { Helmet } from "react-helmet-async";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { useAuth, useHealth, useResourceTypes, useSettings } from "@/shared/api/hooks";
import {
  $paletteContributions,
  $paletteOpen,
  $recentPlaces,
  paletteOpenChanged,
  placeVisited,
} from "@/shared/command-palette";
import { isTypingTarget } from "@/shared/lib/keyboard";
import { ErrorBoundary } from "@/shared/ui";
import { ShortcutsDialog } from "./ShortcutsDialog";

const BRANDING = `${import.meta.env.BASE_URL}branding`;
const SIDEBAR_KEY = "octofhir-sidebar";

const statusThemeByHealth: Record<"ok" | "degraded" | "down", SidebarStatus["theme"]> = {
  ok: "success",
  degraded: "warning",
  down: "danger",
};

interface NavEntry {
  id: string;
  label: string;
  icon: ComponentType<{ size?: number }>;
  path: string;
  /** Extra path prefixes that also mark this entry active. */
  aliases?: string[];
  /** Second key of a `g <key>` go-to shortcut. */
  goKey?: string;
}

interface NavSection {
  id: string;
  label: string;
  items: NavEntry[];
}

const NAV: NavSection[] = [
  {
    id: "data",
    label: "Data",
    items: [
      { id: "resources", label: "Resources", icon: FolderTree, path: "/resources", goKey: "r" },
      { id: "requests", label: "Requests", icon: Send, path: "/console", goKey: "e" },
      { id: "packages", label: "Packages", icon: Boxes, path: "/packages", goKey: "p" },
      { id: "capabilities", label: "Capabilities", icon: FileCode2, path: "/metadata", goKey: "c" },
    ],
  },
  {
    id: "query",
    label: "Query",
    items: [
      { id: "sql", label: "SQL", icon: Terminal, path: "/db-console", goKey: "s" },
      { id: "graphql", label: "GraphQL", icon: Code2, path: "/graphql", goKey: "g" },
      { id: "fhirpath", label: "FHIRPath", icon: Activity, path: "/fhirpath", goKey: "f" },
      { id: "cql", label: "CQL", icon: Sigma, path: "/cql", goKey: "q" },
      { id: "views", label: "Views", icon: Table, path: "/viewdefinition", goKey: "v" },
      { id: "notebooks", label: "Notebooks", icon: NotebookPen, path: "/notebooks", goKey: "n" },
    ],
  },
  {
    id: "access",
    label: "Access",
    items: [
      { id: "users", label: "Users", icon: Users, path: "/auth/users", aliases: ["/users"], goKey: "u" },
      { id: "roles", label: "Roles", icon: ShieldCheck, path: "/auth/roles" },
      { id: "policies", label: "Policies", icon: Shield, path: "/auth/policies", aliases: ["/access-policies"], goKey: "y" },
      { id: "clients", label: "Clients", icon: KeyRound, path: "/auth/clients", aliases: ["/clients"] },
      { id: "sessions", label: "Sessions", icon: Monitor, path: "/auth/sessions" },
      { id: "providers", label: "Identity providers", icon: Globe, path: "/auth/providers" },
    ],
  },
  {
    id: "system",
    label: "System",
    items: [
      { id: "operations", label: "Operations", icon: SquareFunction, path: "/operations", goKey: "o" },
      { id: "apps", label: "Apps", icon: LayoutGrid, path: "/apps" },
      { id: "automations", label: "Automations", icon: Cpu, path: "/automations" },
      { id: "audit", label: "Audit", icon: Receipt, path: "/audit", goKey: "a" },
      { id: "logs", label: "Logs", icon: FileText, path: "/logs", goKey: "l" },
      { id: "database", label: "Database", icon: Database, path: "/database" },
      { id: "settings", label: "Settings", icon: Settings, path: "/settings", goKey: "," },
    ],
  },
];

function isActive(entry: NavEntry, path: string): boolean {
  return [entry.path, ...(entry.aliases ?? [])].some(
    (prefix) => path === prefix || path.startsWith(`${prefix}/`)
  );
}

function findActive(path: string): { section: NavSection; entry: NavEntry } | null {
  for (const section of NAV) {
    const entry = section.items.find((item) => isActive(item, path));
    if (entry) return { section, entry };
  }
  return null;
}

/**
 * Location as a REST-like path: `fhir / Patient / id` for resources, otherwise
 * `section / Page / …rest` so the header always reads as an address.
 */
function locationSegments(path: string, navigate: (to: string) => void): LocationSegment[] {
  const decoded = path.split("/").filter(Boolean).map(decodeURIComponent);
  if (decoded[0] === "resources") {
    const [, type, id] = decoded;
    return [
      { id: "fhir", label: "fhir", onClick: () => navigate("/resources") },
      ...(type ? [{ id: "type", label: type, onClick: () => navigate(`/resources/${type}`) }] : []),
      ...(id ? [{ id: "id", label: id }] : []),
    ];
  }
  const active = findActive(path);
  if (!active) return [{ id: "overview", label: "overview" }];
  const rest = path.slice(active.entry.path.length).split("/").filter(Boolean).map(decodeURIComponent);
  return [
    { id: "section", label: active.section.label.toLowerCase() },
    {
      id: "page",
      label: active.entry.label,
      onClick: rest.length ? () => navigate(active.entry.path) : undefined,
    },
    ...rest.map((part, index) => ({ id: `rest-${index}`, label: part })),
  ];
}

/** Readable name of a place for the recent list; null for places not worth remembering. */
function placeLabel(path: string): string | null {
  const decoded = path.split("/").filter(Boolean).map(decodeURIComponent);
  if (decoded[0] === "resources") return decoded.length > 1 ? decoded.slice(1).join("/") : null;
  const active = findActive(path);
  if (!active) return null;
  const rest = path.slice(active.entry.path.length).split("/").filter(Boolean).map(decodeURIComponent);
  return [active.entry.label, ...rest].join(" / ");
}

function placeIcon(path: string): ComponentType<{ size?: number }> {
  if (path.startsWith("/resources/")) return path.split("/").length > 3 ? FileText : FolderTree;
  return findActive(path.split("?")[0])?.entry.icon ?? FileText;
}

const RESOURCE_ADDRESS = /^([A-Za-z]+)\/([A-Za-z0-9\-.]{1,64})$/;
const RESOURCE_SEARCH = /^([A-Za-z]+)\?(.+)$/;

/** `Patient/123` opens that resource; `Patient?name=anna` runs the search in the browser. */
function resourceQueryItems(
  query: string,
  types: string[],
  navigate: (to: string) => void
): CommandPaletteItem[] {
  const canonical = (name: string) => types.find((type) => type.toLowerCase() === name.toLowerCase());
  const address = RESOURCE_ADDRESS.exec(query);
  const addressType = address && canonical(address[1]);
  if (address && addressType) {
    return [
      {
        id: "open-address",
        label: `Open ${addressType}/${address[2]}`,
        icon: FileText,
        hint: "↵",
        onSelect: () => navigate(`/resources/${addressType}/${encodeURIComponent(address[2])}`),
      },
    ];
  }
  const search = RESOURCE_SEARCH.exec(query);
  const searchType = search && canonical(search[1]);
  if (search && searchType) {
    return [
      {
        id: "search-type",
        label: `Search ${searchType}?${search[2]}`,
        icon: FolderTree,
        hint: "↵",
        onSelect: () =>
          navigate(`/resources/${searchType}?${new URLSearchParams({ q: search[2] }).toString()}`),
      },
    ];
  }
  return [];
}

/** Top-level navigation and layout for the console: panel rail, path header, ⌘K palette. */
export function AppShell() {
  const location = useLocation();
  const navigate = useNavigate();
  const { logout, user } = useAuth();
  const { colorScheme, toggleColorScheme } = useColorScheme();
  const { data: health } = useHealth();
  const { data: resourceTypes } = useResourceTypes();
  useSettings();

  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    try {
      return window.localStorage.getItem(SIDEBAR_KEY) === "1";
    } catch {
      return false;
    }
  });
  const [paletteOpen, setPaletteOpen, contributions, recentPlaces, visit] = useUnit([
    $paletteOpen,
    paletteOpenChanged,
    $paletteContributions,
    $recentPlaces,
    placeVisited,
  ]);

  const path = location.pathname;
  const placePath = `${path}${location.search}`;

  useEffect(() => {
    const label = placeLabel(path);
    const query = new URLSearchParams(location.search).get("q");
    if (label) visit({ path: placePath, label: query ? `${label}?${query}` : label });
  }, [path, placePath, location.search, visit]);

  const groups = useMemo<SidebarNavGroup[]>(
    () =>
      NAV.map((section) => ({
        id: section.id,
        label: section.label,
        items: section.items.map((entry) => ({
          id: entry.id,
          label: entry.label,
          icon: entry.icon,
          active: isActive(entry, path),
          onClick: () => navigate(entry.path),
        })),
      })),
    [path, navigate]
  );

  const paletteItems = useMemo<CommandPaletteItem[]>(() => {
    const pages = NAV.flatMap((section) =>
      section.items.map((entry) => ({
        id: `go-${entry.id}`,
        label: entry.label,
        group: section.label,
        icon: entry.icon,
        hint: entry.goKey ? `g ${entry.goKey}` : entry.path,
        keywords: [section.label],
        onSelect: () => navigate(entry.path),
      }))
    );
    const types = (resourceTypes ?? []).map((type) => ({
      id: `type-${type}`,
      label: type,
      group: "Resource types",
      icon: FolderTree,
      hint: `fhir/${type}`,
      keywords: ["resource", "fhir"],
      onSelect: () => navigate(`/resources/${type}`),
    }));
    const actions: CommandPaletteItem[] = [
      {
        id: "toggle-theme",
        label: colorScheme === "dark" ? "Switch to light theme" : "Switch to dark theme",
        group: "Actions",
        icon: colorScheme === "dark" ? Sun : Moon,
        keywords: ["theme", "dark", "light"],
        onSelect: toggleColorScheme,
      },
      { id: "sign-out", label: "Sign out", group: "Actions", icon: LogOut, onSelect: logout },
    ];
    // Commands from the current page come first: they are the most specific.
    const pageItems = Object.values(contributions).flat();
    return [...pageItems, ...pages, ...actions, ...types];
  }, [navigate, resourceTypes, colorScheme, toggleColorScheme, logout, contributions]);

  const recentItems = useMemo<CommandPaletteItem[]>(
    () =>
      recentPlaces
        .filter((place) => place.path !== placePath)
        .map((place) => ({
          id: `recent-${place.path}`,
          label: place.label,
          group: "Recent",
          icon: placeIcon(place.path),
          onSelect: () => navigate(place.path),
        })),
    [recentPlaces, placePath, navigate]
  );

  const queryItems = useCallback(
    (query: string) => resourceQueryItems(query, resourceTypes ?? [], navigate),
    [resourceTypes, navigate]
  );

  // Capture phase: Monaco-based inputs (REST URL bar, SQL editor) treat ⌘K as a chord
  // prefix and stop the event before it would bubble to a document listener.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        event.stopPropagation();
        setPaletteOpen(true);
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [setPaletteOpen]);

  // Linear-style navigation: `g` then a letter jumps to a page; `?` lists the shortcuts.
  useEffect(() => {
    const goTargets = new Map(
      NAV.flatMap((section) => section.items).flatMap((entry) =>
        entry.goKey ? [[entry.goKey, entry.path] as const] : []
      )
    );
    goTargets.set("h", "/");
    let pendingGo = 0;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey || isTypingTarget(event.target)) return;
      if (event.key === "?") {
        event.preventDefault();
        setShortcutsOpen(true);
        return;
      }
      if (event.key === "[") {
        event.preventDefault();
        setSidebarCollapsed((collapsed) => !collapsed);
        return;
      }
      if (pendingGo && Date.now() - pendingGo < 1200) {
        pendingGo = 0;
        const target = goTargets.get(event.key.toLowerCase());
        if (target) {
          event.preventDefault();
          navigate(target);
        }
        return;
      }
      if (event.key === "g") pendingGo = Date.now();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate]);

  const sidebar = (
    <Sidebar
      brand={{
        title: "OctoFHIR",
        iconSrc: `${BRANDING}/mark-small.svg`,
        logoSrc: `${BRANDING}/${colorScheme === "dark" ? "logo-dark" : "logo-light"}.svg`,
        onClick: () => navigate("/"),
      }}
      search={{ onOpen: () => setPaletteOpen(true), label: "Jump to…", shortcut: "⌘K" }}
      groups={groups}
      status={{
        label: `${health?.status ?? "unknown"}`,
        theme: statusThemeByHealth[health?.status ?? "down"],
      }}
      account={
        user
          ? {
              name: user.name || user.preferred_username || user.sub,
              onSignOut: logout,
            }
          : null
      }
      colorScheme={colorScheme}
      onToggleColorScheme={toggleColorScheme}
      persistKey={SIDEBAR_KEY}
      collapsed={sidebarCollapsed}
      onCollapsedChange={setSidebarCollapsed}
    />
  );

  const segments = locationSegments(path, navigate);
  const pageTitle = segments.at(-1)?.label;

  const header = (
    <LocationBar
      segments={segments}
      actions={
        <ActionIcon
          variant="subtle"
          size="sm"
          aria-label={colorScheme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
          onClick={toggleColorScheme}
        >
          {colorScheme === "dark" ? <Sun size={15} /> : <Moon size={15} />}
        </ActionIcon>
      }
    />
  );

  return (
    <Shell sidebar={sidebar} header={header}>
      <Helmet>
        <title>{typeof pageTitle === "string" ? `${pageTitle} · OctoFHIR` : "OctoFHIR"}</title>
      </Helmet>
      <ErrorBoundary>
        <Outlet />
      </ErrorBoundary>
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        items={paletteItems}
        recent={recentItems}
        queryItems={queryItems}
        placeholder="Jump to a page, or type Patient/123 or Patient?name=anna…"
      />
      <ShortcutsDialog
        open={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
        goTargets={NAV.flatMap((section) => section.items).flatMap((entry) =>
          entry.goKey ? [{ key: entry.goKey, label: entry.label }] : []
        )}
      />
    </Shell>
  );
}
