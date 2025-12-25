import { createEvent, createStore, sample } from "effector";
import { persist } from "effector-storage/local";

export type AppTabKind = "resource" | "page";

export interface AppTab {
	id: string;
	title: string;
	path: string;
	kind: AppTabKind;
	closeable: boolean;
	pinned?: boolean;
	groupKey?: string;
	createdAt?: number;
	customTitle?: boolean;
}

export function buildResourceTabId(resourceType: string, resourceId: string) {
	return `resource:${resourceType}/${resourceId}`;
}

export function buildResourceTabPath(resourceType: string, resourceId: string) {
	return `/resources/${resourceType}/${resourceId}`;
}

export function buildResourceTabTitle(resourceType: string, resourceId: string) {
	return `${resourceType}/${resourceId}`;
}

export function buildPageTabId(path: string) {
	return `page:${path}`;
}

function createTabId() {
	if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
		return crypto.randomUUID();
	}
	return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function normalizePath(pathname: string) {
	if (!pathname) return "/";
	if (pathname === "/") return "/";
	return pathname.replace(/\/+$/, "");
}

function decodeSegment(value: string) {
	try {
		return decodeURIComponent(value);
	} catch {
		return value;
	}
}

type TabConfig = {
	title: string;
	closeable?: boolean;
};

const STATIC_TITLES: Record<string, TabConfig> = {
	"/": { title: "Dashboard", closeable: true },
	"/resources": { title: "Resource Browser", closeable: true },
	"/console": { title: "REST Console" },
	"/packages": { title: "Packages" },
	"/operations": { title: "Operations" },
	"/apps": { title: "Apps" },
	"/auth/clients": { title: "Clients" },
	"/auth/users": { title: "Users" },
	"/auth/policies": { title: "Access Policies" },
	"/db-console": { title: "DB Console" },
	"/graphql": { title: "GraphQL" },
	"/settings": { title: "Settings" },
	"/logs": { title: "System Logs" },
	"/metadata": { title: "Capability Statement" },
};

type ResolveOptions = {
	titleOverride?: string;
};

export function resolveTabFromPath(pathname: string, options?: ResolveOptions): AppTab | null {
	const normalized = normalizePath(pathname);
	const staticConfig = STATIC_TITLES[normalized];
	if (staticConfig) {
		return {
			id: createTabId(),
			title: options?.titleOverride ?? staticConfig.title,
			path: normalized,
			kind: "page",
			closeable: staticConfig.closeable ?? true,
			groupKey: normalized,
			createdAt: Date.now(),
			customTitle: false,
		};
	}

	const segments = normalized.split("/").filter(Boolean);
	if (segments.length === 0) {
		return null;
	}

	if (segments[0] === "resources") {
		if (segments.length === 3) {
			const resourceType = decodeSegment(segments[1]);
			const resourceId = decodeSegment(segments[2]);
			return {
				id: createTabId(),
				title: buildResourceTabTitle(resourceType, resourceId),
				path: normalized,
				kind: "resource",
				closeable: true,
				groupKey: "/resources",
				createdAt: Date.now(),
				customTitle: false,
			};
		}
		if (segments.length === 2) {
			const resourceType = decodeSegment(segments[1]);
			return {
				id: createTabId(),
				title: options?.titleOverride ?? `Resources: ${resourceType}`,
				path: normalized,
				kind: "page",
				closeable: true,
				groupKey: "/resources",
				createdAt: Date.now(),
				customTitle: false,
			};
		}
	}

	if (segments[0] === "packages" && segments.length === 3) {
		const name = decodeSegment(segments[1]);
		const version = decodeSegment(segments[2]);
		return {
			id: createTabId(),
			title: `Package: ${name}@${version}`,
			path: normalized,
			kind: "page",
			closeable: true,
			groupKey: "/packages",
			createdAt: Date.now(),
			customTitle: false,
		};
	}

	if (segments[0] === "operations" && segments.length === 2) {
		const operationId = decodeSegment(segments[1]);
		return {
			id: createTabId(),
			title: `Operation: ${operationId}`,
			path: normalized,
			kind: "page",
			closeable: true,
			groupKey: "/operations",
			createdAt: Date.now(),
			customTitle: false,
		};
	}

	return null;
}

