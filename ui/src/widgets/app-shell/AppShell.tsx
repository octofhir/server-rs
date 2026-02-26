import { useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import {
	AppShell as MantineAppShell,
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
	ErrorBoundary,
} from "@/shared/ui";
import {
	useMantineColorScheme,
	useDisclosure,
	useLocalStorage,
} from "@octofhir/ui-kit";
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
	IconLayoutSidebarLeftCollapse,
	IconLayoutSidebarLeftExpand,
} from "@tabler/icons-react";
import { useHealth, useAuth, useBuildInfo, useSettings } from "@/shared/api/hooks";

interface NavItem {
	label: string;
	path: string;
	description: string;
	icon: typeof IconHome;
}
const logoUrl = `${import.meta.env.BASE_URL}logo.png`;
const SIDEBAR_WIDTH_EXPANDED = 240;
const SIDEBAR_WIDTH_COLLAPSED = 68;


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
		label: "FHIRPath Console",
		path: "/fhirpath",
		description: "FHIRPath Expression Evaluator",
		icon: IconTerminal,
	},
	{
		label: "CQL Console",
		path: "/cql",
		description: "Clinical Quality Language Evaluator",
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
	const [sidebarCompact, setSidebarCompact] = useLocalStorage({
		key: "app-shell-sidebar-compact",
		defaultValue: true,
	});
	const [sidebarHovered, setSidebarHovered] = useState(false);
	const location = useLocation();
	const navigate = useNavigate();
	const { logout, user } = useAuth();
	const { data: buildInfo } = useBuildInfo();
	const { data: settings } = useSettings();

	// Check if CQL feature is enabled
	const cqlEnabled = settings?.features?.cql ?? false;
	const isNavbarCompact = sidebarCompact && !sidebarHovered && !opened;
	const navbarWidth = isNavbarCompact
		? SIDEBAR_WIDTH_COLLAPSED
		: SIDEBAR_WIDTH_EXPANDED;

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

	const toggleSidebarCompact = () => {
		setSidebarCompact((prev) => !prev);
		setSidebarHovered(false);
	};

	const renderNavItems = (items: NavItem[]) =>
		items
			.filter((item) => {
				// Hide CQL console if CQL feature is disabled
				if (item.path === "/cql" && !cqlEnabled) {
					return false;
				}
				return true;
			})
			.map((item) => {
				const navItem = (
					<NavLink
						key={item.path}
						label={item.label}
						leftSection={<item.icon size={15} stroke={1.55} />}
						active={isActive(item.path)}
						onClick={() => {
							navigate(item.path);
							close();
						}}
						variant="subtle"
						styles={{
							root: {
								borderRadius: "var(--mantine-radius-sm)",
								paddingTop: 4,
								paddingBottom: 4,
								paddingLeft: 8,
								paddingRight: 8,
								minHeight: 34,
								overflow: "hidden",
								"&[data-active]": {
									backgroundColor: "var(--octo-surface-3)",
									color: "var(--octo-text-primary)",
								},
								"&:hover:not([data-active])": {
									backgroundColor: "var(--app-header-hover-bg)",
								},
							},
							body: {
								overflow: "hidden",
							},
							label: {
								fontSize: "12px",
								fontWeight: 500,
								whiteSpace: "nowrap",
								maxWidth: isNavbarCompact ? 0 : 168,
								opacity: isNavbarCompact ? 0 : 1,
								transform: isNavbarCompact ? "translateX(-4px)" : "translateX(0)",
								marginLeft: isNavbarCompact ? 0 : 2,
								transition:
									"max-width 180ms ease, opacity 140ms ease, transform 180ms ease, margin-left 180ms ease",
							},
							section: {
								marginRight: isNavbarCompact ? 0 : 8,
								opacity: 0.85,
								transition: "margin-right 180ms ease",
							},
						}}
					/>
				);

				if (!isNavbarCompact) {
					return navItem;
				}

				return (
					<Tooltip
						key={item.path}
						label={`${item.label} — ${item.description}`}
						position="right"
						openDelay={200}
					>
						<Box>{navItem}</Box>
					</Tooltip>
				);
			});

	const renderSection = (title: string, items: NavItem[]) => (
		<>
			<Text
				size="11px"
				fw={600}
				c="dimmed"
				tt="uppercase"
				px="xs"
				style={{
					letterSpacing: "0.05em",
					height: 14,
					overflow: "hidden",
					opacity: isNavbarCompact ? 0 : 1,
					transform: isNavbarCompact ? "translateX(-6px)" : "translateX(0)",
					transition: "opacity 160ms ease, transform 160ms ease",
					pointerEvents: "none",
				}}
			>
				{title}
			</Text>
			{renderNavItems(items)}
		</>
	);

	return (
		<MantineAppShell
			header={{ height: 48 }}
			navbar={{
				width: navbarWidth,
				breakpoint: "sm",
				collapsed: { mobile: !opened },
			}}
			padding="0" // Set to 0 because we handle padding in pages
		>
			<MantineAppShell.Header
				style={{
					background: "var(--app-header-bg)",
					color: "var(--app-header-fg)",
					borderBottom: "1px solid var(--octo-border-subtle)",
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
							<Text
								fw={700}
								size="md"
								style={{
									color: "var(--app-header-fg)",
									letterSpacing: "-0.02em",
								}}
							>
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
				onMouseEnter={() => {
					if (sidebarCompact && !opened) {
						setSidebarHovered(true);
					}
				}}
				onMouseLeave={() => {
					setSidebarHovered(false);
				}}
				style={{
					display: "flex",
					flexDirection: "column",
					backgroundColor: "var(--octo-surface-1)",
					borderRight: "1px solid var(--octo-border-subtle)",
					transition: "width 0.28s cubic-bezier(0.2, 0.8, 0.2, 1)",
				}}
			>
				<Box pb="xs" style={{ flexShrink: 0 }}>
					<Group
						justify={isNavbarCompact ? "center" : "space-between"}
						px={isNavbarCompact ? 0 : "xs"}
					>
						{!isNavbarCompact && (
							<Text
								size="11px"
								fw={600}
								c="dimmed"
								tt="uppercase"
								style={{ letterSpacing: "0.05em" }}
							>
								Navigation
							</Text>
						)}
						<Tooltip
							label={sidebarCompact ? "Pin sidebar open" : "Collapse to icons"}
						>
							<ActionIcon
								variant="subtle"
								size="sm"
								onClick={toggleSidebarCompact}
							>
								{sidebarCompact ? (
									<IconLayoutSidebarLeftExpand size={14} />
								) : (
									<IconLayoutSidebarLeftCollapse size={14} />
								)}
							</ActionIcon>
						</Tooltip>
					</Group>
				</Box>

				<Box
					style={{ flex: 1, overflowY: "auto", overflowX: "hidden" }}
					className="custom-scrollbar"
				>
					<Stack gap={4}>
						{renderSection("Main", mainNavigation)}
						<Divider my={4} styles={{ root: { opacity: 0.3 } }} />

						{renderSection("Packages", packagesNavigation)}
						<Divider my={4} styles={{ root: { opacity: 0.3 } }} />

						{renderSection("Admin", adminNavigation)}
						<Divider my={4} styles={{ root: { opacity: 0.3 } }} />

						{renderSection("Auth", authNavigation)}
						<Divider my={4} styles={{ root: { opacity: 0.3 } }} />

						{renderSection("Tools", toolsNavigation)}
					</Stack>
				</Box>

				<Box pt="md" style={{ flexShrink: 0 }}>
					<Divider mb="sm" styles={{ root: { opacity: 0.5 } }} />
					<Stack gap={4} align="center">
						{!isNavbarCompact && (
							<Text size="xs" fw={500} c="dimmed">
								FHIR R4 Server
							</Text>
						)}
						{buildInfo && (
							<Tooltip
								label={`Server v${buildInfo.serverVersion}${
									buildInfo.commit ? ` (${buildInfo.commit.substring(0, 7)})` : ""
								}`}
							>
								<Group gap={4} justify="center">
									<Badge variant="dot" size="xs" color="primary">
										v{buildInfo.serverVersion}
									</Badge>
									{!isNavbarCompact && buildInfo.commit && (
										<Text
											size="xs"
											c="dimmed"
											style={{
												fontFamily:
													"var(--mantine-font-family-monospace)",
												opacity: 0.6,
											}}
										>
											{buildInfo.commit.substring(0, 7)}
										</Text>
									)}
								</Group>
							</Tooltip>
						)}
					</Stack>
				</Box>
			</MantineAppShell.Navbar>

			<MantineAppShell.Main
				style={{
					display: "flex",
					flexDirection: "column",
					height: "calc(100vh - 48px)",
					backgroundColor: "var(--octo-surface-1)",
				}}
			>
				<Box
					style={{
						flex: 1,
						display: "flex",
						flexDirection: "column",
						overflow: "hidden",
					}}
				>
					<ErrorBoundary layout resetKey={location.pathname}>
						<Outlet />
					</ErrorBoundary>
				</Box>
			</MantineAppShell.Main>
		</MantineAppShell>
	);
}
