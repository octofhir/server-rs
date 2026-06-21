import { useMemo, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
	Text,
	Badge,
	DataPreview,
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
	Modal,
	Code,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
	filterFhirPackageResources,
	getFhirPackageResourceTypeOptions,
	getFhirPackageResourceViews,
} from "@/entities/fhir-package";
import { CircleAlert as CircleExclamation, Search as Magnifier, ArrowLeft, Check, TriangleAlert as TriangleExclamation, Eye, File, Code as CodeIcon } from "lucide-react";
import {
	usePackageDetails,
	usePackageResources,
	usePackageResourceContent,
	usePackageFhirSchema,
} from "@/shared/api/hooks";
import type { PackageResourceSummary } from "@/shared/api/types";
import classes from "./PackageDetailPage.module.css";

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
				leftSection={isCompatible ? <Check size={14} /> : <TriangleExclamation size={14} />}
			>
				FHIR {packageVersion || "unknown"}
			</Badge>
		</Tooltip>
	);
}

function ResourceTypeIcon({ resourceType }: { resourceType: string }) {
	return (
		<span className={classes.resourceTypeIcon} data-resource-type={resourceType}>
			<File size={14} />
		</span>
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
				<div className={classes.modalTitle}>
					<ResourceTypeIcon resourceType={resource.resourceType} />
					<Text fw={500}>{resource.name || resource.id || resourceUrl}</Text>
				</div>
			}
			size="xl"
			styles={{ body: { padding: 0, backgroundColor: "var(--octo-surface-1)" } }}
		>
			<Tabs value={activeTab} onChange={setActiveTab}>
				<Tabs.List>
					<Tabs.Tab value="json" leftSection={<CodeIcon width={14} />}>
						JSON
					</Tabs.Tab>
					{resource.resourceType === "StructureDefinition" && (
						<Tabs.Tab value="fhirschema" leftSection={<CodeIcon width={14} />}>
							FHIRSchema
						</Tabs.Tab>
					)}
				</Tabs.List>

				<Tabs.Panel value="json" p="md">
					{contentLoading ? (
						<div className={classes.modalState}>
							<Loader size="sm" />
							<Text size="sm" c="dimmed">
								Loading resource...
							</Text>
						</div>
					) : content ? (
						<div className={classes.codeScroll}>
							<Code block className={classes.codeBlockSmall}>
								{JSON.stringify(content, null, 2)}
							</Code>
						</div>
					) : (
						<Text c="dimmed">Failed to load resource content</Text>
					)}
				</Tabs.Panel>

				{resource.resourceType === "StructureDefinition" && (
					<Tabs.Panel value="fhirschema" p="md">
						{schemaLoading ? (
							<div className={classes.modalState}>
								<Loader size="sm" />
								<Text size="sm" c="dimmed">
									Loading FHIRSchema...
								</Text>
							</div>
						) : fhirSchema ? (
							<div className={classes.codeScroll}>
								<Code block className={classes.codeBlockSmall}>
									{JSON.stringify(fhirSchema, null, 2)}
								</Code>
							</div>
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

	const filteredResources = useMemo(
		() => filterFhirPackageResources(data?.resources ?? [], search),
		[data?.resources, search],
	);
	const resourceViews = useMemo(
		() => getFhirPackageResourceViews(filteredResources),
		[filteredResources],
	);
	const typeOptions = useMemo(
		() => getFhirPackageResourceTypeOptions(resourceTypes),
		[resourceTypes],
	);

	return (
		<div className={classes.tabStack}>
			<div className={classes.filters}>
				<TextInput
					placeholder="Search resources..."
					leftSection={<Magnifier size={16} />}
					value={search}
					onChange={(e) => setSearch(e.currentTarget.value)}
					className={classes.searchInput}
				/>
				<Select
					placeholder="Filter by type"
					data={typeOptions}
					value={filterType}
					onChange={setFilterType}
					clearable
					className={classes.typeSelect}
				/>
			</div>

			{isLoading && (
				<div className={classes.statePanel}>
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading resources...
					</Text>
				</div>
			)}

			{error && (
				<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
					{error instanceof Error ? error.message : "Failed to load resources"}
				</Alert>
			)}

			{!isLoading && !error && (
				<div className={classes.tablePanel}>
					<DataPreview
						columns={[
							{ id: "type", label: "Type", width: 220 },
							{ id: "name", label: "Name", width: 240 },
							{ id: "url", label: "URL" },
							{ id: "version", label: "Version", width: 120 },
							{ id: "actions", label: "", width: 50 },
						]}
						rows={resourceViews.map((resource, index) => ({
							type: (
								<div className={classes.resourceCell}>
									<ResourceTypeIcon resourceType={resource.resourceType} />
									<Text size="sm">{resource.resourceType}</Text>
								</div>
							),
							name: (
								<Text size="sm" fw={500}>
									{resource.nameLabel}
								</Text>
							),
							url: (
								<Text size="xs" c="dimmed" className={classes.truncateText}>
									{resource.urlLabel}
								</Text>
							),
							version: <Text size="sm">{resource.versionLabel}</Text>,
							actions: (
								<Tooltip label="View resource">
									<ActionIcon
										variant="subtle"
										size="sm"
										onClick={() => {
											const selected = filteredResources[index];
											if (selected) setSelectedResource(selected);
										}}
									>
										<Eye size={16} />
									</ActionIcon>
								</Tooltip>
							),
						}))}
						emptyText="No resources found"
						getRowKey={(_row, index) => resourceViews[index]?.id ?? `${index}`}
					/>
				</div>
			)}

			{selectedResource && (
				<ResourceViewer
					packageName={packageName}
					packageVersion={packageVersion}
					resource={selectedResource}
					onClose={() => setSelectedResource(null)}
				/>
			)}
		</div>
	);
}

export function PackageDetailPage() {
	const { name, version } = useParams<{ name: string; version: string }>();
	const navigate = useNavigate();
	const [activeTab, setActiveTab] = useState<string | null>("overview");

	const { data, isLoading, error } = usePackageDetails(name || "", version || "");

	if (!name || !version) {
		return (
			<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
				Invalid package parameters
			</Alert>
		);
	}

	return (
		<WorkspacePageLayout
			title={name}
			description={`Version ${version}`}
			kicker={
				<Breadcrumbs>
					<Anchor onClick={() => navigate("/packages")}>Packages</Anchor>
					<Text>{name}</Text>
				</Breadcrumbs>
			}
			actions={
				<div className={classes.headerActions}>
					<Button
						variant="subtle"
						leftSection={<ArrowLeft size={16} />}
						onClick={() => navigate("/packages")}
					>
						Back
					</Button>
					{data && <FhirVersionBadge packageVersion={data.fhirVersion} isCompatible={data.isCompatible} />}
				</div>
			}
			maxWidth={1280}
		>

			{isLoading && (
				<div className={classes.statePanel}>
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading package details...
					</Text>
				</div>
			)}

			{error && (
				<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
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
						<div className={classes.tabStack}>
							<div className={classes.panel}>
								<div className={classes.panelStack}>
									{data.description && (
										<div>
											<Text size="sm" fw={500} c="dimmed">
												Description
											</Text>
											<Text size="sm">{data.description}</Text>
										</div>
									)}

									<div className={classes.metrics}>
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
									</div>
								</div>
							</div>

							<div className={classes.panelMuted}>
								<Text size="sm" fw={500} c="dimmed" mb="sm">
									Resource Types
								</Text>
								<div className={classes.resourceTypes}>
									{data.resourceTypes.map((rt) => (
										<Badge key={rt.resourceType} variant="light" size="lg" color="primary">
											{rt.resourceType}: {rt.count}
										</Badge>
									))}
								</div>
							</div>
						</div>
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
		</WorkspacePageLayout>
	);
}
