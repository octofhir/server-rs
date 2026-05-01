import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
	Stack,
	Title,
	Text,
	Paper,
	Group,
	Badge,
	DataPreview,
	Loader,
	Alert,
	TextInput,
	SegmentedControl,
	ActionIcon,
	Tooltip,
	Code,
	Accordion,
	Select,
	Anchor,
	ScrollArea,
} from "@/shared/ui";
import {
	CircleExclamation,
	Magnifier,
	Eye,
	Lock,
	LockOpen,
	Server,
	Code as CodeIcon,
	Database,
	Shield,
	Boxes3,
	Cpu,
} from "@gravity-ui/icons";
import {
	filterOperations,
	getOperationAppOptions,
	getOperationCategoryView,
	getOperationMethodView,
	groupOperationsByCategory,
	operationAccessFilterOptions,
	type GroupedOperations,
	type OperationAccessFilter,
} from "@/entities/operation-catalog";
import { useOperations } from "@/shared/api/hooks";
import type { OperationDefinition } from "@/shared/api/types";

const CATEGORY_ICONS: Record<string, typeof Server> = {
	fhir: Server,
	graphql: CodeIcon,
	system: Database,
	auth: Shield,
	ui: Boxes3,
	api: Cpu,
};

function MethodBadge({ method }: { method: string }) {
	const methodView = getOperationMethodView(method);

	return (
		<Badge size="xs" variant="light" color={methodView.color}>
			{methodView.method}
		</Badge>
	);
}

function CategorySection({
	category,
	operations,
	onViewOperation,
	onNavigateToApp,
}: {
	category: string;
	operations: OperationDefinition[];
	onViewOperation: (id: string) => void;
	onNavigateToApp: (appId: string) => void;
}) {
	const Icon = CATEGORY_ICONS[category] ?? Cpu;
	const categoryView = getOperationCategoryView(category);

	return (
		<Accordion.Item value={category}>
			<Accordion.Control>
				<Group gap="sm">
					<Icon size={20} color="var(--g-color-text-secondary)" />
					<Text fw={500}>{categoryView.label}</Text>
					<Badge size="sm" variant="light" color={categoryView.color}>
						{operations.length}
					</Badge>
				</Group>
			</Accordion.Control>
			<Accordion.Panel>
				<DataPreview
					columns={[
						{ id: "id", label: "ID", width: 180 },
						{ id: "name", label: "Name" },
						{ id: "methods", label: "Methods", width: 180 },
						{ id: "path", label: "Path" },
						{ id: "app", label: "App", width: 180 },
						{ id: "access", label: "Access", width: 90 },
						{ id: "actions", label: "", width: 50 },
					]}
					rows={operations.map((operation) => ({
						id: <Code>{operation.id}</Code>,
						name: (
							<>
								<Text size="sm" fw={500}>
									{operation.name}
								</Text>
								{operation.description && (
									<Text size="xs" c="dimmed" lineClamp={1}>
										{operation.description}
									</Text>
								)}
							</>
						),
						methods: (
							<Group gap={4}>
								{operation.methods.map((method) => (
									<MethodBadge key={method} method={method} />
								))}
							</Group>
						),
						path: <Code size="xs">{operation.path_pattern}</Code>,
						app: operation.app ? (
							<Anchor
								size="sm"
								onClick={() => onNavigateToApp(operation.app?.id ?? "")}
								style={{ cursor: "pointer" }}
							>
								{operation.app.name}
							</Anchor>
						) : (
							<Text size="sm" c="dimmed">-</Text>
						),
						access: (
							<Tooltip label={operation.public ? "Public (no auth required)" : "Protected (requires auth)"}>
								{operation.public ? (
									<LockOpen size={16} style={{ color: "var(--octo-accent-primary)" }} />
								) : (
									<Lock size={16} style={{ color: "var(--octo-accent-warm)" }} />
								)}
							</Tooltip>
						),
						actions: (
							<Tooltip label="View details">
								<ActionIcon variant="subtle" size="sm" onClick={() => onViewOperation(operation.id)}>
									<Eye size={16} />
								</ActionIcon>
							</Tooltip>
						),
					}))}
					getRowKey={(_row, index) => operations[index]?.id ?? `${index}`}
				/>
			</Accordion.Panel>
		</Accordion.Item>
	);
}

export function OperationsPage() {
	const navigate = useNavigate();
	const [search, setSearch] = useState("");
	const [filterAccess, setFilterAccess] = useState<OperationAccessFilter>("all");
	const [filterApp, setFilterApp] = useState<string | null>(null);
	const { data, isLoading, error } = useOperations();

	// Extract unique apps for the filter dropdown
	const appOptions = useMemo(() => {
		return getOperationAppOptions(data?.operations);
	}, [data]);

	const filteredAndGrouped = useMemo(() => {
		if (!data?.operations) return {} as GroupedOperations;
		return groupOperationsByCategory(
			filterOperations(data.operations, search, filterAccess, filterApp),
		);
	}, [data, search, filterAccess, filterApp]);

	const totalFiltered = Object.values(filteredAndGrouped).flat().length;
	const categories = Object.keys(filteredAndGrouped).sort();

	const handleViewOperation = (id: string) => {
		navigate(`/operations/${encodeURIComponent(id)}`);
	};

	const handleNavigateToApp = (appId: string) => {
		navigate(`/apps/${appId}`);
	};

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<div>
				<Title order={2}>Operations</Title>
				<Text c="dimmed" size="sm">
					View and manage server API operations
				</Text>
			</div>

			<Paper p="md" style={{ backgroundColor: "var(--octo-surface-2)" }}>
				<Group gap="md">
					<TextInput
						placeholder="Search operations..."
						leftSection={<Magnifier size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1 }}
					/>
					{appOptions.length > 0 && (
						<Select
							placeholder="All Apps"
							clearable
							data={appOptions}
							value={filterApp}
							onChange={setFilterApp}
							w={180}
						/>
					)}
					<SegmentedControl
						value={filterAccess}
						onChange={(value) => setFilterAccess(value as OperationAccessFilter)}
						data={operationAccessFilterOptions}
					/>
				</Group>
			</Paper>

			{isLoading && (
				<Group justify="center" py="xl">
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading operations...
					</Text>
				</Group>
			)}

			{error && (
				<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
					{error instanceof Error ? error.message : "Failed to load operations"}
				</Alert>
			)}

			{!isLoading && !error && data && (
				<>
					<Group justify="space-between">
						<Text size="sm" c="dimmed">
							{totalFiltered} operations in {categories.length} categories
						</Text>
						{data.total !== totalFiltered && (
							<Text size="sm" c="dimmed">
								(filtered from {data.total} total)
							</Text>
						)}
					</Group>

					{categories.length === 0 ? (
						<Paper p="xl" style={{ backgroundColor: "var(--octo-surface-2)" }}>
							<Text ta="center" c="dimmed">
								No operations match your filters
							</Text>
						</Paper>
					) : (
						<ScrollArea style={{ flex: 1 }} offsetScrollbars>
							<Accordion multiple defaultValue={categories}>
								{categories.map((category) => (
									<CategorySection
										key={category}
										category={category}
										operations={filteredAndGrouped[category]}
										onViewOperation={handleViewOperation}
										onNavigateToApp={handleNavigateToApp}
									/>
								))}
							</Accordion>
						</ScrollArea>
					)}
				</>
			)}
		</Stack>
	);
}
