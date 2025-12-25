import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
	Stack,
	Title,
	Text,
	Paper,
	Group,
	Badge,
	Table,
	Loader,
	Alert,
	TextInput,
	ActionIcon,
	Tooltip,
	SimpleGrid,
	Card,
	ThemeIcon,
	rem,
	Tabs,
	Button,
	Select,
} from "@/shared/ui";
import { notifications } from "@mantine/notifications";
import {
	IconAlertCircle,
	IconSearch,
	IconPackage,
	IconEye,
	IconCheck,
	IconAlertTriangle,
	IconDownload,
	IconWorld,
} from "@tabler/icons-react";
import {
	usePackages,
	usePackageLookup,
	usePackageSearch,
	useInstallPackageWithProgress,
} from "@/shared/api/hooks";
import type { PackageInfo } from "@/shared/api/types";
import { InstallProgressModal } from "./InstallProgressModal";

// Map FHIR version strings to their major version identifier
function normalizeFhirVersion(version?: string): string {
	if (!version) return "unknown";
	const v = version.toLowerCase().trim();
	// Handle semantic versions like "4.0.1", "4.3.0", "5.0.0"
	if (v.startsWith("4.0")) return "R4";
	if (v.startsWith("4.3")) return "R4B";
	if (v.startsWith("5.0")) return "R5";
	if (v.startsWith("6.0")) return "R6";
	// Handle explicit R notation
	if (v === "r4" || v === "r4.0.1") return "R4";
	if (v === "r4b" || v === "r4.3.0") return "R4B";
	if (v === "r5" || v === "r5.0.0") return "R5";
	if (v === "r6") return "R6";
	// Return original if unknown format
	return version.toUpperCase();
}

function FhirVersionBadge({
	packageVersion,
	serverVersion,
}: {
	packageVersion?: string;
	serverVersion: string;
}) {
	const normalizedPackage = normalizeFhirVersion(packageVersion);
	const normalizedServer = normalizeFhirVersion(serverVersion);
	const isCompatible =
		!packageVersion || normalizedPackage === normalizedServer;

	return (
		<Tooltip
			label={
				isCompatible
					? `Compatible with server(${serverVersion})`
					: `Package is ${packageVersion}, server is ${serverVersion} `
			}
		>
			<Badge
				size="sm"
				variant="light"
				color={isCompatible ? "primary" : "warm"}
				leftSection={
					isCompatible ? (
						<IconCheck size={12} />
					) : (
						<IconAlertTriangle size={12} />
					)
				}
			>
				{packageVersion || "unknown"}
			</Badge>
		</Tooltip>
	);
}

function PackageCard({
	pkg,
	serverVersion,
	onView,
}: {
	pkg: PackageInfo;
	serverVersion: string;
	onView: (name: string, version: string) => void;
}) {
	return (
		<Card
			shadow="sm"
			padding="lg"
			radius="md"
			style={{ backgroundColor: "var(--app-surface-1)" }}
		>
			<Group justify="space-between" mb="xs">
				<Group gap="xs">
					<ThemeIcon variant="light" size="md" color="warm">
						<IconPackage size={16} />
					</ThemeIcon>
					<Text fw={500} lineClamp={1}>
						{pkg.name}
					</Text>
				</Group>
				<Tooltip label="View package details">
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={() => onView(pkg.name, pkg.version)}
					>
						<IconEye size={16} />
					</ActionIcon>
				</Tooltip>
			</Group>

			<Group gap="xs" mb="sm">
				<Badge size="sm" variant="outline">
					v{pkg.version}
				</Badge>
				<FhirVersionBadge
					packageVersion={pkg.fhirVersion}
					serverVersion={serverVersion}
				/>
			</Group>

			<Text size="sm" c="dimmed">
				{pkg.resourceCount} resources
			</Text>

			{pkg.installedAt && (
				<Text size="xs" c="dimmed" mt="xs">
					Installed: {new Date(pkg.installedAt).toLocaleDateString()}
				</Text>
			)}
		</Card>
	);
}

