import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
	Stack,
	Text,
	Paper,
	Group,
	Badge,
	DataPreview,
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
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { notifications } from "@octofhir/ui-kit";
import {
	CircleExclamation,
	Magnifier,
	Box,
	Eye,
	Check,
	TriangleExclamation,
	ArrowDownToLine,
	Globe,
} from "@gravity-ui/icons";
import {
	usePackages,
	usePackageLookup,
	usePackageSearch,
	useInstallPackageWithProgress,
} from "@/shared/api/hooks";
import {
	filterFhirPackages,
	getFhirPackageInstalledViews,
	getFhirPackageRegistryViews,
	getFhirPackageVersionOptions,
	getFhirVersionCompatibilityView,
} from "@/entities/fhir-package";
import type { PackageInfo } from "@/shared/api/types";
import { InstallProgressModal } from "./InstallProgressModal";

const singleLineTextStyle = {
	display: "-webkit-box",
	WebkitBoxOrient: "vertical",
	WebkitLineClamp: 1,
	overflow: "hidden",
};

function FhirVersionBadge({
	packageVersion,
	serverVersion,
}: {
	packageVersion?: string;
	serverVersion: string;
}) {
	const compatibility = getFhirVersionCompatibilityView(packageVersion, serverVersion);

	return (
		<Tooltip label={compatibility.tooltip}>
			<Badge
				size="sm"
				variant="light"
				color={compatibility.isCompatible ? "primary" : "warm"}
				leftSection={
					compatibility.isCompatible ? (
						<Check size={12} />
					) : (
						<TriangleExclamation size={12} />
					)
				}
			>
				{compatibility.label}
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
			style={{ backgroundColor: "var(--octo-surface-1)" }}
		>
			<Group justify="space-between" mb="xs">
				<Group gap="xs">
					<ThemeIcon variant="light" size="md" color="warm">
						<Box size={16} />
					</ThemeIcon>
					<Text fw={500} style={singleLineTextStyle}>
						{pkg.name}
					</Text>
				</Group>
				<Tooltip label="View package details">
					<ActionIcon
						variant="subtle"
						size="sm"
						onClick={() => onView(pkg.name, pkg.version)}
					>
						<Eye size={16} />
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
		return filterFhirPackages(data?.packages ?? [], search);
	}, [data, search]);
	const packageViews = useMemo(
		() => getFhirPackageInstalledViews(filteredPackages, serverVersion),
		[filteredPackages, serverVersion],
	);

	return (
		<Stack gap="sm">
			<TextInput
				placeholder="Search installed packages..."
				leftSection={<Magnifier size={16} />}
				value={search}
				onChange={(e) => setSearch(e.currentTarget.value)}
				style={{ maxWidth: 460 }}
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
					icon={<CircleExclamation size={16} />}
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
						<Paper p="md" style={{ backgroundColor: "var(--octo-surface-2)" }}>
							<Stack align="center" gap="sm">
								<ThemeIcon
									size={rem(40)}
									radius="xl"
									variant="light"
									color="warm"
								>
									<Box size={22} />
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
						<Paper style={{ backgroundColor: "var(--octo-surface-1)" }}>
							<DataPreview
								columns={[
									{ id: "package", label: "Package" },
									{ id: "version", label: "Version", width: 120 },
									{ id: "fhirVersion", label: "FHIR Version", width: 150 },
									{ id: "resources", label: "Resources", width: 110 },
									{ id: "installed", label: "Installed", width: 140 },
									{ id: "actions", label: "", width: 50 },
								]}
								rows={packageViews.map((pkg) => ({
									package: (
										<Group gap="xs">
											<Box
												size={16}
												style={{ color: "var(--octo-accent-primary)" }}
											/>
											<Text fw={500}>{pkg.name}</Text>
										</Group>
									),
									version: (
										<Badge size="sm" variant="outline">
											{pkg.versionLabel}
										</Badge>
									),
									fhirVersion: (
										<FhirVersionBadge
											packageVersion={pkg.rawFhirVersion}
											serverVersion={serverVersion}
										/>
									),
									resources: <Text size="sm">{pkg.resourceCountLabel}</Text>,
									installed: (
										<Text size="sm" c="dimmed">
											{pkg.installedAtLabel}
										</Text>
									),
									actions: (
										<Tooltip label="View package details">
											<ActionIcon
												variant="subtle"
												size="sm"
												onClick={() => onView(pkg.name, pkg.rawVersion)}
											>
												<Eye size={16} />
											</ActionIcon>
										</Tooltip>
									),
								}))}
								getRowKey={(_row, index) => packageViews[index]?.id ?? `${index}`}
							/>
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

	const registryViews = useMemo(
		() => getFhirPackageRegistryViews(searchData?.packages ?? []),
		[searchData?.packages],
	);
	const versionOptions = useMemo(() => getFhirPackageVersionOptions(lookupData), [lookupData]);

	return (
		<Stack gap="sm">
			<TextInput
				placeholder="Search packages... (e.g., us core, hl7.fhir)"
				leftSection={<Magnifier size={16} />}
				value={searchQuery}
				onChange={(e) => {
					setSearchQuery(e.currentTarget.value);
					setSelectedPackage(null);
					setSelectedVersion(null);
				}}
				style={{ maxWidth: 520 }}
			/>

			{searchLoading && (
				<Group justify="center" py="xl">
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Searching packages...
					</Text>
				</Group>
			)}

			{searchData && searchData.packages.length > 0 && (
				<Paper style={{ backgroundColor: "var(--octo-surface-1)" }}>
					<DataPreview
						columns={[
							{ id: "package", label: "Package", width: 260 },
							{ id: "description", label: "Description" },
							{ id: "latest", label: "Latest", width: 120 },
							{ id: "version", label: "Version", width: 220 },
							{ id: "actions", label: "", width: 110 },
						]}
						rows={registryViews.map((pkg) => ({
							package: (
								<Group gap="xs">
									<Box
										size={16}
										style={{ color: "var(--octo-accent-primary)" }}
									/>
									<Text fw={500} size="sm">
										{pkg.name}
									</Text>
								</Group>
							),
							description: (
								<Text size="sm" c="dimmed" style={singleLineTextStyle}>
									{pkg.descriptionLabel}
								</Text>
							),
							latest: (
								<Badge size="sm" variant="outline">
									{pkg.latestVersionLabel}
								</Badge>
							),
							version:
								selectedPackage === pkg.name ? (
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
								),
							actions: (
								<Button
									size="xs"
									leftSection={<ArrowDownToLine size={14} />}
									onClick={handleStartInstall}
									disabled={selectedPackage !== pkg.name || !selectedVersion}
								>
									Install
								</Button>
							),
						}))}
						getRowKey={(_row, index) => registryViews[index]?.id ?? `${index}`}
					/>
				</Paper>
			)}

			{searchData &&
				searchData.packages.length === 0 &&
				searchQuery.length >= 2 && (
					<Alert
						icon={<CircleExclamation size={16} />}
						color="warm"
						variant="light"
					>
						No packages found matching "{searchQuery}". Try a different search
						term.
					</Alert>
				)}

			{!searchData && !searchLoading && (
				<Paper p="md" style={{ backgroundColor: "var(--octo-surface-2)" }}>
					<Stack align="center" gap="sm">
						<ThemeIcon size={rem(40)} radius="xl" variant="light" color="fire">
							<Globe size={22} />
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
			`/packages/${encodeURIComponent(name)}/${encodeURIComponent(version)}`,
		);
	};

	const serverVersion = data?.serverFhirVersion || "4.0.1";

	return (
		<WorkspacePageLayout
			title="FHIR Packages"
			description="Manage installed FHIR packages and install new packages from the registry"
			actions={
				<Badge size="lg" variant="light" color="warm">
					Server: FHIR {serverVersion}
				</Badge>
			}
		>

			<Tabs value={activeTab} onChange={setActiveTab}>
				<Tabs.List>
					<Tabs.Tab value="installed" leftSection={<Box size={14} />}>
						Installed
						{data && (
							<Badge size="xs" variant="light" ml="xs">
								{data.packages.length}
							</Badge>
						)}
					</Tabs.Tab>
					<Tabs.Tab value="registry" leftSection={<Globe size={14} />}>
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
		</WorkspacePageLayout>
	);
}
