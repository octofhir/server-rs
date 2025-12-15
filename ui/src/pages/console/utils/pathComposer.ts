import type {
	HttpMethod,
	RestConsoleOperation,
	RestConsoleResource,
	RestConsoleResourceOperationRef,
	RestConsoleSearchParam,
} from "@/shared/api";

export type SuggestionKind = "resource" | "interaction" | "operation" | "search-param" | "slash";

export interface PathSuggestion {
	id: string;
	kind: SuggestionKind;
	value: string;
	label: string;
	description?: string;
	badge?: string;
	action?: "set-resource" | "set-operation" | "set-interaction" | "set-search";
	meta?: Record<string, string>;
}

export interface PathComposerMetadata {
	fhirVersion: string;
	resourceTypes: string[];
	resourceMap: Record<string, RestConsoleResource>;
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	allOperations: RestConsoleOperation[];
}

export interface SuggestionContextState {
	resourceType?: string;
	method: HttpMethod;
}

interface SuggestionArgs {
	context: SuggestionKind;
	query: string;
	state: SuggestionContextState;
	metadata: PathComposerMetadata;
}

const MAX_SUGGESTIONS = 12;

export function getSuggestions(args: SuggestionArgs): PathSuggestion[] {
	const query = args.query.trim().toLowerCase();
	switch (args.context) {
		case "resource":
			return buildResourceSuggestions(args.metadata, query);
		case "interaction":
			return buildInteractionSuggestions(args.state.resourceType, args.metadata, query);
		case "operation":
			return buildOperationSuggestions(args.state, args.metadata, query);
		case "search-param":
			return buildSearchParamSuggestions(args.state.resourceType, args.metadata, query);
		case "slash":
			return buildSlashSuggestions(args.state, args.metadata, query);
		default:
			return [];
	}
}

function buildResourceSuggestions(
	metadata: PathComposerMetadata,
	query: string,
): PathSuggestion[] {
	return metadata.resourceTypes
		.filter((resource) => (query ? resource.toLowerCase().includes(query) : true))
		.slice(0, MAX_SUGGESTIONS)
		.map((resource) => {
			const meta = metadata.resourceMap[resource];
			return {
				id: resource,
				kind: "resource" as const,
				label: resource,
				value: resource,
				description: meta ? describeResource(meta) : undefined,
				action: "set-resource" as const,
			};
		});
}

function describeResource(resource: RestConsoleResource): string {
	const paramCount = resource.search_params.length;
	const interactionCount = resource.interactions.length;
	return `${paramCount} search params · ${interactionCount} interactions`;
}

const INTERACTION_LABELS: Record<string, string> = {
	_history: "History",
	_search: "Search (POST style)",
};

function buildInteractionSuggestions(
	resourceType: string | undefined,
	metadata: PathComposerMetadata,
	query: string,
): PathSuggestion[] {
	if (!resourceType) {
		return [];
	}
	const resource = metadata.resourceMap[resourceType];
	if (!resource) {
		return [];
	}

	const interactions = new Set<string>();
	for (const interaction of resource.interactions) {
		if (interaction.startsWith("history")) {
			interactions.add("_history");
		}
		if (interaction.startsWith("search")) {
			interactions.add("_search");
		}
	}

	return Array.from(interactions)
		.filter((value) => (query ? value.toLowerCase().includes(query) : true))
		.map((value) => ({
			id: value,
			kind: "interaction" as const,
			value,
			label: INTERACTION_LABELS[value] ?? value,
			description: value === "_history" ? "Append _history to read previous versions" : "Use POST /_search",
			action: "set-interaction" as const,
		}));
}

export interface OperationDescriptor {
	code: string;
	method: string;
	scope: "system" | "type" | "instance";
	description?: string;
	resourceTypes: string[];
}

