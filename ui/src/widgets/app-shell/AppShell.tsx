import { Outlet, useLocation, useNavigate } from "react-router-dom";
import {
	AppShell as MantineAppShell,
	Burger,
	Group,
	NavLink,
	Text,
	Badge,
	Tooltip,
	ActionIcon,
	useMantineColorScheme,
	Box,
	Stack,
	Divider,
} from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import {
	IconHome,
	IconFolder,
	IconTerminal,
	IconApi,
	IconApps,
	IconSettings,
	IconDatabase,
	IconCode,
	IconSun,
	IconMoon,
	IconLogout,
	IconActivity,
	IconUsers,
	IconKey,
	IconShield,
} from "@tabler/icons-react";
import { useHealth, useAuth, useBuildInfo } from "@/shared/api/hooks";

interface NavItem {
	label: string;
	path: string;
	description: string;
	icon: typeof IconHome;
}

const mainNavigation: NavItem[] = [
	{
		label: "Dashboard",
		path: "/",
		description: "Overview and quick actions",
		icon: IconHome,
	},
	{
		label: "Resource Browser",
		path: "/resources",
		description: "Browse FHIR resources",
		icon: IconFolder,
	},
	{
		label: "REST Console",
		path: "/console",
		description: "Test FHIR API endpoints",
		icon: IconTerminal,
	},
];

const adminNavigation: NavItem[] = [
	{
		label: "Operations",
		path: "/operations",
		description: "View server API operations",
		icon: IconApi,
	},
	{
		label: "Apps",
		path: "/apps",
		description: "Manage applications",
		icon: IconApps,
	},
];

const authNavigation: NavItem[] = [
	{
		label: "Clients",
		path: "/auth/clients",
		description: "OAuth clients & apps",
		icon: IconKey,
	},
	{
		label: "Users",
		path: "/auth/users",
		description: "User accounts",
		icon: IconUsers,
	},
	{
		label: "Access Policies",
		path: "/auth/policies",
		description: "Access control policies",
		icon: IconShield,
	},
];

const toolsNavigation: NavItem[] = [
	{
		label: "DB Console",
		path: "/db-console",
		description: "SQL Editor & Query Tool",
		icon: IconDatabase,
	},
	{
		label: "GraphQL",
		path: "/graphql",
		description: "GraphQL Query Console",
		icon: IconCode,
	},
	{
		label: "Settings",
		path: "/settings",
		description: "Configure server settings",
		icon: IconSettings,
	},
];

function HealthBadge() {
	const { data: health } = useHealth();

	const statusColor = {
		ok: "green",
		degraded: "yellow",
		down: "red",
	}[health?.status ?? "down"];

	return (
		<Tooltip label={`Server: ${health?.status ?? "unknown"}`}>
			<Badge
				size="sm"
				color={statusColor}
				variant="light"
				leftSection={<IconActivity size={12} />}
			>
				{health?.status ?? "..."}
			</Badge>
		</Tooltip>
	);
}

function ThemeToggle() {
	const { colorScheme, toggleColorScheme } = useMantineColorScheme();

	return (
		<Tooltip label={`Switch to ${colorScheme === "dark" ? "light" : "dark"} mode`}>
			<ActionIcon
				variant="subtle"
				size="lg"
				onClick={() => toggleColorScheme()}
				aria-label="Toggle color scheme"
			>
				{colorScheme === "dark" ? <IconSun size={18} /> : <IconMoon size={18} />}
			</ActionIcon>
		</Tooltip>
	);
}

export function AppShell() {
	const [opened, { toggle, close }] = useDisclosure();
	const location = useLocation();
	const navigate = useNavigate();
	const { logout, user } = useAuth();
	const { data: buildInfo } = useBuildInfo();

	const isActive = (path: string) => {
		if (path === "/") {
			return location.pathname === "/";
		}
		return location.pathname.startsWith(path);
	};

	const handleLogout = async () => {
		await logout();
		navigate("/login");
	};

	const renderNavItems = (items: NavItem[]) =>
		items.map((item) => (
			<NavLink
				key={item.path}
				label={item.label}
				description={item.description}
				leftSection={<item.icon size={18} />}
				active={isActive(item.path)}
				onClick={() => {
					navigate(item.path);
					close();
				}}
				style={{ borderRadius: "var(--mantine-radius-sm)" }}
			/>
		));

	return (
		<MantineAppShell
			header={{ height: 56 }}
			navbar={{
				width: 280,
				breakpoint: "sm",
				collapsed: { mobile: !opened },
			}}
			padding="md"
		>
			<MantineAppShell.Header>
				<Group h="100%" px="md" justify="space-between">
					<Group>
						<Burger opened={opened} onClick={toggle} hiddenFrom="sm" size="sm" />
						<Group gap="xs">
							<Text size="xl">🐙</Text>
							<Text fw={600} size="lg">
								OctoFHIR
							</Text>
						</Group>
					</Group>
					<Group>
						<HealthBadge />
						<ThemeToggle />
						{user && (
							<Tooltip label={`Logged in as ${user.preferred_username || user.sub}`}>
								<ActionIcon variant="subtle" size="lg" onClick={handleLogout}>
									<IconLogout size={18} />
								</ActionIcon>
							</Tooltip>
						)}
					</Group>
				</Group>
			</MantineAppShell.Header>

			<MantineAppShell.Navbar p="md" style={{ display: "flex", flexDirection: "column" }}>
				<Box style={{ flex: 1, overflowY: "auto", overflowX: "hidden" }}>
					<Stack gap="xs">
						<Text size="xs" fw={500} c="dimmed" tt="uppercase">
							Main
						</Text>
						{renderNavItems(mainNavigation)}

						<Divider my="sm" />

						<Text size="xs" fw={500} c="dimmed" tt="uppercase">
							Admin
						</Text>
						{renderNavItems(adminNavigation)}

						<Divider my="sm" />

						<Text size="xs" fw={500} c="dimmed" tt="uppercase">
							Auth
						</Text>
						{renderNavItems(authNavigation)}

						<Divider my="sm" />

						<Text size="xs" fw={500} c="dimmed" tt="uppercase">
							Tools
						</Text>
						{renderNavItems(toolsNavigation)}
					</Stack>
				</Box>

				<Box pt="md" style={{ flexShrink: 0 }}>
					<Divider mb="sm" />
					<Stack gap={2} align="center">
						<Text size="xs" c="dimmed">
							FHIR R4 Server
						</Text>
						{buildInfo && (
							<>
								<Text size="xs" c="dimmed">
									v{buildInfo.serverVersion}
								</Text>
								{buildInfo.commit && (
									<Text size="xs" c="dimmed" style={{ fontFamily: "var(--mantine-font-family-monospace)" }}>
										{buildInfo.commit.substring(0, 7)}
									</Text>
								)}
							</>
						)}
					</Stack>
				</Box>
			</MantineAppShell.Navbar>

			<MantineAppShell.Main
				style={{
					display: "flex",
					flexDirection: "column",
					height: "calc(100vh - 56px)",
				}}
			>
				<Box style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "auto" }}>
					<Outlet />
				</Box>
			</MantineAppShell.Main>
		</MantineAppShell>
	);
}
