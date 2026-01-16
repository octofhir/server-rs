import { useEffect, useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import {
	AppShell as MantineAppShell,
	useMantineColorScheme,
} from "@mantine/core";
import {
	Badge,
	ActionIcon,
	Burger,
	Group,
	NavLink,
	Text,
	Tooltip,
	Box,
	Stack,
	Divider,
	Menu,
	ErrorBoundary,
} from "@/shared/ui";
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
	IconPackage,
	IconWorld,
	IconFileText,
	IconTable,
	IconDevices,
	IconRobot,
} from "@tabler/icons-react";
import { useUnit } from "effector-react";
import { useHealth, useAuth, useBuildInfo } from "@/shared/api/hooks";
import {
	$tabs,
	openTab,
	openNewTabForPath,
	openTabForPath,
	renameTab,
	resolveTabFromPath,
	togglePinTab,
} from "@/shared/state/appTabsStore";
import { AppTabsBar } from "./AppTabsBar";

interface NavItem {
	label: string;
	path: string;
	description: string;
	icon: typeof IconHome;
}
const logoUrl = `${import.meta.env.BASE_URL}logo.png`;


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

const packagesNavigation: NavItem[] = [
	{
		label: "Packages",
		path: "/packages",
		description: "Manage FHIR packages",
		icon: IconPackage,
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
	{
		label: "Automations",
		path: "/automations",
		description: "Event-driven automation scripts",
		icon: IconRobot,
	},
	{
		label: "Audit Trail",
		path: "/audit",
		description: "Track system activity",
		icon: IconShield,
	},
];

const authNavigation: NavItem[] = [
	{
		label: "Identity Providers",
		path: "/auth/providers",
		description: "External auth providers",
		icon: IconWorld,
	},
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
		label: "Sessions",
		path: "/auth/sessions",
		description: "Active login sessions",
		icon: IconDevices,
	},
	{
		label: "Roles",
		path: "/auth/roles",
		description: "Manage roles & permissions",
		icon: IconShield,
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
		label: "ViewDefinition",
		path: "/viewdefinition",
		description: "SQL on FHIR ViewDefinition Editor",
		icon: IconTable,
	},
	{
		label: "System Logs",
		path: "/logs",
		description: "Real-time server logs",
		icon: IconFileText,
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
				style={{ color: "var(--app-header-fg)" }}
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
	const {
		tabs,
		openTab: openTabEvent,
		openTabForPath: openTabForPathEvent,
		openNewTabForPath: openNewTabForPathEvent,
		renameTab: renameTabEvent,
		togglePinTab: togglePinTabEvent,
	} = useUnit({
		tabs: $tabs,
		openTab,
		openTabForPath,
		openNewTabForPath,
		renameTab,
		togglePinTab,
	});
	const [menuState, setMenuState] = useState<{
		x: number;
		y: number;
		item: NavItem;
	} | null>(null);

	useEffect(() => {
		openTabForPathEvent({ pathname: location.pathname });
	}, [location.pathname, openTabForPathEvent]);

	useEffect(() => {
		if (tabs.length === 0) {
			openTabForPathEvent({ pathname: "/" });
		}
	}, [tabs.length, openTabForPathEvent]);

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
				leftSection={<item.icon size={14} stroke={1.5} />}
				active={isActive(item.path)}
				onClick={() => {
					openTabForPathEvent({ pathname: item.path, titleOverride: item.label });
					navigate(item.path);
					close();
				}}
				onContextMenu={(event: React.MouseEvent) => {
					event.preventDefault();
					setMenuState({
						x: event.clientX,
						y: event.clientY,
						item,
					});
				}}
				variant="subtle"
				styles={{
					root: {
						borderRadius: "var(--mantine-radius-sm)",
						paddingTop: 4,
						paddingBottom: 4,
						minHeight: 28,
						"&[data-active]": {
							backgroundColor: "var(--app-surface-3)",
							color: "var(--app-text-primary)",
						},
						"&:hover:not([data-active])": {
							backgroundColor: "var(--app-header-hover-bg)",
						}
					},
					label: {
						fontSize: "12px",
						fontWeight: 500,
						transition: "color 150ms ease",
					},
					section: {
						marginRight: 8,
						opacity: 0.8,
					}
				}}
			/>
		));

	const handleMenuClose = () => setMenuState(null);

	const handleOpenNewTab = () => {
		if (!menuState) return;
		openNewTabForPathEvent({
			pathname: menuState.item.path,
			titleOverride: menuState.item.label,
		});
		handleMenuClose();
	};

	const handleRenameTab = () => {
		if (!menuState) return;
		const normalized = menuState.item.path.replace(/\/+$/, "") || "/";
		const resolved = resolveTabFromPath(normalized);
		const candidate =
			tabs.find((tab) => tab.path === normalized && !tab.customTitle) ??
			(resolved?.groupKey
				? tabs.find((tab) => tab.groupKey === resolved.groupKey && !tab.customTitle)
				: undefined);
		if (!candidate) {
			if (!resolved) return;
			openTabEvent(resolved);
			const nextTitle = window.prompt("Rename tab", resolved.title);
			if (nextTitle && nextTitle.trim()) {
				renameTabEvent({ id: resolved.id, title: nextTitle.trim() });
			}
			handleMenuClose();
			return;
		}
		const nextTitle = window.prompt("Rename tab", candidate.title);
		if (nextTitle && nextTitle.trim()) {
			renameTabEvent({ id: candidate.id, title: nextTitle.trim() });
		}
		handleMenuClose();
	};

	const handleTogglePin = () => {
		if (!menuState) return;
		const normalized = menuState.item.path.replace(/\/+$/, "") || "/";
		const resolved = resolveTabFromPath(normalized);
		const candidate =
			tabs.find((tab) => tab.path === normalized && !tab.customTitle) ??
			(resolved?.groupKey
				? tabs.find((tab) => tab.groupKey === resolved.groupKey && !tab.customTitle)
				: undefined);
		if (candidate) {
			togglePinTabEvent(candidate.id);
		} else {
			if (resolved) {
				openTabEvent({ ...resolved, pinned: true });
			}
		}
		handleMenuClose();
	};

	return (
		<MantineAppShell
			header={{ height: 48 }}
			navbar={{
				width: 240,
				breakpoint: "sm",
				collapsed: { mobile: !opened },
			}}
			padding="0" // Set to 0 because we handle padding in pages
		>
			<MantineAppShell.Header
				style={{
					background: "var(--app-header-bg)",
					color: "var(--app-header-fg)",
					backdropFilter: "blur(var(--app-glass-blur))",
					borderBottom: "1px solid var(--app-glass-border)",
					zIndex: 100,
				}}
			>
				<Group h="100%" px="md" justify="space-between">
					<Group>
						<Burger
							opened={opened}
							onClick={toggle}
							hiddenFrom="sm"
							size="sm"
							color="var(--app-header-fg)"
						/>
						<Group gap="xs">
							<Box
								component="img"
								src={logoUrl}
								alt="Abyxon logo"
								h={32}
								style={{ width: "auto" }}
							/>
							<Text fw={700} size="md" style={{ color: "var(--app-header-fg)", letterSpacing: "-0.02em" }}>
								Abyxon
							</Text>
						</Group>
					</Group>
					<Group gap="sm">
						<HealthBadge />
						<ThemeToggle />
						{user && (
							<Tooltip label={`Logged in as ${user.preferred_username || user.sub}`}>
								<ActionIcon
									variant="subtle"
									size="lg"
									onClick={handleLogout}
									style={{ color: "var(--app-header-fg)" }}
									styles={{
										root: {
											borderRadius: "10px",
											"&:hover": {
												backgroundColor: "var(--app-header-hover-bg)",
											},
										},
									}}
								>
									<IconLogout size={18} />
								</ActionIcon>
							</Tooltip>
						)}
					</Group>
				</Group>
			</MantineAppShell.Header>

			<MantineAppShell.Navbar
				p="xs"
				style={{
					display: "flex",
					flexDirection: "column",
					backgroundColor: "var(--app-glass-bg)",
					backdropFilter: "blur(var(--app-glass-blur))",
					borderRight: "1px solid var(--app-glass-border)",
					transition: "all 0.2s ease",
				}}
			>
				{menuState && (
					<Menu opened onClose={handleMenuClose} withinPortal position="bottom-start">
						<Menu.Target>
							<Box
								style={{
									position: "fixed",
									left: menuState.x,
									top: menuState.y,
									width: 1,
									height: 1,
								}}
							/>
						</Menu.Target>
						<Menu.Dropdown style={{ borderRadius: "12px", border: "1px solid var(--app-border-subtle)" }}>
							<Menu.Item leftSection={<IconCode size={14} />} onClick={handleOpenNewTab}>Open in new tab</Menu.Item>
							<Menu.Item leftSection={<IconSettings size={14} />} onClick={handleRenameTab}>Rename tab</Menu.Item>
							<Menu.Item leftSection={<IconApps size={14} />} onClick={handleTogglePin}>Pin/Unpin tab</Menu.Item>
						</Menu.Dropdown>
					</Menu>
				)}
				<Box style={{ flex: 1, overflowY: "auto", overflowX: "hidden" }} className="custom-scrollbar">
					<Stack gap={4}>
						<Text size="11px" fw={600} c="dimmed" tt="uppercase" px="xs" mt="xs" style={{ letterSpacing: "0.05em" }}>
							Main
						</Text>
						{renderNavItems(mainNavigation)}

						<Divider my={4} styles={{ root: { opacity: 0.3 } }} />

						<Text size="11px" fw={600} c="dimmed" tt="uppercase" px="xs" style={{ letterSpacing: "0.05em" }}>
							Packages
						</Text>
						{renderNavItems(packagesNavigation)}

						<Divider my={4} styles={{ root: { opacity: 0.3 } }} />

						<Text size="11px" fw={600} c="dimmed" tt="uppercase" px="xs" style={{ letterSpacing: "0.05em" }}>
							Admin
						</Text>
						{renderNavItems(adminNavigation)}

						<Divider my={4} styles={{ root: { opacity: 0.3 } }} />

						<Text size="11px" fw={600} c="dimmed" tt="uppercase" px="xs" style={{ letterSpacing: "0.05em" }}>
							Auth
						</Text>
						{renderNavItems(authNavigation)}

						<Divider my={4} styles={{ root: { opacity: 0.3 } }} />

						<Text size="11px" fw={600} c="dimmed" tt="uppercase" px="xs" style={{ letterSpacing: "0.05em" }}>
							Tools
						</Text>
						{renderNavItems(toolsNavigation)}
					</Stack>
				</Box>

				<Box pt="md" style={{ flexShrink: 0 }}>
					<Divider mb="sm" styles={{ root: { opacity: 0.5 } }} />
					<Stack gap={4} align="center">
						<Text size="xs" fw={500} c="dimmed">
							FHIR R4 Server
						</Text>
						{buildInfo && (
							<Group gap={4} justify="center">
								<Badge variant="dot" size="xs" color="primary">
									v{buildInfo.serverVersion}
								</Badge>
								{buildInfo.commit && (
									<Text size="xs" c="dimmed" style={{ fontFamily: "var(--mantine-font-family-monospace)", opacity: 0.6 }}>
										{buildInfo.commit.substring(0, 7)}
									</Text>
								)}
							</Group>
						)}
					</Stack>
				</Box>
			</MantineAppShell.Navbar>

			<MantineAppShell.Main
				style={{
					display: "flex",
					flexDirection: "column",
					height: "calc(100vh - 48px)",
					backgroundColor: "var(--app-surface-1)",
				}}
			>
				<AppTabsBar />
				<Box style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
					<ErrorBoundary layout resetKey={location.pathname}>
						<Outlet />
					</ErrorBoundary>
				</Box>
			</MantineAppShell.Main>
		</MantineAppShell>
	);
}
