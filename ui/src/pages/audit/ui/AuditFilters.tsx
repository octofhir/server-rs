import { memo } from "react";
import {
	Group,
	TextInput,
	Button,
	Menu,
	ActionIcon,
	Tooltip,
	Badge,
	Popover,
	Stack,
	Text,
	Chip,
	Select,
} from "@mantine/core";
import { DateTimePicker } from "@mantine/dates";
import {
	IconSearch,
	IconFilter,
	IconDownload,
	IconRefresh,
	IconX,
	IconClock,
	IconBraces,
	IconFileSpreadsheet,
	IconUser,
	IconServer,
	IconAppWindow,
} from "@tabler/icons-react";
import type { AuditEventUIFilters, AuditAction, AuditOutcome } from "@/shared/api/types";
import classes from "./AuditFilters.module.css";

interface AuditFiltersProps {
	filters: AuditEventUIFilters;
	totalCount: number;
	isLoading?: boolean;
	onFiltersChange: (filters: Partial<AuditEventUIFilters>) => void;
	onRefresh: () => void;
	onExport: (format: "json" | "csv") => void;
}

const ACTION_CATEGORIES = {
	user: {
		label: "User Actions",
		icon: IconUser,
		actions: ["user.login", "user.logout", "user.login_failed"] as AuditAction[],
	},
	resource: {
		label: "Resource Actions",
		icon: IconServer,
		actions: [
			"resource.create",
			"resource.read",
			"resource.update",
			"resource.delete",
			"resource.search",
		] as AuditAction[],
	},
	client: {
		label: "Client Actions",
		icon: IconAppWindow,
		actions: [
			"client.auth",
			"client.create",
			"client.update",
			"client.delete",
		] as AuditAction[],
	},
	system: {
		label: "System Actions",
		icon: IconServer,
		actions: [
			"policy.evaluate",
			"config.change",
			"system.startup",
			"system.shutdown",
		] as AuditAction[],
	},
};

const OUTCOMES: { value: AuditOutcome; label: string; color: string }[] = [
	{ value: "success", label: "Success", color: "green" },
	{ value: "failure", label: "Failure", color: "red" },
	{ value: "partial", label: "Partial", color: "yellow" },
];

const ACTOR_TYPES = [
	{ value: "user", label: "User" },
	{ value: "client", label: "Client" },
	{ value: "system", label: "System" },
] as const;

function getActionLabel(action: AuditAction): string {
	const labels: Record<AuditAction, string> = {
		"user.login": "Login",
		"user.logout": "Logout",
		"user.login_failed": "Login Failed",
		"resource.create": "Create",
		"resource.read": "Read",
		"resource.update": "Update",
		"resource.delete": "Delete",
		"resource.search": "Search",
		"policy.evaluate": "Policy Check",
		"client.auth": "Client Auth",
		"client.create": "Client Create",
		"client.update": "Client Update",
		"client.delete": "Client Delete",
		"config.change": "Config Change",
		"system.startup": "Startup",
		"system.shutdown": "Shutdown",
	};
	return labels[action] || action;
}

