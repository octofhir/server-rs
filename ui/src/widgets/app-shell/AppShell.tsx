import { useColorScheme } from "@octofhir/ui-kit";
import { useMemo } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import {
  Boxes,
  Code2,
  Cpu,
  Database,
  FileText,
  FolderTree,
  Globe,
  House,
  KeyRound,
  LayoutDashboard,
  Monitor,
  Receipt,
  Settings,
  Shield,
  ShieldCheck,
  SquareFunction,
  Table,
  Terminal,
  TerminalSquare,
  Users,
} from "lucide-react";
import { ErrorBoundary } from "@/shared/ui";
import { Shell, Sidebar, type SidebarNavGroup, type SidebarStatus } from "@octofhir/ui-kit";
import { useAuth, useHealth, useSettings } from "@/shared/api/hooks";

const logoUrl = `${import.meta.env.BASE_URL}logo.png`;

const statusThemeByHealth: Record<"ok" | "degraded" | "down", SidebarStatus["theme"]> = {
  ok: "success",
  degraded: "warning",
  down: "danger",
};

/** Top-level navigation and layout for the console, built on the ui-kit Sidebar. */
export function AppShell() {
  const location = useLocation();
  const navigate = useNavigate();
  const { logout, user } = useAuth();
  const { colorScheme, toggleColorScheme } = useColorScheme();
  const { data: health } = useHealth();
  useSettings();

  const path = location.pathname;
  const groups = useMemo<SidebarNavGroup[]>(
    () => [
      {
        id: "main",
        label: "Main",
        items: [
          {
            id: "dashboard",
            label: "Dashboard",
            icon: House,
            active: path === "/" || path === "/ui/",
            onClick: () => navigate("/"),
          },
          {
            id: "resources",
            label: "Resources",
            icon: FolderTree,
            active: path.startsWith("/resources"),
            onClick: () => navigate("/resources"),
          },
          {
            id: "console",
            label: "REST Console",
            icon: Terminal,
            active: path.startsWith("/console"),
            onClick: () => navigate("/console"),
          },
          {
            id: "packages",
            label: "Packages",
            icon: Boxes,
            active: path.startsWith("/packages"),
            onClick: () => navigate("/packages"),
          },
        ],
      },
      {
        id: "admin",
        label: "Administration",
        items: [
          {
            id: "operations",
            label: "Operations",
            icon: SquareFunction,
            active: path.startsWith("/operations"),
            onClick: () => navigate("/operations"),
          },
          {
            id: "apps",
            label: "Apps",
            icon: LayoutDashboard,
            active: path.startsWith("/apps"),
            onClick: () => navigate("/apps"),
          },
          {
            id: "automations",
            label: "Automations",
            icon: Cpu,
            active: path.startsWith("/automations"),
            onClick: () => navigate("/automations"),
          },
          {
            id: "audit",
            label: "Audit Trail",
            icon: Receipt,
            active: path.startsWith("/audit"),
            onClick: () => navigate("/audit"),
          },
        ],
      },
      {
        id: "auth",
        label: "Auth & Security",
        items: [
          {
            id: "providers",
            label: "Identity Providers",
            icon: Globe,
            active: path.startsWith("/auth/providers"),
            onClick: () => navigate("/auth/providers"),
          },
          {
            id: "clients",
            label: "Clients",
            icon: KeyRound,
            active: path.startsWith("/auth/clients") || path.startsWith("/clients"),
            onClick: () => navigate("/auth/clients"),
          },
          {
            id: "users",
            label: "Users",
            icon: Users,
            active: path.startsWith("/auth/users") || path.startsWith("/users"),
            onClick: () => navigate("/auth/users"),
          },
          {
            id: "roles",
            label: "Roles",
            icon: ShieldCheck,
            active: path.startsWith("/auth/roles"),
            onClick: () => navigate("/auth/roles"),
          },
          {
            id: "policies",
            label: "Access Policies",
            icon: Shield,
            active: path.startsWith("/auth/policies") || path.startsWith("/access-policies"),
            onClick: () => navigate("/auth/policies"),
          },
          {
            id: "sessions",
            label: "Sessions",
            icon: Monitor,
            active: path.startsWith("/auth/sessions"),
            onClick: () => navigate("/auth/sessions"),
          },
        ],
      },
      {
        id: "tools",
        label: "Tools",
        items: [
          {
            id: "database",
            label: "Database",
            icon: Database,
            active: path.startsWith("/database"),
            onClick: () => navigate("/database"),
          },
          {
            id: "db-console",
            label: "SQL Console",
            icon: TerminalSquare,
            active: path.startsWith("/db-console"),
            onClick: () => navigate("/db-console"),
          },
          {
            id: "graphql",
            label: "GraphQL",
            icon: Code2,
            active: path.startsWith("/graphql"),
            onClick: () => navigate("/graphql"),
          },
          {
            id: "fhirpath",
            label: "FHIRPath",
            icon: Terminal,
            active: path.startsWith("/fhirpath"),
            onClick: () => navigate("/fhirpath"),
          },
          {
            id: "viewdefinition",
            label: "ViewDefinition",
            icon: Table,
            active: path.startsWith("/viewdefinition"),
            onClick: () => navigate("/viewdefinition"),
          },
          {
            id: "logs",
            label: "System Logs",
            icon: FileText,
            active: path.startsWith("/logs"),
            onClick: () => navigate("/logs"),
          },
          {
            id: "settings",
            label: "Settings",
            icon: Settings,
            active: path.startsWith("/settings"),
            onClick: () => navigate("/settings"),
          },
        ],
      },
    ],
    [path, navigate]
  );

  const sidebar = (
    <Sidebar
      brand={{ title: "OctoFHIR", iconSrc: logoUrl, onClick: () => navigate("/") }}
      groups={groups}
      status={{
        label: health?.status?.toUpperCase() ?? "UNKNOWN",
        theme: statusThemeByHealth[health?.status ?? "down"],
      }}
      account={
        user
          ? {
              name: user.name || user.preferred_username || user.sub,
              secondary: "Signed in",
              onSignOut: logout,
            }
          : null
      }
      colorScheme={colorScheme}
      onToggleColorScheme={toggleColorScheme}
      persistKey="octofhir-sidebar"
    />
  );

  return (
    <Shell sidebar={sidebar}>
      <ErrorBoundary>
        <Outlet />
      </ErrorBoundary>
    </Shell>
  );
}
