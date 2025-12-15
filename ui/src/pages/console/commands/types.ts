import type { ReactNode } from "react";
import type { HttpMethod } from "@/shared/api";
import type { ConsoleMode } from "../state/consoleStore";

/**
 * Category of command for grouping in the command palette
 */
export type CommandCategory = "history" | "builder" | "snippet" | "navigation";

/**
 * Context provided to command execute functions
 * Provides access to store actions and current state
 */
export interface CommandContext {
	// Store actions
	setMethod: (method: HttpMethod) => void;
	setRawPath: (path: string) => void;
	setBody: (body: string) => void;
	setMode: (mode: ConsoleMode) => void;
	setCustomHeaders: (headers: Record<string, string>) => void;

	// Current state (for conditional availability)
	currentMethod: HttpMethod;
	currentMode: ConsoleMode;
	currentPath: string;

	// UI control
	closePalette: () => void;

	// Analytics
	trackEvent?: (event: string, metadata?: Record<string, unknown>) => void;
}

/**
 * A command that can be executed from the command palette
 */
export interface ConsoleCommand {
	/** Unique identifier for this command */
	id: string;

	/** Display text shown in the palette (e.g., "GET /fhir/Patient/123") */
	label: string;

	/** Optional secondary information (e.g., "2 minutes ago") */
	description?: string;

	/** Category for grouping commands */
	category: CommandCategory;

	/** Additional keywords for fuzzy search matching */
	keywords?: string[];

	/** Optional badge text (e.g., "200 OK", "POST") */
	badge?: string;

	/** Mantine color for the badge */
	badgeColor?: string;

	/** Optional icon to display */
	icon?: ReactNode;

	/**
	 * Execute the command
	 * @param context - Context with store actions and state
	 */
	execute: (context: CommandContext) => void | Promise<void>;

	/**
	 * Optional function to determine if command is available
	 * @param context - Current application context
	 * @returns true if command should be shown
	 */
	isAvailable?: (context: CommandContext) => boolean;
}

/**
 * Provider that generates commands
 */
export interface CommandProvider {
	/** Provider name for debugging */
	name: string;

	/** Generate commands based on current state */
	getCommands: () => ConsoleCommand[];
}

/**
 * Hook that provides commands
 */
export type UseCommandProvider = () => ConsoleCommand[];
