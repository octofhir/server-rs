import type { JSX } from "solid-js";

export interface Command {
	id: string;
	name: string;
	shortcut?: string[];
	keywords?: string;
	section?: string;
	priority?: number;
	perform?: () => void | Promise<void>;
	subtitle?: string;
	icon?: JSX.Element;
}

// Predefined command sections
export const COMMAND_SECTIONS = {
	NAVIGATION: "Navigation",
	ACTIONS: "Actions",
	SYSTEM: "System",
	REST_CONSOLE: "REST Console",
	THEME: "Theme",
} as const;

// System commands
export const systemCommands: Command[] = [
	{
		id: "copy-commit-sha",
		name: "Copy Commit SHA",
		shortcut: [],
		keywords: "copy commit sha git",
		section: COMMAND_SECTIONS.SYSTEM,
		priority: 100,
		perform: async () => {
			try {
				const buildInfo = await fetch("/api/build-info", {
					credentials: "include",
				}).then((r) => r.json());
				await navigator.clipboard.writeText(buildInfo.commit);
			} catch (error) {
				console.error("Failed to copy commit SHA:", error);
			}
		},
	},
	{
		id: "refresh-health",
		name: "Refresh Health Status",
		shortcut: ["r", "h"],
		keywords: "refresh health status check",
		section: COMMAND_SECTIONS.SYSTEM,
		priority: 90,
		perform: () => {
			window.dispatchEvent(new CustomEvent("refresh-health"));
		},
	},
];

// Navigation commands factory function
export const createNavigationCommands = (
	navigate: (path: string) => void,
): Command[] => [
	{
		id: "go-dashboard",
		name: "Go to Dashboard",
		shortcut: ["g", "d"],
		keywords: "dashboard home",
		section: COMMAND_SECTIONS.NAVIGATION,
		priority: 100,
		perform: () => navigate("/"),
	},
	{
		id: "go-resources",
		name: "Go to Resource Browser",
		shortcut: ["g", "r"],
		keywords: "resources browser fhir explore",
		section: COMMAND_SECTIONS.NAVIGATION,
		priority: 95,
		perform: () => navigate("/resources"),
	},
	{
		id: "go-console",
		name: "Go to REST Console",
		shortcut: ["g", "c"],
		keywords: "rest console api request",
		section: COMMAND_SECTIONS.NAVIGATION,
		priority: 90,
		perform: () => navigate("/console"),
	},
	{
		id: "go-gateway",
		name: "Go to API Gateway",
		shortcut: ["g", "g"],
		keywords: "gateway api custom endpoints",
		section: COMMAND_SECTIONS.NAVIGATION,
		priority: 85,
		perform: () => navigate("/gateway"),
	},
	{
		id: "go-db-console",
		name: "Go to DB Console",
		shortcut: ["g", "q"],
		keywords: "database sql query console",
		section: COMMAND_SECTIONS.NAVIGATION,
		priority: 80,
		perform: () => navigate("/db-console"),
	},
	{
		id: "go-settings",
		name: "Go to Settings",
		shortcut: ["g", "s"],
		keywords: "settings preferences config",
		section: COMMAND_SECTIONS.NAVIGATION,
		priority: 75,
		perform: () => navigate("/settings"),
	},
	{
		id: "go-metadata",
		name: "Go to Capability Statement",
		shortcut: ["g", "m"],
		keywords: "metadata capability statement fhir",
		section: COMMAND_SECTIONS.NAVIGATION,
		priority: 70,
		perform: () => navigate("/metadata"),
	},
];

// Default navigation commands (fallback)
export const navigationCommands: Command[] = createNavigationCommands(() => {
	console.warn(
		"Navigation function not provided, commands may not work correctly",
	);
});

// Action commands
export const actionCommands: Command[] = [
	{
		id: "toggle-theme",
		name: "Toggle Theme",
		shortcut: ["t"],
		keywords: "theme dark light toggle",
		section: COMMAND_SECTIONS.THEME,
		priority: 85,
		perform: () => {
			window.dispatchEvent(new CustomEvent("toggle-theme"));
		},
	},
];

// REST Console specific commands
export const restConsoleCommands: Command[] = [
	{
		id: "send-request",
		name: "Send Request",
		shortcut: ["ctrl+enter"],
		keywords: "send request execute run",
		section: COMMAND_SECTIONS.REST_CONSOLE,
		priority: 100,
		perform: () => {
			window.dispatchEvent(new CustomEvent("send-request"));
		},
	},
	{
		id: "clear-response",
		name: "Clear Response",
		shortcut: ["ctrl+l"],
		keywords: "clear response reset",
		section: COMMAND_SECTIONS.REST_CONSOLE,
		priority: 90,
		perform: () => {
			window.dispatchEvent(new CustomEvent("clear-response"));
		},
	},
];

// Combine all commands
export const defaultCommands: Command[] = [
	...systemCommands,
	...navigationCommands,
	...actionCommands,
	...restConsoleCommands,
];
