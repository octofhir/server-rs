import { useState, useMemo, useEffect, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
	Stack,
	Title,
	Text,
	Paper,
	Group,
	Badge,
	Loader,
	Center,
	TextInput,
	Table,
	SegmentedControl,
	ActionIcon,
	Button,
	Breadcrumbs,
	Anchor,
	ScrollArea,
	Alert,
	Box,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import {
	IconSearch,
	IconChevronLeft,
	IconChevronRight,
	IconGripVertical,
	IconAlertCircle,
	IconFileDescription,
	IconCode,
} from "@tabler/icons-react";
import {
	useResourceTypesCategorized,
	useResourceSearch,
	useResource,
	useUpdateResource,
	useFollowBundleLink,
	useJsonSchema,
} from "@/shared/api/hooks";
import { JsonEditor } from "@/shared/monaco/JsonEditor";
import type { FhirResource, FhirBundle, FhirOperationOutcome } from "@/shared/api/types";
import { HttpError } from "@/shared/api/fhirClient";

type CategoryFilter = "all" | "fhir" | "system" | "custom";

const MIN_PANEL_WIDTH = 400;
const MAX_PANEL_WIDTH = 900;
const DEFAULT_PANEL_WIDTH = 600;

export function ResourceBrowserPage() {
	const { type: routeType, id: routeId } = useParams<{ type?: string; id?: string }>();
	const navigate = useNavigate();

	const selectedType = routeType ?? null;
	const selectedId = routeId ?? null;
	const [typeFilter, setTypeFilter] = useState("");
	const [categoryFilter, setCategoryFilter] = useState<CategoryFilter>("all");
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
			document.addEventListener("mousemove", (e) => handleMouseMove(e as unknown as MouseEvent));
			document.addEventListener("mouseup", handleMouseUp);
		}

		return () => {
			document.removeEventListener("mousemove", (e) => handleMouseMove(e as unknown as MouseEvent));
			document.removeEventListener("mouseup", handleMouseUp);
		};
	}, [isResizing]);

	// Memoize category filter data to avoid infinite re-renders
	const categoryFilterData = useMemo(() => [
		{
			value: "all",
			label: `All${categorizedTypes?.counts ? ` (${categorizedTypes.counts.all})` : ""}`,
		},
		{
			value: "fhir",
			label: `FHIR${categorizedTypes?.counts ? ` (${categorizedTypes.counts.fhir})` : ""}`,
		},
		{
			value: "system",
			label: `System${categorizedTypes?.counts ? ` (${categorizedTypes.counts.system})` : ""}`,
		},
		{
			value: "custom",
			label: `Custom${categorizedTypes?.counts ? ` (${categorizedTypes.counts.custom})` : ""}`,
		},
	], [categorizedTypes?.counts]);

	// Filter resource types by category and search
	const filteredTypes = useMemo(() => {
		if (!categorizedTypes?.types) return [];

		let types = categorizedTypes.types;

		// Filter by category
		if (categoryFilter !== "all") {
			types = types.filter((t) => t.category === categoryFilter);
		}

		// Filter by search
		if (typeFilter) {
			const lower = typeFilter.toLowerCase();
			types = types.filter((t) => t.name.toLowerCase().includes(lower));
		}

		return types;
	}, [categorizedTypes, categoryFilter, typeFilter]);

	// Extract resources from current bundle
	const resources = useMemo(() => {
		return (currentBundle?.entry?.map((e) => e.resource).filter(Boolean) ?? []) as FhirResource[];
	}, [currentBundle]);

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
			const parsed = JSON.parse(editedResource);
			await updateMutation.mutateAsync(parsed);
			setIsEditMode(false);
			notifications.show({
				title: "Success",
				message: "Resource updated successfully",
				color: "green",
			});
		} catch (error) {
			if (error instanceof HttpError) {
				const responseData = error.response.data as FhirOperationOutcome | undefined;
				if (responseData?.resourceType === "OperationOutcome") {
					setSaveError({
						message: error.message,
						operationOutcome: responseData,
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

	// Extract display value from resource
	const getResourceDisplayValue = (resource: FhirResource, field: string): string => {
		const value = resource[field];
		if (value === undefined || value === null) return "-";
		if (typeof value === "string") return value;
		if (typeof value === "boolean") return value ? "true" : "false";
		if (typeof value === "number") return String(value);
		return JSON.stringify(value);
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
				backgroundColor: "var(--app-surface-1)",
				overflow: "hidden"
			}}
		>
			<Box p="md" style={{ borderBottom: "1px solid var(--app-border-subtle)", backgroundColor: "var(--app-surface-2)" }}>
				<Group justify="space-between">
					<Group gap="md">
						<SegmentedControl
							size="sm"
							radius="md"
							value={categoryFilter}
							onChange={(val) => setCategoryFilter(val as CategoryFilter)}
							data={categoryFilterData}
						/>
						<TextInput
							placeholder="Search resources..."
							size="sm"
							radius="md"
							leftSection={<IconSearch size={14} />}
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
						<IconSearch size={40} style={{ opacity: 0.2 }} />
						<Text c="dimmed" fw={500}>No resource types found</Text>
					</Stack>
				</Center>
			) : (
				<ScrollArea style={{ flex: 1 }} className="custom-scrollbar">
					<Table highlightOnHover verticalSpacing="md" className="modern-table">
						<Table.Thead>
							<Table.Tr>
								<Table.Th style={{ width: 300 }}>Resource Type</Table.Th>
								<Table.Th>Definition URL</Table.Th>
								<Table.Th>Package</Table.Th>
								<Table.Th style={{ width: 120 }}>Category</Table.Th>
							</Table.Tr>
						</Table.Thead>
						<Table.Tbody>
							{filteredTypes.map((item) => (
								<Table.Tr
									key={item.name}
									onClick={() => handleTypeSelect(item.name)}
									style={{ cursor: "pointer" }}
								>
									<Table.Td>
										<Text fw={600} size="sm">{item.name}</Text>
									</Table.Td>
									<Table.Td>
										<Text size="xs" c="dimmed" ff="monospace" lineClamp={1}>
											{item.url ?? "-"}
										</Text>
									</Table.Td>
									<Table.Td>
										<Badge variant="light" size="xs" color="gray" radius="sm">
											{item.package}
										</Badge>
									</Table.Td>
									<Table.Td>
										<Badge
											size="xs"
											variant="filled"
											radius="sm"
											color={
												item.category === "fhir"
													? "primary"
													: item.category === "system"
														? "warm"
														: "fire"
											}
										>
											{item.category}
										</Badge>
									</Table.Td>
								</Table.Tr>
							))}
						</Table.Tbody>
					</Table>
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
				backgroundColor: "var(--app-surface-1)",
				flex: 1,
				display: "flex",
				flexDirection: "column",
				minHeight: 0,
				overflow: "hidden"
			}}
		>
			<Box p="md" style={{ borderBottom: "1px solid var(--app-border-subtle)", backgroundColor: "var(--app-surface-2)" }}>
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
									<IconChevronLeft size={16} />
								</ActionIcon>
								<ActionIcon
									variant="light"
									size="md"
									radius="md"
									disabled={!hasNextPage || followLinkMutation.isPending}
									onClick={handleNextPage}
								>
									<IconChevronRight size={16} />
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
						<IconFileDescription size={40} style={{ opacity: 0.2 }} />
						<Text c="dimmed" fw={500}>No resources found</Text>
					</Stack>
				</Center>
			) : (
				<ScrollArea style={{ flex: 1 }} className="custom-scrollbar">
					<Table highlightOnHover verticalSpacing="md" className="modern-table">
						<Table.Thead>
							<Table.Tr>
								<Table.Th style={{ width: 220 }}>ID</Table.Th>
								<Table.Th style={{ width: 120 }}>Status</Table.Th>
								<Table.Th>Last Updated</Table.Th>
								<Table.Th style={{ width: 100 }}>Version</Table.Th>
							</Table.Tr>
						</Table.Thead>
						<Table.Tbody>
							{resources.map((resource) => (
								<Table.Tr
									key={resource.id}
									onClick={() => resource.id && handleResourceSelect(resource.id)}
									style={{
										cursor: "pointer",
										backgroundColor:
											selectedId === resource.id
												? "var(--app-accent-warm-bg)"
												: undefined,
									}}
								>
									<Table.Td>
										<Text fw={600} size="xs" ff="monospace">
											{resource.id}
										</Text>
									</Table.Td>
									<Table.Td>
										<Badge variant="outline" size="xs" radius="sm">
											{getResourceDisplayValue(resource, "status")}
										</Badge>
									</Table.Td>
									<Table.Td>
										<Text size="xs" c="dimmed">
											{resource.meta?.lastUpdated
												? new Date(resource.meta.lastUpdated).toLocaleString()
												: "-"}
										</Text>
									</Table.Td>
									<Table.Td>
										<Badge variant="light" size="xs" color="gray">
											v{resource.meta?.versionId ?? "1"}
										</Badge>
									</Table.Td>
								</Table.Tr>
							))}
						</Table.Tbody>
					</Table>
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
					backgroundColor: isResizing ? "var(--app-accent-primary)" : "transparent",
				}}
				onMouseEnter={(e) => {
					if (!isResizing) e.currentTarget.style.backgroundColor = "var(--app-border-subtle)";
				}}
				onMouseLeave={(e) => {
					if (!isResizing) e.currentTarget.style.backgroundColor = "transparent";
				}}
			>
				<Box
					style={{
						width: 2,
						height: 32,
						backgroundColor: "var(--app-border-subtle)",
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
					backgroundColor: "var(--app-surface-1)",
					minHeight: 0,
					overflow: "hidden",
					boxShadow: "var(--mantine-shadow-xl)",
				}}
			>
				<Box p="md" style={{ borderBottom: "1px solid var(--app-border-subtle)", backgroundColor: "var(--app-surface-2)" }}>
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
									leftSection={<IconCode size={14} />}
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
								<IconGripVertical size={16} style={{ transform: "rotate(90deg)" }} />
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
								schema={jsonSchema as object | undefined}
								resourceType={selectedType ?? undefined}
							/>
						</Box>
						{saveError && (
							<Box p="md" style={{ borderTop: "1px solid var(--app-border-subtle)", backgroundColor: "var(--app-surface-2)" }}>
								<Alert
									color="red"
									icon={<IconAlertCircle size={16} />}
									radius="md"
									title={saveError.message}
								>
									{saveError.operationOutcome?.issue ? (
										<Stack gap="xs" mt="xs">
											{saveError.operationOutcome.issue.slice(0, 3).map((issue, idx) => (
												<Box key={idx} p="xs" style={{ backgroundColor: "rgba(255,0,0,0.05)", borderRadius: "8px", border: "1px solid rgba(255,0,0,0.1)" }}>
													<Group gap="xs" mb={4}>
														<Badge size="xs" color="red">{issue.severity}</Badge>
														<Text size="xs" fw={700}>{issue.code}</Text>
													</Group>
													{issue.diagnostics && (
														<Text size="xs" style={{ whiteSpace: "pre-wrap" }}>
															{issue.diagnostics}
														</Text>
													)}
												</Box>
											))}
										</Stack>
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
		<Box p="xl" h="100%" className="page-enter" style={{ display: "flex", flexDirection: "column" }}>
			<Box mb="xl">
				<Group justify="space-between" align="flex-end">
					<Box>
						<Title order={2} style={{ letterSpacing: "-0.02em" }}>Resource Browser</Title>
						<Breadcrumbs mt="xs" separator="→" style={{ fontSize: "12px" }}>
							{breadcrumbItems}
						</Breadcrumbs>
					</Box>
					{selectedType && !selectedId && (
						<Button
							variant="subtle"
							leftSection={<IconChevronLeft size={16} />}
							onClick={handleBackToTypes}
							radius="md"
						>
							Change Resource Type
						</Button>
					)}
				</Group>
			</Box>

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
		</Box>
	);
}
