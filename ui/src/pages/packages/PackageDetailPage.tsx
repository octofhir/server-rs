import { useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
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
	Tabs,
	Button,
	Select,
	Breadcrumbs,
	Anchor,
	ThemeIcon,
	Modal,
	ScrollArea,
	Code,
} from "@/shared/ui";
import {
	IconAlertCircle,
	IconSearch,
	IconPackage,
	IconArrowLeft,
	IconCheck,
	IconAlertTriangle,
	IconEye,
	IconFile,
	IconCode,
} from "@tabler/icons-react";
import {
	usePackageDetails,
	usePackageResources,
	usePackageResourceContent,
	usePackageFhirSchema,
} from "@/shared/api/hooks";
import type { PackageResourceSummary } from "@/shared/api/types";

function FhirVersionBadge({
	packageVersion,
	isCompatible,
}: {
	packageVersion?: string;
	isCompatible: boolean;
}) {
	return (
		<Tooltip label={isCompatible ? "Compatible with server" : "Version mismatch with server"}>
			<Badge
				size="md"
				variant="light"
				color={isCompatible ? "primary" : "warm"}
				leftSection={isCompatible ? <IconCheck size={14} /> : <IconAlertTriangle size={14} />}
			>
				FHIR {packageVersion || "unknown"}
			</Badge>
		</Tooltip>
	);
}

function ResourceTypeIcon({ resourceType }: { resourceType: string }) {
	const colors: Record<string, string> = {
		StructureDefinition: "primary",
		ValueSet: "deep",
		CodeSystem: "warm",
		SearchParameter: "primary",
		OperationDefinition: "fire",
		CapabilityStatement: "deep",
		CompartmentDefinition: "warm",
		NamingSystem: "fire",
	};

	return (
		<ThemeIcon variant="light" size="sm" color={colors[resourceType] || "deep"}>
			<IconFile size={14} />
		</ThemeIcon>
	);
}

interface ResourceViewerProps {
	packageName: string;
	packageVersion: string;
	resource: PackageResourceSummary;
	onClose: () => void;
}

function ResourceViewer({ packageName, packageVersion, resource, onClose }: ResourceViewerProps) {
	const [activeTab, setActiveTab] = useState<string | null>("json");
	const resourceUrl = resource.url || resource.id || "";

	const { data: content, isLoading: contentLoading } = usePackageResourceContent(
		packageName,
		packageVersion,
		resourceUrl,
	);

	const { data: fhirSchema, isLoading: schemaLoading } = usePackageFhirSchema(
		packageName,
		packageVersion,
		resourceUrl,
	);

	return (
		<Modal
			opened
			onClose={onClose}
			title={
				<Group gap="xs">
					<ResourceTypeIcon resourceType={resource.resourceType} />
					<Text fw={500}>{resource.name || resource.id || resourceUrl}</Text>
				</Group>
			}
			size="xl"
			styles={{ body: { padding: 0, backgroundColor: "var(--octo-surface-1)" } }}
		>
			<Tabs value={activeTab} onChange={setActiveTab}>
				<Tabs.List>
					<Tabs.Tab value="json" leftSection={<IconCode size={14} />}>
						JSON
					</Tabs.Tab>
					{resource.resourceType === "StructureDefinition" && (
						<Tabs.Tab value="fhirschema" leftSection={<IconCode size={14} />}>
							FHIRSchema
						</Tabs.Tab>
					)}
				</Tabs.List>

				<Tabs.Panel value="json" p="md">
					{contentLoading ? (
						<Group justify="center" py="xl">
							<Loader size="sm" />
							<Text size="sm" c="dimmed">
								Loading resource...
							</Text>
						</Group>
					) : content ? (
						<ScrollArea h={400}>
							<Code block style={{ fontSize: "12px" }}>
								{JSON.stringify(content, null, 2)}
							</Code>
						</ScrollArea>
					) : (
						<Text c="dimmed">Failed to load resource content</Text>
					)}
				</Tabs.Panel>

				{resource.resourceType === "StructureDefinition" && (
					<Tabs.Panel value="fhirschema" p="md">
						{schemaLoading ? (
							<Group justify="center" py="xl">
								<Loader size="sm" />
								<Text size="sm" c="dimmed">
									Loading FHIRSchema...
								</Text>
							</Group>
						) : fhirSchema ? (
							<ScrollArea h={400}>
								<Code block style={{ fontSize: "12px" }}>
									{JSON.stringify(fhirSchema, null, 2)}
								</Code>
							</ScrollArea>
						) : (
							<Text c="dimmed">FHIRSchema not available for this resource</Text>
						)}
					</Tabs.Panel>
				)}
			</Tabs>
		</Modal>
	);
}

