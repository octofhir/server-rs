import { useCallback } from "react";
import {
	Stack,
	Text,
	Group,
	Badge,
	ActionIcon,
	ScrollArea,
	Box,
	Tooltip,
	Table,
	Loader,
} from "@/shared/ui";
import {
	IconArrowLeft,
	IconTrash,
	IconKey,
	IconFingerprint,
} from "@tabler/icons-react";
import { useTableDetail, useDropIndex } from "@/shared/api/hooks";
import { modals, notifications } from "@octofhir/ui-kit";

interface TableDetailViewProps {
	schema: string;
	table: string;
	onBack: () => void;
}

function formatBytes(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function TableDetailView({ schema, table, onBack }: TableDetailViewProps) {
	const { data, isLoading } = useTableDetail(schema, table);
	const dropIndexMutation = useDropIndex();

	const handleDropIndex = useCallback(
		(indexName: string) => {
			modals.openConfirmModal({
				title: "Drop Index",
				children: (
					<Text size="sm">
						Are you sure you want to drop index <strong>{indexName}</strong>? This
						action cannot be undone.
					</Text>
				),
				labels: { confirm: "Drop Index", cancel: "Cancel" },
				confirmProps: { color: "red" },
				onConfirm: () => {
					dropIndexMutation.mutate(
						{ schema, indexName },
						{
							onSuccess: () => {
								notifications.show({
									title: "Index dropped",
									message: `${schema}.${indexName} has been removed`,
									color: "green",
								});
							},
							onError: (err) => {
								notifications.show({
									title: "Failed to drop index",
									message: err.message,
									color: "red",
								});
							},
						},
					);
				},
			});
		},
		[dropIndexMutation, schema],
	);

	return (
		<Stack gap={0} h="100%">
			<Group gap="xs" px="sm" py="xs" style={{ flexShrink: 0, borderBottom: "1px solid var(--octo-border-subtle)" }}>
				<ActionIcon variant="subtle" size="sm" onClick={onBack}>
					<IconArrowLeft size={14} />
				</ActionIcon>
				<Text size="xs" fw={600} truncate style={{ flex: 1 }}>
					{schema}.{table}
				</Text>
			</Group>

			<ScrollArea style={{ flex: 1 }} p="xs">
				{isLoading && (
					<Box ta="center" py="xl">
						<Loader size="sm" />
					</Box>
				)}

				{data && (
					<Stack gap="md">
						{/* Columns */}
						<Box>
							<Text size="xs" fw={600} c="dimmed" mb={4} tt="uppercase" lts={0.5}>
								Columns ({data.columns.length})
							</Text>
							<Table>
								<Table.Thead>
									<Table.Tr>
										<Table.Th>Name</Table.Th>
										<Table.Th>Type</Table.Th>
										<Table.Th>Null</Table.Th>
									</Table.Tr>
								</Table.Thead>
								<Table.Tbody>
									{data.columns.map((col) => (
										<Table.Tr key={col.name}>
											<Table.Td>
												<Text size="xs" ff="monospace">
													{col.name}
												</Text>
											</Table.Td>
											<Table.Td>
												<Text size="xs" c="dimmed">
													{col.dataType}
												</Text>
											</Table.Td>
											<Table.Td>
												{!col.isNullable && (
													<Text size="xs" c="red" fw={500}>NN</Text>
												)}
											</Table.Td>
										</Table.Tr>
									))}
								</Table.Tbody>
							</Table>
						</Box>

						{/* Indexes */}
						<Box>
							<Text size="xs" fw={600} c="dimmed" mb={4} tt="uppercase" lts={0.5}>
								Indexes ({data.indexes.length})
							</Text>
							{data.indexes.length === 0 ? (
								<Text size="xs" c="dimmed" ta="center" py="sm">
									No indexes
								</Text>
							) : (
								<Stack gap={4}>
									{data.indexes.map((idx) => (
										<Box
											key={idx.name}
											p="xs"
											style={{
												borderRadius: "var(--mantine-radius-sm)",
												border: "1px solid var(--octo-border-subtle)",
											}}
										>
											<Group justify="space-between" wrap="nowrap">
												<Group gap={6} wrap="nowrap" style={{ minWidth: 0 }}>
													{idx.isPrimary ? (
														<IconKey size={12} style={{ flexShrink: 0, opacity: 0.6 }} />
													) : idx.isUnique ? (
														<IconFingerprint size={12} style={{ flexShrink: 0, opacity: 0.6 }} />
													) : null}
													<Text size="xs" ff="monospace" truncate>
														{idx.name}
													</Text>
												</Group>
												{!idx.isPrimary && (
													<Tooltip label="Drop index">
														<ActionIcon
															variant="subtle"
															size="xs"
															color="fire"
															onClick={() => handleDropIndex(idx.name)}
															loading={dropIndexMutation.isPending}
														>
															<IconTrash size={12} />
														</ActionIcon>
													</Tooltip>
												)}
											</Group>
											<Group gap={6} mt={4}>
												<Badge size="xs" variant="light">
													{idx.indexType}
												</Badge>
												{idx.isPrimary && (
													<Badge size="xs" variant="light" color="warm">
														PK
													</Badge>
												)}
												{idx.isUnique && !idx.isPrimary && (
													<Badge size="xs" variant="light" color="primary">
														unique
													</Badge>
												)}
												{idx.sizeBytes != null && (
													<Text size="xs" c="dimmed">
														{formatBytes(idx.sizeBytes)}
													</Text>
												)}
											</Group>
											<Text size="xs" c="dimmed" mt={2} ff="monospace">
												({idx.columns.join(", ")})
											</Text>
										</Box>
									))}
								</Stack>
							)}
						</Box>
					</Stack>
				)}
			</ScrollArea>
		</Stack>
	);
}
