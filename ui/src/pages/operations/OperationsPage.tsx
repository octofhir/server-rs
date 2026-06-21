import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
	Text,
	Badge,
	DataPreview,
	Skeleton,
	EmptyState,
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
import { CircleAlert as CircleExclamation, Search as Magnifier, Eye, Lock, LockOpen, Server, Code as CodeIcon, Database, Shield, Boxes as Boxes3, Cpu } from "lucide-react";
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
		<Badge size="xs" color={methodView.color}>
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
					<Icon
						width={20}
						height={20}
						color="var(--g-color-text-secondary)"
						aria-hidden="true"
					/>
					<Text>
						<strong>{categoryView.label}</strong>
					</Text>
					<Badge size="sm" color={categoryView.color}>
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
							<Text variant="body-2">
								<strong>{operation.name}</strong>
							</Text>
							{operation.description && (
								<Text
									variant="caption-2"
									color="secondary"
									ellipsis
									className={classes.truncateText}
								>
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
					path: <Code className={classes.pathCode}>{operation.path_pattern}</Code>,
					app: operation.app ? (
						<Anchor
							onClick={() => onNavigateToApp(operation.app?.id ?? "")}
							className={classes.appLink}
						>
							{operation.app.name}
						</Anchor>
					) : (
						<Text variant="body-2" color="secondary">
							-
						</Text>
					),
					access: (
						<Tooltip
							content={
								operation.public
									? "Public (no auth required)"
									: "Protected (requires auth)"
							}
						>
							{operation.public ? (
								<LockOpen
									width={16}
									height={16}
									className={classes.publicIcon}
									aria-label="Public (no auth required)"
								/>
							) : (
								<Lock
									width={16}
									height={16}
									className={classes.protectedIcon}
									aria-label="Protected (requires auth)"
								/>
							)}
						</Tooltip>
					),
					actions: (
						<Tooltip content="View details">
							<ActionIcon
								view="flat"
								size="s"
								aria-label={`View details for ${operation.name}`}
								onClick={() => onViewOperation(operation.id)}
							>
								<Eye width={16} height={16} aria-hidden="true" />
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
	const { data, isLoading, isError, error, refetch } = useOperations();

	const hasActiveFilters =
		search.trim() !== "" || filterAccess !== "all" || filterApp !== null;

	const clearFilters = () => {
		setSearch("");
		setFilterAccess("all");
		setFilterApp(null);
	};

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
						aria-label="Search operations"
						leftSection={<Magnifier width={16} height={16} aria-hidden="true" />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						className={classes.searchInput}
					/>
					{appOptions.length > 0 && (
						<Select
							placeholder="All Apps"
							aria-label="Filter by app"
							clearable
							data={appOptions}
							value={filterApp}
							onChange={setFilterApp}
							className={classes.appFilter}
						/>
					)}
					<SegmentedControl
						aria-label="Filter by access level"
						value={filterAccess}
						onUpdate={(value) => {
							if (isOperationAccessFilter(value)) {
								setFilterAccess(value);
							}
						}}
						options={operationAccessFilterOptions.map((option) => ({
							value: option.value,
							content: option.label,
						}))}
					/>
				</div>
			}
		>

			{isLoading && (
				<div className={classes.categoryGrid} aria-busy="true">
					{[0, 1, 2].map((section) => (
						<SectionPanel key={section} padding="s" className={classes.categorySection}>
							<div className={classes.categoryHeader}>
								<div className={classes.categoryTitle}>
									<Skeleton className={classes.skeletonIcon} />
									<Skeleton className={classes.skeletonTitle} />
									<Skeleton className={classes.skeletonBadge} />
								</div>
							</div>
							<div className={classes.skeletonRows}>
								{[0, 1, 2, 3].map((row) => (
									<Skeleton key={row} className={classes.skeletonRow} />
								))}
							</div>
						</SectionPanel>
					))}
				</div>
			)}

			{isError && (
				<EmptyState
					image={<CircleExclamation width={48} height={48} aria-hidden="true" />}
					title="Failed to load operations"
					description={
						error instanceof Error
							? error.message
							: "Something went wrong while loading operations."
					}
					actions={[
						{
							text: "Retry",
							view: "action",
							onClick: () => {
								void refetch();
							},
						},
					]}
				/>
			)}

			{!isLoading && !isError && data && (
				<>
					<div className={classes.resultSummary}>
						<Text variant="body-2" color="secondary">
							{totalFiltered} operations in {categories.length} categories
						</Text>
						{data.total !== totalFiltered && (
							<Text variant="body-2" color="secondary">
								(filtered from {data.total} total)
							</Text>
						)}
					</div>

					{categories.length === 0 ? (
						hasActiveFilters ? (
							<EmptyState
								image={<Magnifier width={48} height={48} aria-hidden="true" />}
								title="No operations match your filters"
								description="Try a different search term or clear the active filters."
								actions={[
									{
										text: "Clear filters",
										view: "outlined",
										onClick: clearFilters,
									},
								]}
							/>
						) : (
							<EmptyState
								image={<Cpu width={48} height={48} aria-hidden="true" />}
								title="No operations available"
								description="No server API operations have been registered yet."
							/>
						)
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
