import type { OperationDefinition } from "@/shared/api/types";

export type OperationAccessFilter = "all" | "public" | "protected";

export interface OperationCategoryView {
	id: string;
	label: string;
	color: string;
}

export interface OperationMethodView {
	method: string;
	color: string;
}

export interface OperationAccessView {
	label: string;
	color: string;
	tone: "public" | "protected";
	description: string;
}

export interface OperationAppOption {
	value: string;
	label: string;
}

export type GroupedOperations = Record<string, OperationDefinition[]>;

const categoryColorById: Record<string, string> = {
	fhir: "primary",
	graphql: "deep",
	system: "warm",
	auth: "fire",
	ui: "warm",
	api: "deep",
};

const categoryLabelById: Record<string, string> = {
	fhir: "FHIR REST API",
	graphql: "GraphQL",
	system: "System",
	auth: "Authentication",
	ui: "UI API",
	api: "Custom API",
};

const methodColorById: Record<string, string> = {
	GET: "primary",
	POST: "warm",
	PUT: "deep",
	DELETE: "fire",
	PATCH: "warm",
};

export const operationAccessFilterOptions: Array<{
	label: string;
	value: OperationAccessFilter;
}> = [
	{ label: "All", value: "all" },
	{ label: "Public", value: "public" },
	{ label: "Protected", value: "protected" },
];

export function getOperationCategoryView(category: string): OperationCategoryView {
	return {
		id: category,
		label: categoryLabelById[category] ?? category,
		color: categoryColorById[category] ?? "gray",
	};
}

export function getOperationMethodView(method: string): OperationMethodView {
	return {
		method,
		color: methodColorById[method] ?? "gray",
	};
}

export function getOperationAccessView(isPublic: boolean): OperationAccessView {
	return isPublic
		? {
				label: "Public",
				color: "primary",
				tone: "public",
				description: "No authentication required",
			}
		: {
				label: "Protected",
				color: "deep",
				tone: "protected",
				description: "Authentication required",
			};
}

export function getOperationAppOptions(
	operations: OperationDefinition[] | undefined,
): OperationAppOption[] {
	if (!operations) return [];

	const apps = new Map<string, string>();
	for (const operation of operations) {
		if (operation.app) {
			apps.set(operation.app.id, operation.app.name);
		}
	}

	return Array.from(apps.entries()).map(([value, label]) => ({ value, label }));
}

export function filterOperations(
	operations: OperationDefinition[],
	search: string,
	accessFilter: OperationAccessFilter,
	appId: string | null,
): OperationDefinition[] {
	const query = search.trim().toLowerCase();

	return operations.filter((operation) => {
		const matchesSearch =
			!query ||
			operation.id.toLowerCase().includes(query) ||
			operation.name.toLowerCase().includes(query) ||
			operation.description?.toLowerCase().includes(query) ||
			operation.path_pattern.toLowerCase().includes(query) ||
			operation.app?.name.toLowerCase().includes(query);

		const matchesAccess =
			accessFilter === "all" ||
			(accessFilter === "public" && operation.public) ||
			(accessFilter === "protected" && !operation.public);

		const matchesApp = !appId || operation.app?.id === appId;

		return Boolean(matchesSearch && matchesAccess && matchesApp);
	});
}

export function groupOperationsByCategory(
	operations: OperationDefinition[],
): GroupedOperations {
	return operations.reduce((acc, operation) => {
		const category = operation.category || "other";
		acc[category] ??= [];
		acc[category].push(operation);
		return acc;
	}, {} as GroupedOperations);
}
