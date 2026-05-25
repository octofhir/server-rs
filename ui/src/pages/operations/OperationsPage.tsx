import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
	Text,
	Badge,
	DataPreview,
	Loader,
	Alert,
	TextInput,
	SegmentedControl,
	ActionIcon,
	Tooltip,
	Code,
	Select,
	Anchor,
	SectionPanel,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import classes from "./OperationsPage.module.css";
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

function isOperationAccessFilter(value: string): value is OperationAccessFilter {
	return operationAccessFilterOptions.some((option) => option.value === value);
}

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
		<SectionPanel padding="s" className={classes.categorySection}>
			<div className={classes.categoryHeader}>
				<div className={classes.categoryTitle}>
					<Icon size={20} color="var(--g-color-text-secondary)" />
					<Text fw={500}>{categoryView.label}</Text>
					<Badge size="sm" variant="light" color={categoryView.color}>
						{operations.length}
					</Badge>
				</div>
			</div>
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
						<div className={classes.nameCell}>
							<Text size="sm" fw={500}>
								{operation.name}
							</Text>
							{operation.description && (
								<Text size="xs" c="dimmed" className={classes.truncateText}>
									{operation.description}
								</Text>
							)}
						</div>
					),
					methods: (
						<div className={classes.methodList}>
							{operation.methods.map((method) => (
								<MethodBadge key={method} method={method} />
							))}
						</div>
					),
					path: <Code size="xs">{operation.path_pattern}</Code>,
					app: operation.app ? (
						<Anchor
							size="sm"
							onClick={() => onNavigateToApp(operation.app?.id ?? "")}
							className={classes.appLink}
						>
							{operation.app.name}
						</Anchor>
					) : (
						<Text size="sm" c="dimmed">-</Text>
					),
					access: (
						<Tooltip label={operation.public ? "Public (no auth required)" : "Protected (requires auth)"}>
							{operation.public ? (
								<LockOpen size={16} className={classes.publicIcon} />
							) : (
								<Lock size={16} className={classes.protectedIcon} />
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
		</SectionPanel>
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
		if (!data?.operations) {
			const empty: GroupedOperations = {};
			return empty;
		}
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
		<WorkspacePageLayout
			title="Operations"
			description="View and manage server API operations"
			toolbar={
				<div className={classes.toolbar}>
					<TextInput
						placeholder="Search operations..."
						leftSection={<Magnifier size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						className={classes.searchInput}
					/>
					{appOptions.length > 0 && (
						<Select
							placeholder="All Apps"
							clearable
							data={appOptions}
							value={filterApp}
							onChange={setFilterApp}
							className={classes.appFilter}
						/>
					)}
					<SegmentedControl
						value={filterAccess}
						onChange={(value) => {
							if (isOperationAccessFilter(value)) {
								setFilterAccess(value);
							}
						}}
						data={operationAccessFilterOptions}
					/>
				</div>
			}
		>

			{isLoading && (
				<div className={classes.loadingState}>
					<Loader size="sm" />
					<Text size="sm" c="dimmed">
						Loading operations...
					</Text>
				</div>
			)}

			{error && (
				<Alert icon={<CircleExclamation size={16} />} color="fire" variant="light">
					{error instanceof Error ? error.message : "Failed to load operations"}
				</Alert>
			)}

			{!isLoading && !error && data && (
				<>
					<div className={classes.resultSummary}>
						<Text size="sm" c="dimmed">
							{totalFiltered} operations in {categories.length} categories
						</Text>
						{data.total !== totalFiltered && (
							<Text size="sm" c="dimmed">
								(filtered from {data.total} total)
							</Text>
						)}
					</div>

					{categories.length === 0 ? (
						<div className={classes.emptyState}>
							<Text ta="center" c="dimmed">
								No operations match your filters
							</Text>
						</div>
					) : (
						<div className={classes.categoryGrid}>
							{categories.map((category) => (
								<CategorySection
									key={category}
									category={category}
									operations={filteredAndGrouped[category]}
									onViewOperation={handleViewOperation}
									onNavigateToApp={handleNavigateToApp}
								/>
							))}
						</div>
					)}
				</>
			)}
		</WorkspacePageLayout>
	);
}
