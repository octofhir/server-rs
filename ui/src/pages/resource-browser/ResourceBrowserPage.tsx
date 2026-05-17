import { useState, useMemo, useEffect, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
	Stack,
	Text,
	Paper,
	Group,
	Badge,
	Loader,
	Center,
	TextInput,
	SegmentedControl,
	ActionIcon,
	Button,
	Breadcrumbs,
	Anchor,
	ScrollArea,
	Alert,
	Box,
	RecordList,
} from "@/shared/ui";
import { ToolWorkspaceLayout } from "@/widgets/tool-workspace";
import { OperationOutcomePanel, notifications } from "@octofhir/ui-kit";
import {
	Magnifier,
	ChevronLeft,
	ChevronRight,
	GripHorizontal,
	CircleExclamation,
	FileText,
	Code,
} from "@gravity-ui/icons";
import {
	useResourceTypesCategorized,
	useResourceSearch,
	useResource,
	useUpdateResource,
	useFollowBundleLink,
	useJsonSchema,
} from "@/shared/api/hooks";
import {
	filterFhirCatalogTypes,
	getFhirCatalogCategoryOptions,
	getFhirCatalogTypeViews,
	type FhirCatalogCategoryFilter,
} from "@/entities/fhir-catalog";
import {
	getFhirBundleResources,
	getFhirResourceListViews,
} from "@/entities/fhir-resource";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import type { FhirBundle, FhirOperationOutcome } from "@/shared/api/types";
import { HttpError } from "@/shared/api/fhirClient";
import { assertFhirResource, isRecord } from "@/shared/api/guards";

const MIN_PANEL_WIDTH = 400;
const MAX_PANEL_WIDTH = 900;
const DEFAULT_PANEL_WIDTH = 600;

function isCatalogCategoryFilter(value: string): value is FhirCatalogCategoryFilter {
	return value === "all" || value === "fhir" || value === "system" || value === "custom";
}

function isOperationOutcome(value: unknown): value is FhirOperationOutcome {
	return isRecord(value) && value.resourceType === "OperationOutcome";
}

