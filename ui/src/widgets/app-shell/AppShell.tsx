import { useMemo } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import {
	Flex,
	ErrorBoundary,
} from "@/shared/ui";
import {
	DashboardShell,
} from "@octofhir/ui-kit";
import {
	House,
	Folder,
	Terminal,
	Cpu,
	Boxes3,
	Gear,
	Database,
	Code,
	Sun,
	Moon,
	Persons,
	Key,
	Shield,
	FileText,
	SquareListUl,
	Cubes3Overlap,
	Function as FunctionIcon,
	Receipt,
	Globe,
	Display,
} from "@gravity-ui/icons";
import { useHealth, useAuth, useSettings } from "@/shared/api/hooks";
import { useColorScheme } from "@octofhir/ui-kit";

const logoUrl = `${import.meta.env.BASE_URL}logo.png`;

/**
 * AppShell provides the top-level navigation and layout for the console.
 * Utilizes the DashboardShell from our UI kit.
 */
export function AppShell() {
	const location = useLocation();
	const navigate = useNavigate();
	const { logout, user } = useAuth();
	const { colorScheme, toggleColorScheme } = useColorScheme();
	const { data: health } = useHealth();
	const { data: settings } = useSettings();

	const menuGroups = useMemo(() => [
		{
			id: "main",
			title: "Main",
			items: [
				{
					id: "dashboard",
					title: "Dashboard",
					icon: House,
					active: location.pathname === "/" || location.pathname === "/ui/",
					onItemClick: () => navigate("/"),
				},
				{
					id: "resources",
					title: "Resources",
					icon: Folder,
					active: location.pathname.startsWith("/resources"),
					onItemClick: () => navigate("/resources"),
				},
				{
					id: "console",
					title: "REST Console",
					icon: Terminal,
					active: location.pathname.startsWith("/console"),
					onItemClick: () => navigate("/console"),
				},
				{
					id: "packages",
					title: "Packages",
					icon: Boxes3,
					active: location.pathname.startsWith("/packages"),
					onItemClick: () => navigate("/packages"),
				},
			],
		},
		{
			id: "admin",
			title: "Administration",
			items: [
				{
					id: "operations",
					title: "Operations",
					icon: FunctionIcon,
					active: location.pathname.startsWith("/operations"),
					onItemClick: () => navigate("/operations"),
				},
				{
					id: "apps",
					title: "Apps",
					icon: Cubes3Overlap,
					active: location.pathname.startsWith("/apps"),
					onItemClick: () => navigate("/apps"),
				},
				{
					id: "automations",
					title: "Automations",
					icon: Cpu,
					active: location.pathname.startsWith("/automations"),
					onItemClick: () => navigate("/automations"),
				},
				{
					id: "audit",
					title: "Audit Trail",
					icon: Receipt,
					active: location.pathname.startsWith("/audit"),
					onItemClick: () => navigate("/audit"),
				},
			],
		},
		{
			id: "auth",
			title: "Auth & Security",
			items: [
				{
					id: "providers",
					title: "Identity Providers",
					icon: Globe,
					active: location.pathname.startsWith("/auth/providers"),
					onItemClick: () => navigate("/auth/providers"),
				},
				{
					id: "clients",
					title: "Clients",
					icon: Key,
					active: location.pathname.startsWith("/auth/clients") || location.pathname.startsWith("/clients"),
					onItemClick: () => navigate("/auth/clients"),
				},
				{
					id: "users",
					title: "Users",
					icon: Persons,
					active: location.pathname.startsWith("/auth/users") || location.pathname.startsWith("/users"),
					onItemClick: () => navigate("/auth/users"),
				},
				{
					id: "roles",
					title: "Roles",
					icon: Shield,
					active: location.pathname.startsWith("/auth/roles"),
					onItemClick: () => navigate("/auth/roles"),
				},
				{
					id: "policies",
					title: "Access Policies",
					icon: Shield,
					active: location.pathname.startsWith("/auth/policies") || location.pathname.startsWith("/access-policies"),
					onItemClick: () => navigate("/auth/policies"),
				},
				{
					id: "sessions",
					title: "Sessions",
					icon: Display,
					active: location.pathname.startsWith("/auth/sessions"),
					onItemClick: () => navigate("/auth/sessions"),
				},
			],
		},
		{
			id: "tools",
			title: "Tools",
			items: [
				{
					id: "db-console",
					title: "DB Console",
					icon: Database,
					active: location.pathname.startsWith("/db-console"),
					onItemClick: () => navigate("/db-console"),
				},
				{
					id: "graphql",
					title: "GraphQL",
					icon: Code,
					active: location.pathname.startsWith("/graphql"),
					onItemClick: () => navigate("/graphql"),
				},
				{
					id: "fhirpath",
					title: "FHIRPath",
					icon: Terminal,
					active: location.pathname.startsWith("/fhirpath"),
					onItemClick: () => navigate("/fhirpath"),
				},
				{
					id: "viewdefinition",
					title: "ViewDefinition",
					icon: SquareListUl,
					active: location.pathname.startsWith("/viewdefinition"),
					onItemClick: () => navigate("/viewdefinition"),
				},
				{
					id: "logs",
					title: "System Logs",
					icon: FileText,
					active: location.pathname.startsWith("/logs"),
					onItemClick: () => navigate("/logs"),
				},
				{
					id: "settings",
					title: "Settings",
					icon: Gear,
					active: location.pathname.startsWith("/settings"),
					onItemClick: () => navigate("/settings"),
				},
			],
		},
	], [location.pathname, navigate]);

	const statusColor = {
		ok: "success",
		degraded: "warning",
		down: "danger",
	}[health?.status ?? "down"];

	return (
		<DashboardShell
			logo={{
				text: settings?.serverName ?? "Abyxon",
				iconSrc: logoUrl,
				onClick: () => navigate("/"),
			}}
			menuItems={[]} // Move everything to groups to ensure they all render correctly
			menuGroups={menuGroups}
			persistKey="octofhir-sidebar"
			themeAction={{
				label: `Switch to ${colorScheme === "dark" ? "light" : "dark"} mode`,
				icon: colorScheme === "dark" ? <Sun size={18} /> : <Moon size={18} />,
				onClick: toggleColorScheme,
			}}
			status={{
				label: health?.status?.toUpperCase() ?? "UNKNOWN",
				theme: statusColor as any,
			}}
			account={user ? {
				name: user.name || user.username,
				onSignOut: logout,
			} : null}
		>
			<Flex direction="column" style={{ minHeight: "100%", backgroundColor: "var(--g-color-base-background)" }}>
				<ErrorBoundary>
					<Outlet />
				</ErrorBoundary>
			</Flex>
		</DashboardShell>
	);
}