function ResourcesTab({
	packageName,
	packageVersion,
	resourceTypes,
}: {
	packageName: string;
	packageVersion: string;
	resourceTypes: Array<{ resourceType: string; count: number }>;
}) {
	const [search, setSearch] = useState("");
	const [filterType, setFilterType] = useState<string | null>(null);
	const [selectedResource, setSelectedResource] = useState<PackageResourceSummary | null>(null);

	const { data, isLoading, error } = usePackageResources(packageName, packageVersion, {
		resourceType: filterType || undefined,
		limit: 100,
	});

	const filteredResources = data?.resources.filter((r) => {
		if (!search) return true;
		const searchLower = search.toLowerCase();
		return (
			r.name?.toLowerCase().includes(searchLower) ||
			r.url?.toLowerCase().includes(searchLower) ||
			r.id?.toLowerCase().includes(searchLower)
		);
	});

	const typeOptions = [
		{ value: "", label: "All types" },
		...resourceTypes.map((rt) => ({
			value: rt.resourceType,
			label: `${rt.resourceType} (${rt.count})`,
		})),
	];

	return (
		<Stack gap="md">
			<Group gap="md">
				<TextInput
					placeholder="Search resources..."
					leftSection={<IconSearch size={16} />}
					value={search}
					onChange={(e) => setSearch(e.currentTarget.value)}
					style={{ flex: 1 }}
				/>
				<Select
					placeholder="Filter by type"
					data={typeOptions}
					value={filterType}
					onChange={setFilterType}
					clearable
					w={250}
				/>
			</Group>

			{isLoading && (
				<Group justify="center" py="xl">
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading resources...
					</Text>
				</Group>
			)}

			{error && (
				<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
					{error instanceof Error ? error.message : "Failed to load resources"}
				</Alert>
			)}

			{!isLoading && !error && filteredResources && (
				<Paper style={{ backgroundColor: "var(--octo-surface-1)" }}>
					<Table striped highlightOnHover>
						<Table.Thead>
							<Table.Tr>
								<Table.Th>Type</Table.Th>
								<Table.Th>Name</Table.Th>
								<Table.Th>URL</Table.Th>
								<Table.Th>Version</Table.Th>
								<Table.Th w={50} />
							</Table.Tr>
						</Table.Thead>
						<Table.Tbody>
							{filteredResources.length === 0 ? (
								<Table.Tr>
									<Table.Td colSpan={5}>
										<Text ta="center" c="dimmed" py="md">
											No resources found
										</Text>
									</Table.Td>
								</Table.Tr>
							) : (
								filteredResources.map((resource, idx) => (
									<Table.Tr key={resource.url || resource.id || idx}>
										<Table.Td>
											<Group gap="xs">
												<ResourceTypeIcon resourceType={resource.resourceType} />
												<Text size="sm">{resource.resourceType}</Text>
											</Group>
										</Table.Td>
										<Table.Td>
											<Text size="sm" fw={500}>
												{resource.name || "-"}
											</Text>
										</Table.Td>
										<Table.Td>
											<Text size="xs" c="dimmed" lineClamp={1}>
												{resource.url || "-"}
											</Text>
										</Table.Td>
										<Table.Td>
											<Text size="sm">{resource.version || "-"}</Text>
										</Table.Td>
										<Table.Td>
											<Tooltip label="View resource">
												<ActionIcon
													variant="subtle"
													size="sm"
													onClick={() => setSelectedResource(resource)}
												>
													<IconEye size={16} />
												</ActionIcon>
											</Tooltip>
										</Table.Td>
									</Table.Tr>
								))
							)}
						</Table.Tbody>
					</Table>
				</Paper>
			)}

			{selectedResource && (
				<ResourceViewer
					packageName={packageName}
					packageVersion={packageVersion}
					resource={selectedResource}
					onClose={() => setSelectedResource(null)}
				/>
			)}
		</Stack>
	);
}

