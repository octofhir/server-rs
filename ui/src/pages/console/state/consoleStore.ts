import { createEvent, createStore, sample } from "effector";
import { persist } from "effector-storage/local";
import type { HttpMethod } from "@/shared/api";
import { buildPathTokens } from "@/shared/utils/pathTokens";
import type { ConsoleSearchParamToken } from "./searchParams";

export type { ConsoleSearchParamToken } from "./searchParams";

export type ConsoleMode = "smart" | "raw";

export interface ConsoleResponseSnapshot {
	status?: number;
	statusText?: string;
	durationMs?: number;
	body?: unknown;
	requestedAt: string;
}

export interface ConsoleRequestEntry {
	id: string;
	method: HttpMethod;
	path: string;
	requestedAt: string;
}

const DEFAULT_FHIR_HEADERS = {
	Accept: "application/fhir+json",
	"Content-Type": "application/fhir+json",
};

const initialTokens = buildPathTokens({
	resourceType: undefined,
	resourceId: undefined,
	interaction: null,
	operation: undefined,
	searchParams: [],
});

export const setMethod = createEvent<HttpMethod>();
export const setMode = createEvent<ConsoleMode>();
export const setPathTokens = createEvent<string[]>();
export const setResourceType = createEvent<string | undefined>();
export const setResourceId = createEvent<string | undefined>();
export const setInteraction = createEvent<string | null | undefined>();
export const setOperation = createEvent<string | undefined>();
export const setSearchParams = createEvent<ConsoleSearchParamToken[]>();
export const setBody = createEvent<string>();
export const setHeaders = createEvent<Record<string, string>>();
export const setQueryParams = createEvent<Record<string, string>>();
export const setLastResponse = createEvent<ConsoleResponseSnapshot | null>();
export const sendDraftRequest = createEvent();
export const resetDraft = createEvent();
export const setRawPath = createEvent<string>();
export const setDefaultHeaders = createEvent<Record<string, string>>();
export const setCustomHeaders = createEvent<Record<string, string>>();
export const addCustomHeader = createEvent<{ key: string; value: string }>();
export const removeCustomHeader = createEvent<string>();
export const updateCustomHeader = createEvent<{
	oldKey: string;
	newKey: string;
	value: string;
}>();
export const setCommandPaletteOpen = createEvent<boolean>();

export const $method = createStore<HttpMethod>("GET")
	.on(setMethod, (_, method) => method)
	.reset(resetDraft);

export const $mode = createStore<ConsoleMode>("smart").on(
	setMode,
	(_, mode) => mode,
);

export const $pathTokens = createStore<string[]>(initialTokens)
	.on(setPathTokens, (_, tokens) => tokens)
	.reset(resetDraft);

export const $resourceType = createStore<string | undefined>(undefined, {
	skipVoid: false,
})
	.on(setResourceType, (_, value) => value)
	.reset(resetDraft);

export const $resourceId = createStore<string | undefined>(undefined, {
	skipVoid: false,
})
	.on(setResourceId, (_, value) => value)
	.reset(resetDraft);

export const $interaction = createStore<string | null>(null)
	.on(setInteraction, (_, value) => value ?? null)
	.reset(resetDraft);

export const $operation = createStore<string | undefined>(undefined, {
	skipVoid: false,
})
	.on(setOperation, (_, value) => value)
	.reset(resetDraft);

export const $searchParams = createStore<ConsoleSearchParamToken[]>([])
	.on(setSearchParams, (_, params) => params)
	.reset(resetDraft);

export const $body = createStore<string>("")
	.on(setBody, (_, body) => body)
	.reset(resetDraft);

export const $headers = createStore<Record<string, string>>({})
	.on(setHeaders, (_, headers) => headers)
	.reset(resetDraft);

export const $queryParams = createStore<Record<string, string>>({})
	.on(setQueryParams, (_, params) => params)
	.reset(resetDraft);

export const $lastResponse = createStore<ConsoleResponseSnapshot | null>(
	null,
).on(setLastResponse, (_, response) => response);

export const $requests = createStore<ConsoleRequestEntry[]>([]);

export const $rawPath = createStore<string>("")
	.on(setRawPath, (_, path) => path)
	.reset(resetDraft);

export const $defaultHeaders = createStore<Record<string, string>>(
	DEFAULT_FHIR_HEADERS,
).on(setDefaultHeaders, (_, headers) => headers);

export const $customHeaders = createStore<Record<string, string>>({})
	.on(setCustomHeaders, (_, headers) => headers)
	.on(addCustomHeader, (state, { key, value }) => ({ ...state, [key]: value }))
	.on(removeCustomHeader, (state, key) => {
		const { [key]: _, ...rest } = state;
		return rest;
	})
	.on(updateCustomHeader, (state, { oldKey, newKey, value }) => {
		const { [oldKey]: _, ...rest } = state;
		return { ...rest, [newKey]: value };
	})
	.reset(resetDraft);

export const $commandPaletteOpen = createStore<boolean>(false).on(
	setCommandPaletteOpen,
	(_, open) => open,
);

sample({
	source: {
		resourceType: $resourceType,
		resourceId: $resourceId,
		interaction: $interaction,
		operation: $operation,
		searchParams: $searchParams,
	},
	clock: [
		setResourceType,
		setResourceId,
		setInteraction,
		setOperation,
		setSearchParams,
	],
	fn: ({ resourceType, resourceId, interaction, operation, searchParams }) =>
		buildPathTokens({
			resourceType,
			resourceId,
			interaction,
			operation,
			searchParams,
		}),
	target: setPathTokens,
});

sendDraftRequest.watch(() => {
	if (typeof window !== "undefined") {
		console.debug("REST console send stub", new Date().toISOString());
	}
});

persist({ store: $method, key: "octofhir.console.method" });
persist({ store: $mode, key: "octofhir.console.mode" });
persist({ store: $pathTokens, key: "octofhir.console.pathTokens" });
persist({ store: $resourceType, key: "octofhir.console.resourceType" });
persist({ store: $resourceId, key: "octofhir.console.resourceId" });
persist({ store: $interaction, key: "octofhir.console.interaction" });
persist({ store: $operation, key: "octofhir.console.operation" });
persist({ store: $searchParams, key: "octofhir.console.searchParams" });
persist({ store: $body, key: "octofhir.console.body" });
persist({ store: $headers, key: "octofhir.console.headers" });
persist({ store: $queryParams, key: "octofhir.console.queryParams" });
persist({ store: $rawPath, key: "octofhir.console.rawPath" });
persist({ store: $customHeaders, key: "octofhir.console.customHeaders" });