function AuditFiltersComponent({
	filters,
	totalCount,
	isLoading,
	onFiltersChange,
	onRefresh,
	onExport,
}: AuditFiltersProps) {
	const handleActionToggle = (action: AuditAction) => {
		const current = filters.action || [];
		const newActions = current.includes(action)
			? current.filter((a) => a !== action)
			: [...current, action];
		onFiltersChange({ action: newActions.length > 0 ? newActions : undefined });
	};

	const handleOutcomeToggle = (outcome: AuditOutcome) => {
		const current = filters.outcome || [];
		const newOutcomes = current.includes(outcome)
			? current.filter((o) => o !== outcome)
			: [...current, outcome];
		onFiltersChange({ outcome: newOutcomes.length > 0 ? newOutcomes : undefined });
	};

	const handleActorTypeChange = (value: string | null) => {
		onFiltersChange({
			actorType: value ? [value as "user" | "client" | "system"] : undefined,
		});
	};

	const handleSearchChange = (value: string) => {
		onFiltersChange({ search: value || undefined });
	};

	const hasActiveFilters =
		(filters.action?.length ?? 0) > 0 ||
		(filters.outcome?.length ?? 0) > 0 ||
		(filters.actorType?.length ?? 0) > 0 ||
		filters.startTime ||
		filters.endTime ||
		filters.resourceType ||
		filters.ipAddress;

	const clearAllFilters = () => {
		onFiltersChange({
			search: undefined,
			action: undefined,
			outcome: undefined,
			actorType: undefined,
			startTime: undefined,
			endTime: undefined,
			resourceType: undefined,
			resourceId: undefined,
			ipAddress: undefined,
			actorId: undefined,
		});
	};

	return (
		<div className={classes.container}>
			<Group gap="sm" wrap="nowrap" className={classes.filtersRow}>
				<TextInput
					placeholder="Search events..."
					leftSection={<IconSearch size={14} />}
					value={filters.search || ""}
					onChange={(e) => handleSearchChange(e.currentTarget.value)}
					className={classes.searchInput}
					size="sm"
				/>

				{/* Action Filter */}
				<Menu shadow="md" width={280} position="bottom-start" closeOnItemClick={false}>
					<Menu.Target>
						<Button
							variant="light"
							leftSection={<IconFilter size={14} />}
							size="sm"
							className={classes.filterButton}
						>
							Actions
							{(filters.action?.length ?? 0) > 0 && (
								<Badge size="xs" ml={4} variant="filled" color="primary">
									{filters.action?.length}
								</Badge>
							)}
						</Button>
					</Menu.Target>
					<Menu.Dropdown>
						{Object.entries(ACTION_CATEGORIES).map(([key, category]) => (
							<div key={key}>
								<Menu.Label>{category.label}</Menu.Label>
								<Group gap={4} p="xs" wrap="wrap">
									{category.actions.map((action) => (
										<Chip
											key={action}
											checked={filters.action?.includes(action) ?? false}
											onChange={() => handleActionToggle(action)}
											size="xs"
											variant="light"
										>
											{getActionLabel(action)}
										</Chip>
									))}
								</Group>
							</div>
						))}
					</Menu.Dropdown>
				</Menu>

				{/* Outcome Filter */}
				<Menu shadow="md" width={200} position="bottom-start" closeOnItemClick={false}>
					<Menu.Target>
						<Button
							variant="light"
							size="sm"
							className={classes.filterButton}
						>
							Outcome
							{(filters.outcome?.length ?? 0) > 0 && (
								<Badge size="xs" ml={4} variant="filled" color="primary">
									{filters.outcome?.length}
								</Badge>
							)}
						</Button>
					</Menu.Target>
					<Menu.Dropdown>
						<Menu.Label>Outcome</Menu.Label>
						<Group gap={4} p="xs">
							{OUTCOMES.map((outcome) => (
								<Chip
									key={outcome.value}
									checked={filters.outcome?.includes(outcome.value) ?? false}
									onChange={() => handleOutcomeToggle(outcome.value)}
									size="xs"
									color={outcome.color}
									variant="light"
								>
									{outcome.label}
								</Chip>
							))}
						</Group>
					</Menu.Dropdown>
				</Menu>

				{/* Actor Type Filter */}
				<Select
					placeholder="Actor Type"
					data={ACTOR_TYPES}
					value={filters.actorType?.[0] || null}
					onChange={handleActorTypeChange}
					clearable
					size="sm"
					w={140}
				/>

				{/* Time Range Filter */}
				<Popover width={320} position="bottom-start" shadow="md">
					<Popover.Target>
						<Button
							variant="light"
							leftSection={<IconClock size={14} />}
							size="sm"
							className={classes.filterButton}
						>
							Time Range
							{(filters.startTime || filters.endTime) && (
								<Badge size="xs" ml={4} variant="filled" color="primary">
									Set
								</Badge>
							)}
						</Button>
					</Popover.Target>
					<Popover.Dropdown>
						<Stack gap="sm">
							<Text size="sm" fw={600}>
								Time Range Filter
							</Text>
							<DateTimePicker
								label="From"
								placeholder="Start time"
								size="xs"
								value={filters.startTime ? new Date(filters.startTime) : null}
								onChange={(date) =>
									onFiltersChange({ startTime: date?.toISOString() })
								}
								clearable
								maxDate={filters.endTime ? new Date(filters.endTime) : undefined}
							/>
							<DateTimePicker
								label="To"
								placeholder="End time"
								size="xs"
								value={filters.endTime ? new Date(filters.endTime) : null}
								onChange={(date) =>
									onFiltersChange({ endTime: date?.toISOString() })
								}
								clearable
								minDate={filters.startTime ? new Date(filters.startTime) : undefined}
							/>
							{(filters.startTime || filters.endTime) && (
								<Button
									variant="subtle"
									size="xs"
									leftSection={<IconX size={12} />}
									onClick={() =>
										onFiltersChange({ startTime: undefined, endTime: undefined })
									}
								>
									Clear Time Range
								</Button>
							)}
						</Stack>
					</Popover.Dropdown>
				</Popover>

				<div className={classes.spacer} />

				{hasActiveFilters && (
					<Button
						variant="subtle"
						size="sm"
						color="gray"
						leftSection={<IconX size={14} />}
						onClick={clearAllFilters}
					>
						Clear filters
					</Button>
				)}

				<Badge
					variant="light"
					color="primary"
					size="lg"
					radius="sm"
				>
					{totalCount.toLocaleString()} events
				</Badge>

				<div className={classes.divider} />

				<Group gap={4}>
					<Tooltip label="Refresh">
						<ActionIcon
							variant="light"
							color="gray"
							size="lg"
							onClick={onRefresh}
							loading={isLoading}
						>
							<IconRefresh size={18} />
						</ActionIcon>
					</Tooltip>

					<Menu shadow="md" width={160} position="bottom-end">
						<Menu.Target>
							<Tooltip label="Export audit logs">
								<ActionIcon variant="light" color="gray" size="lg">
									<IconDownload size={18} />
								</ActionIcon>
							</Tooltip>
						</Menu.Target>
						<Menu.Dropdown>
							<Menu.Label>Export Format</Menu.Label>
							<Menu.Item
								leftSection={<IconBraces size={14} />}
								onClick={() => onExport("json")}
							>
								JSON
							</Menu.Item>
							<Menu.Item
								leftSection={<IconFileSpreadsheet size={14} />}
								onClick={() => onExport("csv")}
							>
								CSV
							</Menu.Item>
						</Menu.Dropdown>
					</Menu>
				</Group>
			</Group>
		</div>
	);
}

export const AuditFilters = memo(AuditFiltersComponent);
