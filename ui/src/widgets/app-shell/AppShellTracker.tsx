import { useMemo } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import {
    type AsideHeaderMenuItem,
    TrackerAppShell,
    useColorScheme,
} from "@octofhir/ui-kit";
import { ErrorBoundary } from "@/shared/ui";
import {
    Boxes3,
    Code,
    Cubes3Overlap,
    Database,
    Display,
    FaceRobot,
    FileText,
    Folders,
    Function as FunctionIcon,
    Gear,
    Globe,
    House,
    Key,
    Moon,
    Persons,
    Receipt,
    Shield,
    SquareListUl,
    Sun,
    Terminal,
    TerminalLine,
} from "@gravity-ui/icons";
import { useAuth, useHealth } from "@/shared/api/hooks";

import logoUrl from "/logo.png?url";

interface NavLink {
    id: string;
    title: string;
    path: string;
    icon: AsideHeaderMenuItem["icon"];
    groupId?: string;
}

const NAV_GROUPS = [
    { id: "main", title: "Main" },
    { id: "tools", title: "Tools" },
    { id: "admin", title: "Administration" },
    { id: "auth", title: "Auth" },
];

const NAV_ITEMS: NavLink[] = [
    { id: "dashboard", title: "Dashboard", path: "/", icon: House, groupId: "main" },
    { id: "resources", title: "Resource Browser", path: "/resources", icon: Folders, groupId: "main" },
    { id: "console", title: "REST Console", path: "/console", icon: Terminal, groupId: "main" },
    { id: "packages", title: "Packages", path: "/packages", icon: Boxes3, groupId: "main" },

    { id: "db-console", title: "DB Console", path: "/db-console", icon: Database, groupId: "tools" },
    { id: "graphql", title: "GraphQL", path: "/graphql", icon: Code, groupId: "tools" },
    { id: "fhirpath", title: "FHIRPath", path: "/fhirpath", icon: TerminalLine, groupId: "tools" },
    { id: "viewdef", title: "ViewDefinition", path: "/viewdefinition", icon: SquareListUl, groupId: "tools" },
    { id: "logs", title: "System Logs", path: "/logs", icon: FileText, groupId: "tools" },

    { id: "operations", title: "Operations", path: "/operations", icon: FunctionIcon, groupId: "admin" },
    { id: "apps", title: "Apps", path: "/apps", icon: Cubes3Overlap, groupId: "admin" },
    { id: "automations", title: "Automations", path: "/automations", icon: FaceRobot, groupId: "admin" },
    { id: "audit", title: "Audit Trail", path: "/audit", icon: Receipt, groupId: "admin" },
    { id: "settings", title: "Settings", path: "/settings", icon: Gear, groupId: "admin" },

    { id: "providers", title: "Identity Providers", path: "/auth/providers", icon: Globe, groupId: "auth" },
    { id: "clients", title: "Clients", path: "/auth/clients", icon: Key, groupId: "auth" },
    { id: "users", title: "Users", path: "/auth/users", icon: Persons, groupId: "auth" },
    { id: "sessions", title: "Sessions", path: "/auth/sessions", icon: Display, groupId: "auth" },
    { id: "roles", title: "Roles", path: "/auth/roles", icon: Shield, groupId: "auth" },
    { id: "policies", title: "Access Policies", path: "/auth/policies", icon: Shield, groupId: "auth" },
];

export function AppShellTracker() {
    const navigate = useNavigate();
    const location = useLocation();
    const { logout, user } = useAuth();
    const { colorScheme, toggleColorScheme } = useColorScheme();
    const { data: health } = useHealth();
    const status = health?.status ?? "down";
    const statusTheme: "success" | "warning" | "danger" =
        status === "ok" ? "success" : status === "degraded" ? "warning" : "danger";

    const menuItems: AsideHeaderMenuItem[] = useMemo(() => {
        const isActive = (path: string): boolean =>
            path === "/" ? location.pathname === "/" : location.pathname.startsWith(path);
        return NAV_ITEMS.map((item) => ({
            id: item.id,
            title: item.title,
            icon: item.icon,
            iconSize: 18,
            groupId: item.groupId,
            current: isActive(item.path),
            onItemClick: () => navigate(item.path),
        }));
    }, [location.pathname, navigate]);

    return (
        <TrackerAppShell
            logo={{
                text: "OctoFHIR",
                iconSrc: logoUrl,
                iconSize: 28,
                onClick: () => navigate("/"),
            }}
            menuItems={menuItems}
            menuGroups={NAV_GROUPS}
            defaultPinned
            persistKey="octofhir.tracker"
            collapseBelow={900}
            status={{ label: status, theme: statusTheme }}
            themeAction={{
                icon: colorScheme === "dark" ? <Sun width={18} /> : <Moon width={18} />,
                label: colorScheme === "dark" ? "Light" : "Dark",
                onClick: toggleColorScheme,
            }}
            account={
                user
                    ? {
                          name: user.username,
                          onSignOut: () => void logout().then(() => navigate("/login")),
                      }
                    : null
            }
        >
            <ErrorBoundary>
                <Outlet />
            </ErrorBoundary>
        </TrackerAppShell>
    );
}