export function PackageDetailPage() {
	const { name, version } = useParams<{ name: string; version: string }>();
	const navigate = useNavigate();
	const [activeTab, setActiveTab] = useState<string | null>("overview");

	const { data, isLoading, error } = usePackageDetails(name || "", version || "");

	if (!name || !version) {
		return (
			<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
				Invalid package parameters
			</Alert>
		);
	}

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Breadcrumbs>
				<Anchor onClick={() => navigate("/packages")}>Packages</Anchor>
				<Text>{name}</Text>
			</Breadcrumbs>

			<Group justify="space-between">
				<Group gap="md">
					<Button
						variant="subtle"
						leftSection={<IconArrowLeft size={16} />}
						onClick={() => navigate("/packages")}
					>
						Back
					</Button>
					<Group gap="xs">
						<ThemeIcon variant="light" size="lg" color="warm">
							<IconPackage size={20} />
						</ThemeIcon>
						<div>
							<Title order={2}>{name}</Title>
							<Text size="sm" c="dimmed">
								Version {version}
							</Text>
						</div>
					</Group>
				</Group>

				{data && <FhirVersionBadge packageVersion={data.fhirVersion} isCompatible={data.isCompatible} />}
			</Group>

			{isLoading && (
				<Group justify="center" py="xl">
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading package details...
					</Text>
				</Group>
			)}

			{error && (
				<Alert icon={<IconAlertCircle size={16} />} color="fire" variant="light">
					{error instanceof Error ? error.message : "Failed to load package"}
				</Alert>
			)}

			{!isLoading && !error && data && (
				<Tabs value={activeTab} onChange={setActiveTab}>
					<Tabs.List>
						<Tabs.Tab value="overview">Overview</Tabs.Tab>
						<Tabs.Tab value="resources">
							Resources
							<Badge size="sm" variant="light" color="warm" ml="xs">
								{data.resourceCount}
							</Badge>
						</Tabs.Tab>
					</Tabs.List>

					<Tabs.Panel value="overview" pt="md">
						<Stack gap="md">
							<Paper p="md" style={{ backgroundColor: "var(--octo-surface-1)" }}>
								<Stack gap="sm">
									{data.description && (
										<div>
											<Text size="sm" fw={500} c="dimmed">
												Description
											</Text>
											<Text size="sm">{data.description}</Text>
										</div>
									)}

									<Group gap="xl">
										<div>
											<Text size="sm" fw={500} c="dimmed">
												Total Resources
											</Text>
											<Text size="lg" fw={500}>
												{data.resourceCount}
											</Text>
										</div>
										{data.installedAt && (
											<div>
												<Text size="sm" fw={500} c="dimmed">
													Installed
												</Text>
												<Text size="sm">{new Date(data.installedAt).toLocaleString()}</Text>
											</div>
										)}
									</Group>
								</Stack>
							</Paper>

							<Paper p="md" style={{ backgroundColor: "var(--octo-surface-2)" }}>
								<Text size="sm" fw={500} c="dimmed" mb="sm">
									Resource Types
								</Text>
								<Group gap="xs">
									{data.resourceTypes.map((rt) => (
										<Badge key={rt.resourceType} variant="light" size="lg" color="primary">
											{rt.resourceType}: {rt.count}
										</Badge>
									))}
								</Group>
							</Paper>
						</Stack>
					</Tabs.Panel>

					<Tabs.Panel value="resources" pt="md">
						<ResourcesTab
							packageName={name}
							packageVersion={version}
							resourceTypes={data.resourceTypes}
						/>
					</Tabs.Panel>
				</Tabs>
			)}
		</Stack>
	);
}