export const openTab = createEvent<AppTab>();
export const openResourceTab = createEvent<{ resourceType: string; resourceId: string }>();
export const openTabForPath = createEvent<{ pathname: string; titleOverride?: string }>();
export const openNewTabForPath = createEvent<{ pathname: string; titleOverride?: string }>();
export const closeTab = createEvent<string>();
export const setActiveTab = createEvent<string | null>();
export const togglePinTab = createEvent<string>();
export const reorderTabs = createEvent<{ orderedIds: string[] }>();
export const renameTab = createEvent<{ id: string; title: string }>();

export const $tabs = createStore<AppTab[]>([])
	.on(openTab, (state, tab) => {
		const existingIndex = state.findIndex((entry) => entry.id === tab.id);
		if (existingIndex >= 0) {
			const next = [...state];
			const existing = next[existingIndex];
			next[existingIndex] = {
				...existing,
				...tab,
				title: existing.customTitle ? existing.title : tab.title,
			};
			return next;
		}
		return [...state, tab];
	})
	.on(closeTab, (state, id) => state.filter((entry) => entry.id !== id))
	.on(renameTab, (state, { id, title }) =>
		state.map((entry) =>
			entry.id === id ? { ...entry, title, customTitle: true } : entry,
		),
	)
	.on(togglePinTab, (state, id) =>
		state.map((entry) =>
			entry.id === id ? { ...entry, pinned: !entry.pinned } : entry,
		),
	)
	.on(reorderTabs, (state, { orderedIds }) => {
		if (orderedIds.length === 0) return state;
		const lookup = new Map(state.map((tab) => [tab.id, tab]));
		const next: AppTab[] = [];
		for (const id of orderedIds) {
			const tab = lookup.get(id);
			if (tab) {
				next.push(tab);
				lookup.delete(id);
			}
		}
		if (lookup.size > 0) {
			next.push(...lookup.values());
		}
		return next;
	});

export const $activeTabId = createStore<string | null>(null)
	.on(setActiveTab, (_, id) => id)
	.on(openTab, (_, tab) => tab.id)
	.on(closeTab, (current, id) => (current === id ? null : current));

sample({
	clock: openResourceTab,
	fn: ({ resourceType, resourceId }) => ({
		id: createTabId(),
		title: buildResourceTabTitle(resourceType, resourceId),
		path: buildResourceTabPath(resourceType, resourceId),
		kind: "resource" as const,
		closeable: true,
		groupKey: "/resources",
		createdAt: Date.now(),
		customTitle: false,
	}),
	target: openTab,
});

function findReusableTab(
	tabs: AppTab[],
	normalized: string,
	groupKey?: string,
): AppTab | undefined {
	const byPath = tabs.find((tab) => tab.path === normalized && !tab.customTitle);
	if (byPath) return byPath;
	if (groupKey) {
		return tabs.find((tab) => tab.groupKey === groupKey && !tab.customTitle);
	}
	return undefined;
}

sample({
	source: $tabs,
	clock: openTabForPath,
	fn: (tabs, payload) => {
		const normalized = normalizePath(payload.pathname);
		const resolved = resolveTabFromPath(normalized, {
			titleOverride: payload.titleOverride,
		});
		if (!resolved) return null;
		const existing = findReusableTab(tabs, normalized, resolved.groupKey);
		return existing?.id ?? null;
	},
	filter: (id): id is string => id !== null,
	target: setActiveTab,
});

sample({
	source: $tabs,
	clock: openTabForPath,
	fn: (tabs, payload) => {
		const normalized = normalizePath(payload.pathname);
		const resolved = resolveTabFromPath(normalized, {
			titleOverride: payload.titleOverride,
		});
		if (!resolved) return null;
		const existing = findReusableTab(tabs, normalized, resolved.groupKey);
		if (existing) {
			return {
				...existing,
				title: resolved.title,
				path: resolved.path,
			};
		}
		return resolved;
	},
	filter: (tab): tab is AppTab => tab !== null,
	target: openTab,
});

sample({
	clock: openNewTabForPath,
	fn: (payload) => resolveTabFromPath(payload.pathname, { titleOverride: payload.titleOverride }),
	filter: (tab): tab is AppTab => tab !== null,
	target: openTab,
});

persist({ store: $tabs, key: "octofhir.tabs" });
persist({ store: $activeTabId, key: "octofhir.tabs.active" });
