import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
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
	SegmentedControl,
	ActionIcon,
	Tooltip,
	Code,
	Accordion,
} from "@mantine/core";
import {
	IconAlertCircle,
	IconSearch,
	IconEye,
	IconLock,
	IconLockOpen,
	IconServer,
	IconCode,
	IconDatabase,
	IconShield,
	IconApps,
	IconApi,
} from "@tabler/icons-react";
import { useOperations } from "@/shared/api/hooks";
import type { OperationDefinition } from "@/shared/api/types";

const CATEGORY_ICONS: Record<string, typeof IconServer> = {
	fhir: IconServer,
	graphql: IconCode,
	system: IconDatabase,
	auth: IconShield,
	ui: IconApps,
	api: IconApi,
};

const CATEGORY_COLORS: Record<string, string> = {
	fhir: "blue",
	graphql: "grape",
	system: "teal",
	auth: "red",
	ui: "orange",
	api: "cyan",
};

const CATEGORY_LABELS: Record<string, string> = {
	fhir: "FHIR REST API",
	graphql: "GraphQL",
	system: "System",
	auth: "Authentication",
	ui: "UI API",
	api: "Custom API",
};

interface GroupedOperations {
	[category: string]: OperationDefinition[];
}

function MethodBadge({ method }: { method: string }) {
	const colors: Record<string, string> = {
		GET: "green",
		POST: "blue",
		PUT: "orange",
		DELETE: "red",
		PATCH: "yellow",
	};

	return (
		<Badge size="xs" variant="light" color={colors[method] ?? "gray"}>
			{method}
		</Badge>
	);
}

function OperationRow({
	operation,
	onView,
}: {
	operation: OperationDefinition;
	onView: (id: string) => void;
}) {
	return (
		<Table.Tr key={operation.id}>
			<Table.Td>
				<Group gap="xs">
					<Code>{operation.id}</Code>
				</Group>
			</Table.Td>
			<Table.Td>
				<Text size="sm" fw={500}>
					{operation.name}
				</Text>
				{operation.description && (
					<Text size="xs" c="dimmed" lineClamp={1}>
						{operation.description}
					</Text>
				)}
			</Table.Td>
			<Table.Td>
				<Group gap={4}>
					{operation.methods.map((method) => (
						<MethodBadge key={method} method={method} />
					))}
				</Group>
			</Table.Td>
			<Table.Td>
				<Code size="xs">{operation.path_pattern}</Code>
			</Table.Td>
			<Table.Td>
				<Tooltip label={operation.public ? "Public (no auth required)" : "Protected (requires auth)"}>
					{operation.public ? (
						<IconLockOpen size={16} color="var(--mantine-color-green-6)" />
					) : (
						<IconLock size={16} color="var(--mantine-color-gray-5)" />
					)}
				</Tooltip>
			</Table.Td>
			<Table.Td>
				<Tooltip label="View details">
					<ActionIcon variant="subtle" size="sm" onClick={() => onView(operation.id)}>
						<IconEye size={16} />
					</ActionIcon>
				</Tooltip>
			</Table.Td>
		</Table.Tr>
	);
}

function CategorySection({
	category,
	operations,
	onViewOperation,
}: {
	category: string;
	operations: OperationDefinition[];
	onViewOperation: (id: string) => void;
}) {
	const Icon = CATEGORY_ICONS[category] ?? IconApi;
	const color = CATEGORY_COLORS[category] ?? "gray";
	const label = CATEGORY_LABELS[category] ?? category;

	return (
		<Accordion.Item value={category}>
			<Accordion.Control>
				<Group gap="sm">
					<Icon size={20} color={`var(--mantine-color-${color}-6)`} />
					<Text fw={500}>{label}</Text>
					<Badge size="sm" variant="light" color={color}>
						{operations.length}
					</Badge>
				</Group>
			</Accordion.Control>
			<Accordion.Panel>
				<Table striped highlightOnHover>
					<Table.Thead>
						<Table.Tr>
							<Table.Th>ID</Table.Th>
							<Table.Th>Name</Table.Th>
							<Table.Th>Methods</Table.Th>
							<Table.Th>Path</Table.Th>
							<Table.Th>Access</Table.Th>
							<Table.Th w={50} />
						</Table.Tr>
					</Table.Thead>
					<Table.Tbody>
						{operations.map((op) => (
							<OperationRow key={op.id} operation={op} onView={onViewOperation} />
						))}
					</Table.Tbody>
				</Table>
			</Accordion.Panel>
		</Accordion.Item>
	);
}

export function OperationsPage() {
	const navigate = useNavigate();
	const [search, setSearch] = useState("");
	const [filterAccess, setFilterAccess] = useState<string>("all");
	const { data, isLoading, error } = useOperations();

	const filteredAndGrouped = useMemo(() => {
		if (!data?.operations) return {} as GroupedOperations;

		const searchLower = search.toLowerCase();
		const filtered = data.operations.filter((op) => {
			// Search filter
			const matchesSearch =
				!search ||
				op.id.toLowerCase().includes(searchLower) ||
				op.name.toLowerCase().includes(searchLower) ||
				op.description?.toLowerCase().includes(searchLower) ||
				op.path_pattern.toLowerCase().includes(searchLower);

			// Access filter
			const matchesAccess =
				filterAccess === "all" ||
				(filterAccess === "public" && op.public) ||
				(filterAccess === "protected" && !op.public);

			return matchesSearch && matchesAccess;
		});

		// Group by category
		return filtered.reduce((acc, op) => {
			const cat = op.category || "other";
			if (!acc[cat]) acc[cat] = [];
			acc[cat].push(op);
			return acc;
		}, {} as GroupedOperations);
	}, [data, search, filterAccess]);

	const totalFiltered = Object.values(filteredAndGrouped).flat().length;
	const categories = Object.keys(filteredAndGrouped).sort();

	const handleViewOperation = (id: string) => {
		navigate(`/operations/${encodeURIComponent(id)}`);
	};

	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<div>
				<Title order={2}>Operations</Title>
				<Text c="dimmed" size="sm">
					View and manage server API operations
				</Text>
			</div>

			<Paper withBorder p="md">
				<Group gap="md">
					<TextInput
						placeholder="Search operations..."
						leftSection={<IconSearch size={16} />}
						value={search}
						onChange={(e) => setSearch(e.currentTarget.value)}
						style={{ flex: 1 }}
					/>
					<SegmentedControl
						value={filterAccess}
						onChange={setFilterAccess}
						data={[
							{ label: "All", value: "all" },
							{ label: "Public", value: "public" },
							{ label: "Protected", value: "protected" },
						]}
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
				<Alert icon={<IconAlertCircle size={16} />} color="red" variant="light">
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
						<Paper withBorder p="xl">
							<Text ta="center" c="dimmed">
								No operations match your filters
							</Text>
						</Paper>
					) : (
						<Accordion multiple defaultValue={categories}>
							{categories.map((category) => (
								<CategorySection
									key={category}
									category={category}
									operations={filteredAndGrouped[category]}
									onViewOperation={handleViewOperation}
								/>
							))}
						</Accordion>
					)}
				</>
			)}
		</Stack>
	);
}
