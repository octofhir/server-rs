import { modals, notifications } from "@octofhir/ui-kit";
import { useCallback, useMemo } from "react";
import {
	Text,
	ActionIcon,
	ScrollArea,
	Tooltip,
	Loader,
	DataPreview,
	RecordList,
} from "@octofhir/ui-kit";
import { ArrowLeft, Trash2 as TrashBin, Key, Fingerprint } from "lucide-react";
import { getDbColumnViews, getDbIndexViews } from "@/entities/db-schema";
import { useTableDetail, useDropIndex } from "@/shared/api/hooks";
import classes from "../DbConsolePage.module.css";

interface TableDetailViewProps {
	schema: string;
	table: string;
	onBack: () => void;
}

export function TableDetailView({ schema, table, onBack }: TableDetailViewProps) {
	const { data, isLoading } = useTableDetail(schema, table);
	const dropIndexMutation = useDropIndex();
	const columnViews = useMemo(
		() => getDbColumnViews(data?.columns ?? []),
		[data?.columns],
	);
	const indexViews = useMemo(
		() => getDbIndexViews(data?.indexes ?? []),
		[data?.indexes],
	);

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
		<div className={classes.tableDetailRoot}>
			<div className={classes.tableDetailHeader}>
				<ActionIcon variant="subtle" size="sm" onClick={onBack}>
					<ArrowLeft size={14} />
				</ActionIcon>
				<Text size="xs" fw={600} truncate className={classes.tableDetailTitle}>
					{schema}.{table}
				</Text>
			</div>

			<ScrollArea className={classes.tableDetailScroll}>
				{isLoading && (
					<div className={classes.centeredLoader}>
						<Loader size="sm" />
					</div>
				)}

				{data && (
					<div className={classes.tableDetailContent}>
						{/* Columns */}
						<section>
							<Text size="xs" fw={600} c="dimmed" mb={4} style={{ textTransform: "uppercase", letterSpacing: 0.5 }}>
								Columns ({data.columns.length})
							</Text>
							<DataPreview
								columns={[
									{ id: "name", label: "Name", width: "45%" },
									{ id: "type", label: "Type", width: "40%" },
									{ id: "null", label: "Null", width: 56 },
								]}
								rows={columnViews.map((column) => ({
									name: (
										<Text size="xs" ff="monospace">
											{column.name}
										</Text>
									),
									type: (
										<Text size="xs" c="dimmed">
											{column.dataType}
										</Text>
									),
									null: column.nullability === "required" ? (
										<Text size="xs" c="red" fw={500}>
											NN
										</Text>
									) : null,
								}))}
								getRowKey={(_row, rowIndex) => columnViews[rowIndex]?.id ?? `${rowIndex}`}
							/>
						</section>

						{/* Indexes */}
						<section>
							<Text size="xs" fw={600} c="dimmed" mb={4} style={{ textTransform: "uppercase", letterSpacing: 0.5 }}>
								Indexes ({data.indexes.length})
							</Text>
							<RecordList
								density="compact"
								emptyText="No indexes"
								items={indexViews.map((index) => ({
									id: index.id,
									title: index.name,
									subtitle: index.indexType,
									description: index.columnList,
									leading: index.isPrimary ? (
										<Key size={14} />
									) : index.isUnique ? (
										<Fingerprint size={14} />
									) : null,
									meta: [
										{ id: "type", label: index.indexType, tone: "neutral" as const },
										...(index.isPrimary
											? [{ id: "pk", label: "PK", tone: "warning" as const }]
											: []),
										...(index.isUnique && !index.isPrimary
											? [{ id: "unique", label: "unique", tone: "info" as const }]
											: []),
										...(index.sizeLabel
											? [
													{
														id: "size",
														label: index.sizeLabel,
														tone: "neutral" as const,
													},
												]
											: []),
									],
									aside: !index.isPrimary ? (
										<Tooltip label="Drop index">
											<ActionIcon
												variant="subtle"
												size="xs"
												color="fire"
												onClick={() => handleDropIndex(index.name)}
												loading={dropIndexMutation.isPending}
											>
												<TrashBin size={12} />
											</ActionIcon>
										</Tooltip>
									) : null,
								}))}
							/>
						</section>
					</div>
				)}
			</ScrollArea>
		</div>
	);
}