export function ResourceBrowserPage() {
	const { type: routeType, id: routeId } = useParams<{ type?: string; id?: string }>();
	const navigate = useNavigate();

	const selectedType = routeType ?? null;
	const selectedId = routeId ?? null;
	const [typeFilter, setTypeFilter] = useState("");
	const [categoryFilter, setCategoryFilter] = useState<FhirCatalogCategoryFilter>("all");
	const [isEditMode, setIsEditMode] = useState(false);
	const [editedResource, setEditedResource] = useState("");
	const [currentBundle, setCurrentBundle] = useState<FhirBundle | null>(null);
	const [panelWidth, setPanelWidth] = useState(DEFAULT_PANEL_WIDTH);
	const [isResizing, setIsResizing] = useState(false);
	const [saveError, setSaveError] = useState<{
		message: string;
		operationOutcome?: FhirOperationOutcome;
	} | null>(null);

	const { data: categorizedTypes, isLoading: resourceTypesLoading } =
		useResourceTypesCategorized();
	const { data: searchBundle, isLoading: searchLoading } = useResourceSearch(
		selectedType ?? "",
		{ _count: 50 },
		{ enabled: !!selectedType },
	);
	const { data: selectedResource, isLoading: resourceLoading } = useResource(
		selectedType ?? "",
		selectedId ?? "",
		{ enabled: !!selectedType && !!selectedId },
	);
	const { data: jsonSchema } = useJsonSchema(selectedType ?? undefined);
	const jsonSchemaObject = isRecord(jsonSchema) ? jsonSchema : undefined;
	const updateMutation = useUpdateResource();
	const followLinkMutation = useFollowBundleLink();

	// Reset browser state when the route resource type changes
	useEffect(() => {
		setCurrentBundle(null);
		setIsEditMode(false);
		setSaveError(null);
	}, [selectedType]);

	// Update current bundle when search bundle changes
	useEffect(() => {
		if (searchBundle) {
			setCurrentBundle(searchBundle);
		}
	}, [searchBundle]);

	// Update edited resource when selected resource changes
	useEffect(() => {
		if (selectedResource) {
			setEditedResource(JSON.stringify(selectedResource, null, 2));
			setIsEditMode(false);
		}
	}, [selectedResource]);

	// Resize handlers
	const handleMouseDown = useCallback(() => {
		setIsResizing(true);
	}, []);

	useEffect(() => {
		const handleMouseMove = (e: MouseEvent) => {
			if (!isResizing) return;
			const newWidth = window.innerWidth - e.clientX - 32; // 32 for padding
			setPanelWidth(Math.min(MAX_PANEL_WIDTH, Math.max(MIN_PANEL_WIDTH, newWidth)));
		};

		const handleMouseUp = () => {
			setIsResizing(false);
		};

		if (isResizing) {
			document.addEventListener("mousemove", handleMouseMove);
			document.addEventListener("mouseup", handleMouseUp);
		}

		return () => {
			document.removeEventListener("mousemove", handleMouseMove);
			document.removeEventListener("mouseup", handleMouseUp);
		};
	}, [isResizing]);

	// Memoize category filter data to avoid infinite re-renders
	const categoryFilterData = useMemo(() => [
		...getFhirCatalogCategoryOptions(categorizedTypes),
	], [categorizedTypes]);

	// Filter resource types by category and search
	const filteredTypes = useMemo(() => {
		return filterFhirCatalogTypes(categorizedTypes, categoryFilter, typeFilter);
	}, [categorizedTypes, categoryFilter, typeFilter]);
	const filteredTypeViews = useMemo(
		() => getFhirCatalogTypeViews(filteredTypes),
		[filteredTypes],
	);

	// Extract resources from current bundle
	const resources = useMemo(() => {
		return getFhirBundleResources(currentBundle);
	}, [currentBundle]);
	const resourceViews = useMemo(
		() => getFhirResourceListViews(resources),
		[resources],
	);

	// Pagination state
	const hasNextPage = currentBundle?.link?.some((l) => l.relation === "next") ?? false;
	const hasPrevPage = currentBundle?.link?.some((l) => l.relation === "prev") ?? false;

	const handleTypeSelect = (type: string) => {
		setCurrentBundle(null);
		setIsEditMode(false);
		setSaveError(null);
		navigate(`/resources/${type}`);
	};

	const handleResourceSelect = (id: string) => {
		if (!selectedType) return;
		navigate(`/resources/${selectedType}/${id}`);
	};

	const handleBackToTypes = () => {
		setCurrentBundle(null);
		setIsEditMode(false);
		setSaveError(null);
		navigate("/resources");
	};

	const handleCloseDetails = () => {
		setIsEditMode(false);
		setSaveError(null);
		if (selectedType) {
			navigate(`/resources/${selectedType}`);
			return;
		}
		navigate("/resources");
	};

	const handleNextPage = async () => {
		if (currentBundle) {
			const result = await followLinkMutation.mutateAsync({
				bundle: currentBundle,
				relation: "next",
			});
			if (result) {
				setCurrentBundle(result);
				if (selectedType) {
					navigate(`/resources/${selectedType}`);
				}
			}
		}
	};

	const handlePrevPage = async () => {
		if (currentBundle) {
			const result = await followLinkMutation.mutateAsync({
				bundle: currentBundle,
				relation: "prev",
			});
			if (result) {
				setCurrentBundle(result);
				if (selectedType) {
					navigate(`/resources/${selectedType}`);
				}
			}
		}
	};

	const handleSave = async () => {
		setSaveError(null);
		try {
			const parsed = assertFhirResource(JSON.parse(editedResource), "Resource editor save");
			await updateMutation.mutateAsync(parsed);
			setIsEditMode(false);
			notifications.show({
				title: "Success",
				message: "Resource updated successfully",
				color: "green",
			});
		} catch (error) {
			if (error instanceof HttpError) {
				if (isOperationOutcome(error.response.data)) {
					setSaveError({
						message: error.message,
						operationOutcome: error.response.data,
					});
				} else {
					setSaveError({ message: error.message });
				}
			} else {
				const errorMessage = error instanceof Error ? error.message : "Failed to update resource";
				setSaveError({ message: errorMessage });
			}
		}
	};

	const handleCancel = () => {
		if (selectedResource) {
			setEditedResource(JSON.stringify(selectedResource, null, 2));
		}
		setIsEditMode(false);
	};

	// Breadcrumb items
	const breadcrumbItems = [
		<Anchor key="root" onClick={handleBackToTypes} size="sm">
			Resources
		</Anchor>,
	];
	if (selectedType) {
		breadcrumbItems.push(
			<Text key="type" size="sm" fw={selectedId ? undefined : 500}>
				{selectedType}
			</Text>,
		);
	}
	if (selectedId) {
		breadcrumbItems.push(
			<Text key="id" size="sm" fw={500}>
				{selectedId}
			</Text>,
		);
	}

	// Resource Types Table
	const renderResourceTypesTable = () => (
		<Paper
			withBorder
			p="0"
			radius="lg"
			style={{
				flex: 1,
				display: "flex",
				flexDirection: "column",
				minHeight: 0,
				backgroundColor: "var(--octo-surface-1)",
				overflow: "hidden"
			}}
		>
			<Box p="md" style={{ borderBottom: "1px solid var(--octo-border-subtle)", backgroundColor: "var(--octo-surface-2)" }}>
				<Group justify="space-between">
					<Group gap="md">
						<SegmentedControl
							size="sm"
							radius="md"
							value={categoryFilter}
							onChange={(val) => {
								if (isCatalogCategoryFilter(val)) {
									setCategoryFilter(val);
								}
							}}
							data={categoryFilterData}
						/>
						<TextInput
							placeholder="Search resources..."
							size="sm"
							radius="md"
							leftSection={<Magnifier size={14} />}
							value={typeFilter}
							onChange={(e) => setTypeFilter(e.currentTarget.value)}
							style={{ width: 240 }}
						/>
					</Group>
					<Badge size="lg" variant="dot" color="primary">
						{filteredTypes.length} Types
					</Badge>
				</Group>
			</Box>

			{resourceTypesLoading ? (
				<Center py={100}>
					<Loader size="lg" variant="dots" color="primary" />
				</Center>
			) : filteredTypes.length === 0 ? (
				<Center py={100}>
					<Stack align="center" gap="xs">
						<Magnifier size={40} style={{ opacity: 0.2 }} />
						<Text c="dimmed" fw={500}>No resource types found</Text>
					</Stack>
				</Center>
			) : (
				<ScrollArea style={{ flex: 1 }} className="custom-scrollbar">
					<Box p="md">
						<RecordList
							items={filteredTypeViews.map((item) => ({
								id: item.id,
								title: item.name,
								subtitle: item.packageName,
								description: item.definitionUrl ?? "No canonical URL",
								meta: [
									{
										id: "category",
										label: item.category,
										tone: item.categoryTone,
									},
								],
							}))}
							onSelect={(item) => handleTypeSelect(item.id)}
						/>
					</Box>
				</ScrollArea>
			)}
		</Paper>
	);

	// Resources Table
	const renderResourcesTable = () => (
		<Paper
			withBorder
			radius="lg"
			style={{
				backgroundColor: "var(--octo-surface-1)",
				flex: 1,
				display: "flex",
				flexDirection: "column",
				minHeight: 0,
				overflow: "hidden"
			}}
		>
			<Box p="md" style={{ borderBottom: "1px solid var(--octo-border-subtle)", backgroundColor: "var(--octo-surface-2)" }}>
				<Group justify="space-between">
					<Group gap="sm">
						{(hasNextPage || hasPrevPage) && (
							<Group gap={4}>
								<ActionIcon
									variant="light"
									size="md"
									radius="md"
									disabled={!hasPrevPage || followLinkMutation.isPending}
									onClick={handlePrevPage}
								>
									<ChevronLeft size={16} />
								</ActionIcon>
								<ActionIcon
									variant="light"
									size="md"
									radius="md"
									disabled={!hasNextPage || followLinkMutation.isPending}
									onClick={handleNextPage}
								>
									<ChevronRight size={16} />
								</ActionIcon>
							</Group>
						)}
						<Text size="sm" fw={600} c="dimmed" tt="uppercase" style={{ letterSpacing: "0.05em" }}>
							{selectedType}s
						</Text>
					</Group>
					{currentBundle && (
						<Badge size="md" variant="light" radius="sm" color="primary">
							{currentBundle.total ?? resources.length} Total
						</Badge>
					)}
				</Group>
			</Box>

			{searchLoading ? (
				<Center py={100}>
					<Loader size="lg" variant="dots" color="primary" />
				</Center>
			) : resources.length === 0 ? (
				<Center py={100}>
					<Stack align="center" gap="xs">
						<FileText size={40} style={{ opacity: 0.2 }} />
						<Text c="dimmed" fw={500}>No resources found</Text>
					</Stack>
				</Center>
			) : (
				<ScrollArea style={{ flex: 1 }} className="custom-scrollbar">
					<Box p="md">
						<RecordList
							density="compact"
							selectedId={selectedId ?? undefined}
							items={resourceViews.map((resource) => ({
								id: resource.id,
								title: resource.resourceId ?? "(no id)",
								subtitle: resource.resourceType,
								description: resource.lastUpdatedLabel,
								disabled: !resource.canOpen,
								meta: [
									{
										id: "status",
										label: resource.statusLabel,
										tone: "neutral",
									},
									{
										id: "version",
										label: resource.versionLabel,
										tone: "info",
									},
								],
							}))}
							onSelect={(item) => handleResourceSelect(item.id)}
						/>
					</Box>
				</ScrollArea>
			)}
		</Paper>
	);

	// Details Panel with resize handle
	const renderDetailsPanel = () => (
		<Box
			style={{
				display: "flex",
				height: "100%",
				animation: "softAppear 0.3s cubic-bezier(0.16, 1, 0.3, 1)"
			}}
		>
			{/* Resize handle */}
			<Box
				onMouseDown={handleMouseDown}
				style={{
					width: 12,
					cursor: "col-resize",
					display: "flex",
					alignItems: "center",
					justifyContent: "center",
					position: "relative",
					zIndex: 20,
					transition: "background-color 0.2s ease",
					backgroundColor: isResizing ? "var(--octo-accent-primary)" : "transparent",
				}}
				onMouseEnter={(e) => {
					if (!isResizing) e.currentTarget.style.backgroundColor = "var(--octo-border-subtle)";
				}}
				onMouseLeave={(e) => {
					if (!isResizing) e.currentTarget.style.backgroundColor = "transparent";
				}}
			>
				<Box
					style={{
						width: 2,
						height: 32,
						backgroundColor: "var(--octo-border-subtle)",
						borderRadius: 2,
						opacity: isResizing ? 0 : 1
					}}
				/>
			</Box>
			<Paper
				withBorder
				radius="lg"
				style={{
					width: panelWidth,
					display: "flex",
					flexDirection: "column",
					backgroundColor: "var(--octo-surface-1)",
					minHeight: 0,
					overflow: "hidden",
					boxShadow: "var(--octo-shadow-xl)",
				}}
			>
				<Box p="md" style={{ borderBottom: "1px solid var(--octo-border-subtle)", backgroundColor: "var(--octo-surface-2)" }}>
					<Group justify="space-between">
						<Group gap="xs">
							<Badge size="lg" radius="sm" variant="gradient" gradient={{ from: "primary", to: "fire", deg: 135 }}>
								{selectedType}
							</Badge>
							<Text fw={600} size="sm" ff="monospace" c="dimmed">
								{selectedId}
							</Text>
						</Group>
						<Group gap="xs">
							{isEditMode ? (
								<Group gap={6}>
									<Button
										size="xs"
										variant="subtle"
										color="gray"
										radius="md"
										onClick={handleCancel}
										disabled={updateMutation.isPending}
									>
										Cancel
									</Button>
									<Button
										size="xs"
										radius="md"
										onClick={handleSave}
										loading={updateMutation.isPending}
									>
										Save Resource
									</Button>
								</Group>
							) : (
								<Button
									size="xs"
									variant="light"
									radius="md"
									leftSection={<Code size={14} />}
									onClick={() => {
										setSaveError(null);
										setIsEditMode(true);
									}}
								>
									Edit JSON
								</Button>
							)}
							<ActionIcon
								variant="subtle"
								color="gray"
								radius="md"
								size="md"
								onClick={handleCloseDetails}
							>
								<GripHorizontal size={16} style={{ transform: "rotate(90deg)" }} />
							</ActionIcon>
						</Group>
					</Group>
				</Box>

				{resourceLoading ? (
					<Center py={100}>
						<Loader size="lg" variant="dots" color="primary" />
					</Center>
				) : (
					<Box style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
						<Box style={{ flex: 1, minHeight: 0 }}>
							<JsonEditor
								value={editedResource}
								onChange={isEditMode ? setEditedResource : undefined}
								readOnly={!isEditMode}
								height="100%"
								schema={jsonSchemaObject}
								resourceType={selectedType ?? undefined}
							/>
						</Box>
						{saveError && (
							<Box p="md" style={{ borderTop: "1px solid var(--octo-border-subtle)", backgroundColor: "var(--octo-surface-2)" }}>
								<Alert
									color="red"
									icon={<CircleExclamation size={16} />}
									radius="md"
									title={saveError.message}
								>
									{saveError.operationOutcome?.issue ? (
										<OperationOutcomePanel
											outcome={saveError.operationOutcome}
											maxIssues={3}
										/>
									) : (
										<Text size="xs">An error occurred while saving.</Text>
									)}
								</Alert>
							</Box>
						)}
					</Box>
				)}
			</Paper>
		</Box>
	);

	return (
		<ToolWorkspaceLayout
			title="Resource Browser"
			description="Browse, inspect, and edit FHIR resources"
			className="page-enter"
			kicker={
				<Breadcrumbs separator="→" style={{ fontSize: "12px" }}>
					{breadcrumbItems}
				</Breadcrumbs>
			}
			actions={
				selectedType && !selectedId ? (
						<Button
							variant="subtle"
							leftSection={<ChevronLeft size={16} />}
							onClick={handleBackToTypes}
							radius="md"
						>
							Change Resource Type
						</Button>
				) : null
			}
		>

			<Box style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
				{!selectedType ? (
					// Level 1: Resource Types Table
					renderResourceTypesTable()
				) : (
					// Level 2: Resources Table (+ optional Details Panel)
					<Box style={{ display: "flex", flex: 1, minHeight: 0, gap: 0 }}>
						<Box style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
							{renderResourcesTable()}
						</Box>
						{selectedId && renderDetailsPanel()}
					</Box>
				)}
			</Box>
		</ToolWorkspaceLayout>
	);
}