function PackageTableRow({
	pkg,
	serverVersion,
	onView,
}: {
	pkg: PackageInfo;
	serverVersion: string;
	onView: (name: string, version: string) => void;
}) {
	return (
		<Table.Tr>
			<Table.Td>
				<Group gap="xs">
					<IconPackage
						size={16}
						style={{ color: "var(--app-accent-primary)" }}
					/>
					<Text fw={500}>{pkg.name}</Text>
				</Group>
			</Table.Td>
			<Table.Td>
				<Badge size="sm" variant="outline">
					{pkg.version}
				</Badge>
			</Table.Td>
			<Table.Td>
				<FhirVersionBadge
					packageVersion={pkg.fhirVersion}
					serverVersion={serverVersion}
				/>
			</Table.Td>
			<Table.Td>
				<Text size="sm">{pkg.resourceCount}</Text>
			</Table.Td>
			<Table.Td>
				{pkg.installedAt ? (
					<Text size="sm" c="dimmed">
						{new Date(pkg.installedAt).toLocaleDateString()}
					</Text>
				) : (
					<Text size="sm" c="dimmed">
						-
					</Text>
				)}
			</Table.Td>
			<Table.Td>
				<Tooltip label="View package details">
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={() => onView(pkg.name, pkg.version)}
					>
						<IconEye size={16} />
					</ActionIcon>
				</Tooltip>
			</Table.Td>
		</Table.Tr>
	);
}

function InstalledPackagesTab({
	serverVersion,
	onView,
}: {
	serverVersion: string;
	onView: (name: string, version: string) => void;
}) {
	const [search, setSearch] = useState("");
	const [viewMode] = useState<"grid" | "table">("table");
	const { data, isLoading, error } = usePackages();

	const filteredPackages = useMemo(() => {
		if (!data?.packages) return [];

		const searchLower = search.toLowerCase();
		return data.packages.filter(
			(pkg) =>
				!search ||
				pkg.name.toLowerCase().includes(searchLower) ||
				pkg.version.toLowerCase().includes(searchLower),
		);
	}, [data, search]);

	return (
		<Stack gap="md">
			<TextInput
				placeholder="Search installed packages..."
				leftSection={<IconSearch size={16} />}
				value={search}
				onChange={(e) => setSearch(e.currentTarget.value)}
			/>

			{isLoading && (
				<Group justify="center" py="xl">
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading packages...
					</Text>
				</Group>
			)}

			{error && (
				<Alert
					icon={<IconAlertCircle size={16} />}
					color="fire"
					variant="light"
				>
					{error instanceof Error ? error.message : "Failed to load packages"}
				</Alert>
			)}

			{!isLoading && !error && data && (
				<>
					<Text size="sm" c="dimmed">
						{filteredPackages.length} packages
						{filteredPackages.length !== data.packages.length &&
							` (filtered from ${data.packages.length})`}
					</Text>

					{filteredPackages.length === 0 ? (
						<Paper p="xl" style={{ backgroundColor: "var(--app-surface-2)" }}>
							<Stack align="center" gap="md">
								<ThemeIcon
									size={rem(60)}
									radius="xl"
									variant="light"
									color="warm"
								>
									<IconPackage size={30} />
								</ThemeIcon>
								<Text ta="center" c="dimmed">
									{search
										? "No packages match your search"
										: "No packages installed"}
								</Text>
							</Stack>
						</Paper>
					) : viewMode === "grid" ? (
						<SimpleGrid cols={{ base: 1, sm: 2, md: 3, lg: 4 }}>
							{filteredPackages.map((pkg) => (
								<PackageCard
									key={`${pkg.name} @${pkg.version} `}
									pkg={pkg}
									serverVersion={serverVersion}
									onView={onView}
								/>
							))}
						</SimpleGrid>
					) : (
						<Paper style={{ backgroundColor: "var(--app-surface-1)" }}>
							<Table striped highlightOnHover>
								<Table.Thead>
									<Table.Tr>
										<Table.Th>Package</Table.Th>
										<Table.Th>Version</Table.Th>
										<Table.Th>FHIR Version</Table.Th>
										<Table.Th>Resources</Table.Th>
										<Table.Th>Installed</Table.Th>
										<Table.Th w={50} />
									</Table.Tr>
								</Table.Thead>
								<Table.Tbody>
									{filteredPackages.map((pkg) => (
										<PackageTableRow
											key={`${pkg.name} @${pkg.version} `}
											pkg={pkg}
											serverVersion={serverVersion}
											onView={onView}
										/>
									))}
								</Table.Tbody>
							</Table>
						</Paper>
					)}
				</>
			)}
		</Stack>
	);
}

