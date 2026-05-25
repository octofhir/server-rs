import { useState, useMemo, useEffect } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
	Text,
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
	RecordList,
	Resizable,
} from "@octofhir/ui-kit";
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
import classes from "./ResourceBrowserPage.module.css";

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
		<div className={classes.tablePanel}>
			<div className={classes.panelHeader}>
				<div className={classes.toolbar}>
					<div className={classes.filtersRow}>
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
							className={classes.searchInput}
						/>
					</div>
					<Badge size="lg" variant="dot" color="primary">
						{filteredTypes.length} Types
					</Badge>
				</div>
			</div>

			{resourceTypesLoading ? (
				<Center py={100}>
					<Loader size="lg" variant="dots" color="primary" />
				</Center>
			) : filteredTypes.length === 0 ? (
				<Center py={100}>
					<div className={classes.emptyContent}>
						<Magnifier size={40} className={classes.faintIcon} />
						<Text c="dimmed" fw={500}>No resource types found</Text>
					</div>
				</Center>
			) : (
				<ScrollArea className={`${classes.scrollArea} custom-scrollbar`}>
					<div className={classes.listPadding}>
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
					</div>
				</ScrollArea>
			)}
		</div>
	);

	// Resources Table
	const renderResourcesTable = () => (
		<div className={classes.tablePanel} style={{ height: "100%" }}>
			<div className={classes.panelHeader}>
				<div className={classes.toolbar}>
					<div className={classes.titleRow}>
						{(hasNextPage || hasPrevPage) && (
							<div className={classes.paginationActions}>
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
							</div>
						)}
						<Text size="sm" fw={600} c="dimmed" tt="uppercase" className={classes.overline}>
							{selectedType}s
						</Text>
					</div>
					{currentBundle && (
						<Badge size="md" variant="light" radius="sm" color="primary">
							{currentBundle.total ?? resources.length} Total
						</Badge>
					)}
				</div>
			</div>

			{searchLoading ? (
				<Center py={100}>
					<Loader size="lg" variant="dots" color="primary" />
				</Center>
			) : resources.length === 0 ? (
				<Center py={100}>
					<div className={classes.emptyContent}>
						<FileText size={40} className={classes.faintIcon} />
						<Text c="dimmed" fw={500}>No resources found</Text>
					</div>
				</Center>
			) : (
				<ScrollArea className={`${classes.scrollArea} custom-scrollbar`}>
					<div className={classes.listPadding}>
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
					</div>
				</ScrollArea>
			)}
		</div>
	);

	// Details Panel Content without manual resize handle
	const renderDetailsPanelContent = () => (
		<div className={classes.detailsPanel} style={{ width: "100%", height: "100%" }}>
			<div className={classes.panelHeader}>
				<div className={classes.toolbar}>
					<div className={classes.resourceIdentity}>
						<Badge size="lg" radius="sm" variant="gradient" gradient={{ from: "primary", to: "fire", deg: 135 }}>
							{selectedType}
						</Badge>
						<Text fw={600} size="sm" ff="monospace" c="dimmed">
							{selectedId}
						</Text>
					</div>
					<div className={classes.detailActions}>
						{isEditMode ? (
							<div className={classes.editActions}>
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
							</div>
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
							<GripHorizontal size={16} className={classes.closeIcon} />
						</ActionIcon>
					</div>
				</div>
			</div>

			{resourceLoading ? (
				<Center py={100}>
					<Loader size="lg" variant="dots" color="primary" />
				</Center>
			) : (
				<div className={classes.detailsBody}>
					<div className={classes.editorFill}>
						<JsonEditor
							value={editedResource}
							onChange={isEditMode ? setEditedResource : undefined}
							readOnly={!isEditMode}
							height="100%"
							schema={jsonSchemaObject}
							resourceType={selectedType ?? undefined}
						/>
					</div>
					{saveError && (
						<div className={classes.errorPanel}>
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
						</div>
					)}
				</div>
			)}
		</div>
	);

	return (
		<ToolWorkspaceLayout
			title="Resource Browser"
			description="Browse, inspect, and edit FHIR resources"
			className="page-enter"
			kicker={
				<Breadcrumbs separator="→" className={classes.breadcrumbs}>
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

			<div className={classes.pageBody}>
				{!selectedType ? (
					// Level 1: Resource Types Table
					renderResourceTypesTable()
				) : (
					// Level 2: Resources Table (+ optional Details Panel)
					<div className={classes.splitView}>
						<Resizable.Group orientation="horizontal">
							<Resizable.Pane defaultSize={selectedId ? 50 : 100} minSize={30}>
								{renderResourcesTable()}
							</Resizable.Pane>

							{selectedId && (
								<>
									<Resizable.Handle />
									<Resizable.Pane defaultSize={50} minSize={30}>
										<div className={classes.detailsShell} style={{ flex: 1, minWidth: 0 }}>
											{renderDetailsPanelContent()}
										</div>
									</Resizable.Pane>
								</>
							)}
						</Resizable.Group>
					</div>
				)}
			</div>
		</ToolWorkspaceLayout>
	);
}
