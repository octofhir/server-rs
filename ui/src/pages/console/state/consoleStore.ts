import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
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

export interface ConsoleStoreState {
	method: HttpMethod;
	mode: ConsoleMode;
	pathTokens: string[];
	resourceType?: string;
	resourceId?: string;
	interaction?: string | null;
	operation?: string;
	searchParams: ConsoleSearchParamToken[];
	body: string;
	headers: Record<string, string>;
	queryParams: Record<string, string>;
	lastResponse: ConsoleResponseSnapshot | null;
	requests: ConsoleRequestEntry[];
	// RC-04: Raw mode and extended headers
	rawPath: string;
	defaultHeaders: Record<string, string>;
	customHeaders: Record<string, string>;
	// RC-06: Command palette
	commandPaletteOpen: boolean;
	setMethod: (method: HttpMethod) => void;
	setMode: (mode: ConsoleMode) => void;
	setPathTokens: (tokens: string[]) => void;
	setResourceType: (resourceType?: string) => void;
	setResourceId: (resourceId?: string) => void;
	setInteraction: (interaction?: string | null) => void;
	setOperation: (operation?: string) => void;
	setSearchParams: (params: ConsoleSearchParamToken[]) => void;
	setBody: (body: string) => void;
	setHeaders: (headers: Record<string, string>) => void;
	setQueryParams: (params: Record<string, string>) => void;
	setLastResponse: (response: ConsoleResponseSnapshot | null) => void;
	sendDraftRequest: () => void;
	resetDraft: () => void;
	// RC-04: New actions
	setRawPath: (path: string) => void;
	setDefaultHeaders: (headers: Record<string, string>) => void;
	setCustomHeaders: (headers: Record<string, string>) => void;
	addCustomHeader: (key: string, value: string) => void;
	removeCustomHeader: (key: string) => void;
	updateCustomHeader: (oldKey: string, newKey: string, value: string) => void;
	mergeAllHeaders: () => Record<string, string>;
	// RC-06: Command palette actions
	setCommandPaletteOpen: (open: boolean) => void;
}

const DEFAULT_FHIR_HEADERS = {
	Accept: "application/fhir+json",
	"Content-Type": "application/fhir+json",
};

const initialState = {
	method: "GET" as HttpMethod,
	mode: "smart" as ConsoleMode,
	pathTokens: buildPathTokens({
		resourceType: undefined,
		resourceId: undefined,
		interaction: null,
		operation: undefined,
		searchParams: [],
	}),
	resourceType: undefined as string | undefined,
	resourceId: undefined as string | undefined,
	interaction: null as string | null,
	operation: undefined as string | undefined,
	searchParams: [] as ConsoleSearchParamToken[],
	body: "",
	headers: {} as Record<string, string>,
	queryParams: {} as Record<string, string>,
	lastResponse: null as ConsoleResponseSnapshot | null,
	requests: [] as ConsoleRequestEntry[],
	// RC-04: New state fields
	rawPath: "",
	defaultHeaders: DEFAULT_FHIR_HEADERS,
	customHeaders: {} as Record<string, string>,
	// RC-06: Command palette
	commandPaletteOpen: false,
};

const storage = typeof window !== "undefined"
	? createJSONStorage(() => localStorage)
	: undefined;

export const useConsoleStore = create<ConsoleStoreState>()(
	persist(
		(set, get) => ({
			...initialState,
			setMethod: (method) => set({ method }),
			setMode: (mode) => set({ mode }),
			setPathTokens: (tokens) => set({ pathTokens: tokens }),
			setResourceType: (resourceType) =>
				set((state) => ({
					resourceType,
					pathTokens: buildPathTokens({
						resourceType,
						resourceId: state.resourceId,
						interaction: state.interaction,
						operation: state.operation,
						searchParams: state.searchParams,
					}),
				})),
			setResourceId: (resourceId) =>
				set((state) => ({
					resourceId,
					pathTokens: buildPathTokens({
						resourceType: state.resourceType,
						resourceId,
						interaction: state.interaction,
						operation: state.operation,
						searchParams: state.searchParams,
					}),
				})),
			setInteraction: (interaction) =>
				set((state) => ({
					interaction: interaction ?? null,
					pathTokens: buildPathTokens({
						resourceType: state.resourceType,
						resourceId: state.resourceId,
						interaction: interaction ?? null,
						operation: state.operation,
						searchParams: state.searchParams,
					}),
				})),
			setOperation: (operation) =>
				set((state) => ({
					operation,
					pathTokens: buildPathTokens({
						resourceType: state.resourceType,
						resourceId: state.resourceId,
						interaction: state.interaction,
						operation,
						searchParams: state.searchParams,
					}),
				})),
			setSearchParams: (params) =>
				set((state) => ({
					searchParams: params,
					pathTokens: buildPathTokens({
						resourceType: state.resourceType,
						resourceId: state.resourceId,
						interaction: state.interaction,
						operation: state.operation,
						searchParams: params,
					}),
				})),
			setBody: (body) => set({ body }),
			setHeaders: (headers) => set({ headers }),
			setQueryParams: (params) => set({ queryParams: params }),
			setLastResponse: (response) => set({ lastResponse: response }),
			sendDraftRequest: () => {
				if (typeof window !== "undefined") {
					console.debug("REST console send stub", new Date().toISOString());
				}
			},
			resetDraft: () =>
				set({
					method: "GET",
					pathTokens: buildPathTokens({
						resourceType: undefined,
						resourceId: undefined,
						interaction: null,
						operation: undefined,
						searchParams: [],
					}),
					resourceType: undefined,
					resourceId: undefined,
					interaction: null,
					operation: undefined,
					searchParams: [],
					body: "",
					headers: {},
					queryParams: {},
					rawPath: "",
					customHeaders: {},
				}),
			// RC-04: New action implementations
			setRawPath: (path) => set({ rawPath: path }),
			setDefaultHeaders: (headers) => set({ defaultHeaders: headers }),
			setCustomHeaders: (headers) => set({ customHeaders: headers }),
			addCustomHeader: (key, value) =>
				set((state) => ({
					customHeaders: { ...state.customHeaders, [key]: value },
				})),
			removeCustomHeader: (key) =>
				set((state) => {
					const { [key]: _, ...rest } = state.customHeaders;
					return { customHeaders: rest };
				}),
			updateCustomHeader: (oldKey, newKey, value) =>
				set((state) => {
					const { [oldKey]: _, ...rest } = state.customHeaders;
					return { customHeaders: { ...rest, [newKey]: value } };
				}),
			mergeAllHeaders: (): Record<string, string> => {
				const state = get();
				return { ...state.defaultHeaders, ...state.customHeaders };
			},
			// RC-06: Command palette action
			setCommandPaletteOpen: (open) => set({ commandPaletteOpen: open }),
		}),
		{
			name: "octofhir.console",
			version: 1,
			storage,
			partialize: (state) => ({
				method: state.method,
				mode: state.mode,
				pathTokens: state.pathTokens,
				resourceType: state.resourceType,
				resourceId: state.resourceId,
				interaction: state.interaction,
				operation: state.operation,
				searchParams: state.searchParams,
				body: state.body,
				headers: state.headers,
				queryParams: state.queryParams,
				// RC-04: Persist new fields
				rawPath: state.rawPath,
				customHeaders: state.customHeaders,
			}),
		},
	),
);