export function collectOperationDescriptors(metadata: PathComposerMetadata): OperationDescriptor[] {
	const descriptors: OperationDescriptor[] = [];
	for (const operation of metadata.allOperations) {
		const scope: OperationDescriptor["scope"] = operation.system
			? "system"
			: operation.instance
				? "instance"
				: "type";
		descriptors.push({
			code: operation.code,
			method: operation.method.toUpperCase(),
			scope,
			description: operation.path_templates[0] ?? undefined,
			resourceTypes: operation.resource_types,
		});
	}
	return descriptors;
}

function buildOperationSuggestions(
	state: SuggestionContextState,
	metadata: PathComposerMetadata,
	query: string,
): PathSuggestion[] {
	const descriptors = collectOperationDescriptors(metadata);
	const allowed = descriptors.filter((operation) => {
		if (operation.scope === "system") {
			return true;
		}
		if (!state.resourceType) {
			return false;
		}
		return operation.resourceTypes.includes(state.resourceType);
	});

	return allowed
		.filter((operation) => (query ? operation.code.toLowerCase().includes(query) : true))
		.slice(0, MAX_SUGGESTIONS)
		.map((operation) => ({
			id: operation.code,
			kind: "operation" as const,
			value: `$${operation.code}`,
			label: `$${operation.code}`,
			description: operation.description ?? `${operation.scope} level · ${operation.method}`,
			badge: operation.method,
			action: "set-operation" as const,
			meta: {
				method: operation.method,
				scope: operation.scope,
			},
		}));
}

export function getSearchParamMetadata(
	resourceType: string | undefined,
	code: string,
	metadata: PathComposerMetadata,
): RestConsoleSearchParam | undefined {
	if (!resourceType) {
		return undefined;
	}
	const params = metadata.searchParamsByResource[resourceType] ?? [];
	return params.find((param) => param.code === code);
}

function buildSearchParamSuggestions(
	resourceType: string | undefined,
	metadata: PathComposerMetadata,
	query: string,
): PathSuggestion[] {
	if (!resourceType) {
		return [];
	}
	const params = metadata.searchParamsByResource[resourceType] ?? [];
	return params
		.filter((param) => (query ? param.code.toLowerCase().includes(query) : true))
		.slice(0, MAX_SUGGESTIONS)
		.map((param) => ({
			id: param.code,
			kind: "search-param" as const,
			value: param.code,
			label: param.code,
			description: param.description,
			badge: param.search_type,
			action: "set-search" as const,
		}));
}

function buildSlashSuggestions(
	state: SuggestionContextState,
	metadata: PathComposerMetadata,
	query: string,
): PathSuggestion[] {
	const items: PathSuggestion[] = [];
	const normalizedQuery = query.replace("/", "");
	items.push(
		...buildResourceSuggestions(metadata, normalizedQuery).map((suggestion) => ({
			...suggestion,
			kind: "slash" as const,
			action: "set-resource" as const,
		})),
	);
	items.push(
		...buildOperationSuggestions(state, metadata, normalizedQuery).map((suggestion) => ({
			...suggestion,
			kind: "slash" as const,
			action: "set-operation" as const,
		})),
	);

	items.push(
		...buildInteractionSuggestions(state.resourceType, metadata, normalizedQuery).map(
			(suggestion) => ({
				...suggestion,
				kind: "slash" as const,
				action: "set-interaction" as const,
			}),
		),
	);

	return items.slice(0, MAX_SUGGESTIONS);
}

export function getModifiersForParam(
	param: RestConsoleSearchParam | undefined,
	fhirVersion: string,
): string[] {
	if (!param) {
		return [];
	}
	if (param.fhir_versions.length > 0 && !param.fhir_versions.includes(fhirVersion)) {
		return [];
	}
	return param.modifiers ?? [];
}

export function toOperationRef(
	resource: RestConsoleResource,
	scope: "type" | "instance",
): RestConsoleResourceOperationRef[] {
	return scope === "type" ? resource.operations.type : resource.operations.instance_level;
}
