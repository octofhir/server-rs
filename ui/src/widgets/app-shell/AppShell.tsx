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
import { type ComponentType, useEffect, useMemo } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { useAuth, useHealth, useResourceTypes, useSettings } from "@/shared/api/hooks";
import { $paletteContributions, $paletteOpen, paletteOpenChanged } from "@/shared/command-palette";
import { ErrorBoundary } from "@/shared/ui";

const BRANDING = `${import.meta.env.BASE_URL}branding`;

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
      { id: "resources", label: "Resources", icon: FolderTree, path: "/resources" },
      { id: "requests", label: "Requests", icon: Send, path: "/console" },
      { id: "packages", label: "Packages", icon: Boxes, path: "/packages" },
      { id: "capabilities", label: "Capabilities", icon: FileCode2, path: "/metadata" },
    ],
  },
  {
    id: "query",
    label: "Query",
    items: [
      { id: "sql", label: "SQL", icon: Terminal, path: "/db-console" },
      { id: "graphql", label: "GraphQL", icon: Code2, path: "/graphql" },
      { id: "fhirpath", label: "FHIRPath", icon: Activity, path: "/fhirpath" },
      { id: "cql", label: "CQL", icon: Sigma, path: "/cql" },
      { id: "views", label: "Views", icon: Table, path: "/viewdefinition" },
      { id: "notebooks", label: "Notebooks", icon: NotebookPen, path: "/notebooks" },
    ],
  },
  {
    id: "access",
    label: "Access",
    items: [
      { id: "users", label: "Users", icon: Users, path: "/auth/users", aliases: ["/users"] },
      { id: "roles", label: "Roles", icon: ShieldCheck, path: "/auth/roles" },
      { id: "policies", label: "Policies", icon: Shield, path: "/auth/policies", aliases: ["/access-policies"] },
      { id: "clients", label: "Clients", icon: KeyRound, path: "/auth/clients", aliases: ["/clients"] },
      { id: "sessions", label: "Sessions", icon: Monitor, path: "/auth/sessions" },
      { id: "providers", label: "Identity providers", icon: Globe, path: "/auth/providers" },
    ],
  },
  {
    id: "system",
    label: "System",
    items: [
      { id: "operations", label: "Operations", icon: SquareFunction, path: "/operations" },
      { id: "apps", label: "Apps", icon: LayoutGrid, path: "/apps" },
      { id: "automations", label: "Automations", icon: Cpu, path: "/automations" },
      { id: "audit", label: "Audit", icon: Receipt, path: "/audit" },
      { id: "logs", label: "Logs", icon: FileText, path: "/logs" },
      { id: "database", label: "Database", icon: Database, path: "/database" },
      { id: "settings", label: "Settings", icon: Settings, path: "/settings" },
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
  if (!active) return [{ id: "home", label: "home" }];
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

/** Top-level navigation and layout for the console: panel rail, path header, ⌘K palette. */
export function AppShell() {
  const location = useLocation();
  const navigate = useNavigate();
  const { logout, user } = useAuth();
  const { colorScheme, toggleColorScheme } = useColorScheme();
  const { data: health } = useHealth();
  const { data: resourceTypes } = useResourceTypes();
  useSettings();

  const [paletteOpen, setPaletteOpen, contributions] = useUnit([
    $paletteOpen,
    paletteOpenChanged,
    $paletteContributions,
  ]);

  const path = location.pathname;

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
        hint: entry.path,
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
      persistKey="octofhir-sidebar"
    />
  );

  const header = (
    <LocationBar
      segments={locationSegments(path, navigate)}
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
      <ErrorBoundary>
        <Outlet />
      </ErrorBoundary>
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} items={paletteItems} />
    </Shell>
  );
}