function RegistryTab({
	serverVersion: _serverVersion,
}: {
	serverVersion: string;
}) {
	const [searchQuery, setSearchQuery] = useState("");
	const [selectedPackage, setSelectedPackage] = useState<string | null>(null);
	const [selectedVersion, setSelectedVersion] = useState<string | null>(null);
	const [installModalOpen, setInstallModalOpen] = useState(false);

	// Search for packages using partial matching
	const { data: searchData, isLoading: searchLoading } =
		usePackageSearch(searchQuery);
	// Lookup specific package versions when one is selected
	const { data: lookupData } = usePackageLookup(selectedPackage || "");

	// Use SSE-based install with progress tracking
	const { install, reset, events, isInstalling, error } =
		useInstallPackageWithProgress();

	const handleStartInstall = () => {
		if (!selectedPackage || !selectedVersion) return;
		setInstallModalOpen(true);
		install({ name: selectedPackage, version: selectedVersion });
	};

	const handleCloseModal = () => {
		if (!isInstalling) {
			setInstallModalOpen(false);
			// Clear selection after successful install
			const lastEvent = events[events.length - 1];
			if (lastEvent?.type === "completed") {
				setSelectedVersion(null);
				setSelectedPackage(null);
				notifications.show({
					title: "Package Installed",
					message: `Successfully installed ${selectedPackage} @${selectedVersion} `,
					color: "green",
				});
			}
			reset();
		}
	};

	const versionOptions = useMemo(() => {
		if (!lookupData?.versions) return [];
		return lookupData.versions.map((v) => ({
			value: v,
			label: lookupData.installedVersions.includes(v) ? `${v} (installed)` : v,
			disabled: lookupData.installedVersions.includes(v),
		}));
	}, [lookupData]);

	return (
		<Stack gap="md">
			<Paper p="md" style={{ backgroundColor: "var(--app-surface-2)" }}>
				<Stack gap="md">
					<Text size="sm" c="dimmed">
						Search for packages in the FHIR Package Registry (fs.get-ig.org)
					</Text>
					<TextInput
						placeholder="Search packages... (e.g., us core, hl7.fhir)"
						leftSection={<IconSearch size={16} />}
						value={searchQuery}
						onChange={(e) => {
							setSearchQuery(e.currentTarget.value);
							setSelectedPackage(null);
							setSelectedVersion(null);
						}}
					/>
				</Stack>
			</Paper>

			{searchLoading && (
				<Group justify="center" py="xl">
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Searching packages...
					</Text>
				</Group>
			)}

			{searchData && searchData.packages.length > 0 && (
				<Paper style={{ backgroundColor: "var(--app-surface-1)" }}>
					<Table striped highlightOnHover>
						<Table.Thead>
							<Table.Tr>
								<Table.Th>Package</Table.Th>
								<Table.Th>Description</Table.Th>
								<Table.Th>Latest</Table.Th>
								<Table.Th w={200}>Version</Table.Th>
								<Table.Th w={100} />
							</Table.Tr>
						</Table.Thead>
						<Table.Tbody>
							{searchData.packages.map((pkg) => (
								<Table.Tr key={pkg.name}>
									<Table.Td>
										<Group gap="xs">
											<IconPackage
												size={16}
												style={{ color: "var(--app-accent-primary)" }}
											/>
											<Text fw={500} size="sm">
												{pkg.name}
											</Text>
										</Group>
									</Table.Td>
									<Table.Td>
										<Text size="sm" c="dimmed" lineClamp={1}>
											{pkg.description || "-"}
										</Text>
									</Table.Td>
									<Table.Td>
										<Badge size="sm" variant="outline">
											{pkg.latestVersion}
										</Badge>
									</Table.Td>
									<Table.Td>
										{selectedPackage === pkg.name ? (
											<Select
												size="xs"
												placeholder="Select version"
												data={versionOptions}
												value={selectedVersion}
												onChange={setSelectedVersion}
											/>
										) : (
											<Button
												size="xs"
												variant="light"
												onClick={() => {
													setSelectedPackage(pkg.name);
													setSelectedVersion(null);
												}}
											>
												Select version
											</Button>
										)}
									</Table.Td>
									<Table.Td>
										<Button
											size="xs"
											leftSection={<IconDownload size={14} />}
											onClick={handleStartInstall}
											disabled={
												selectedPackage !== pkg.name || !selectedVersion
											}
										>
											Install
										</Button>
									</Table.Td>
								</Table.Tr>
							))}
						</Table.Tbody>
					</Table>
				</Paper>
			)}

			{searchData &&
				searchData.packages.length === 0 &&
				searchQuery.length >= 2 && (
					<Alert
						icon={<IconAlertCircle size={16} />}
						color="warm"
						variant="light"
					>
						No packages found matching "{searchQuery}". Try a different search
						term.
					</Alert>
				)}

			{!searchData && !searchLoading && (
				<Paper p="xl" style={{ backgroundColor: "var(--app-surface-2)" }}>
					<Stack align="center" gap="md">
						<ThemeIcon size={rem(60)} radius="xl" variant="light" color="fire">
							<IconWorld size={30} />
						</ThemeIcon>
						<Text ta="center" c="dimmed">
							Enter a search term to find packages in the FHIR registry
						</Text>
						<Text ta="center" size="xs" c="dimmed">
							Examples: us core, hl7.fhir, terminology, mcode
						</Text>
					</Stack>
				</Paper>
			)}

			{/* Install Progress Modal */}
			<InstallProgressModal
				opened={installModalOpen}
				onClose={handleCloseModal}
				events={events}
				isInstalling={isInstalling}
				error={error}
				packageName={selectedPackage || ""}
				packageVersion={selectedVersion || ""}
			/>
		</Stack>
	);
}

