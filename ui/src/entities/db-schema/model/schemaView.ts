import type { DbColumnInfo, DbIndexInfo, DbTableInfo } from "@/shared/api/types";

export interface DbSchemaTableView {
	id: string;
	schema: string;
	name: string;
	displayName: string;
	kind: string;
	isView: boolean;
	rowEstimateLabel?: string;
}

export interface DbColumnView {
	id: string;
	name: string;
	dataType: string;
	nullability: "nullable" | "required";
}

export interface DbIndexView {
	id: string;
	name: string;
	indexType: string;
	columnList: string;
	isPrimary: boolean;
	isUnique: boolean;
	sizeLabel?: string;
}

export function formatDbSize(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function getDbSchemaTableView(table: DbTableInfo): DbSchemaTableView {
	const isView = table.tableType === "VIEW";

	return {
		id: `${table.schema}.${table.name}`,
		schema: table.schema,
		name: table.name,
		displayName: table.schema !== "public" ? `${table.schema}.${table.name}` : table.name,
		kind: table.tableType.toLowerCase(),
		isView,
		rowEstimateLabel:
			table.rowEstimate != null && table.rowEstimate > 0
				? `~${table.rowEstimate.toLocaleString()} rows`
				: undefined,
	};
}

export function getDbSchemaTableViews(tables: DbTableInfo[]): DbSchemaTableView[] {
	return tables.map(getDbSchemaTableView);
}

export function filterDbSchemaTables(
	tables: DbTableInfo[],
	search: string,
): DbTableInfo[] {
	const query = search.trim().toLowerCase();
	if (!query) return tables;

	return tables.filter(
		(table) =>
			table.name.toLowerCase().includes(query) ||
			table.schema.toLowerCase().includes(query),
	);
}

export function getDbColumnViews(columns: DbColumnInfo[]): DbColumnView[] {
	return columns.map((column) => ({
		id: column.name,
		name: column.name,
		dataType: column.dataType,
		nullability: column.isNullable ? "nullable" : "required",
	}));
}

export function getDbIndexViews(indexes: DbIndexInfo[]): DbIndexView[] {
	return indexes.map((index) => ({
		id: index.name,
		name: index.name,
		indexType: index.indexType,
		columnList: `(${index.columns.join(", ")})`,
		isPrimary: index.isPrimary,
		isUnique: index.isUnique,
		sizeLabel: index.sizeBytes != null ? formatDbSize(index.sizeBytes) : undefined,
	}));
}