export function PackagesPage() {
	const navigate = useNavigate();
	const [activeTab, setActiveTab] = useState<string | null>("installed");
	const { data } = usePackages();

	const handleViewPackage = (name: string, version: string) => {
		navigate(
			`/ packages / ${encodeURIComponent(name)}/${encodeURIComponent(version)}`,
		);
	};

	const serverVersion = data?.serverFhirVersion || "4.0.1";

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Group justify="space-between">
				<div>
					<Title order={2}>FHIR Packages</Title>
					<Text c="dimmed" size="sm">
						Manage installed FHIR packages and install new packages from the
						registry
					</Text>
				</div>
				<Badge size="lg" variant="light" color="warm">
					Server: FHIR {serverVersion}
				</Badge>
			</Group>

			<Tabs value={activeTab} onChange={setActiveTab}>
				<Tabs.List>
					<Tabs.Tab value="installed" leftSection={<IconPackage size={14} />}>
						Installed
						{data && (
							<Badge size="xs" variant="light" ml="xs">
								{data.packages.length}
							</Badge>
						)}
					</Tabs.Tab>
					<Tabs.Tab value="registry" leftSection={<IconWorld size={14} />}>
						Registry
					</Tabs.Tab>
				</Tabs.List>

				<Tabs.Panel value="installed" pt="md">
					<InstalledPackagesTab
						serverVersion={serverVersion}
						onView={handleViewPackage}
					/>
				</Tabs.Panel>

				<Tabs.Panel value="registry" pt="md">
					<RegistryTab serverVersion={serverVersion} />
				</Tabs.Panel>
			</Tabs>
		</Stack>
	);
}
