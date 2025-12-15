import { useState, useCallback } from "react";
import { useConsoleStore } from "../../state/consoleStore";
import type { ConsoleCommand } from "../types";

/**
 * Return type for the useConsoleCommands hook
 */
export interface UseConsoleCommandsReturn {
	/** Open the command palette */
	openPalette: () => void;

	/** Close the command palette */
	closePalette: () => void;

	/** Register a custom command (for extensions) */
	registerAction: (command: ConsoleCommand) => void;

	/** Unregister a custom command by ID */
	unregisterAction: (id: string) => void;

	/** Get all registered custom commands */
	customCommands: ConsoleCommand[];
}

/**
 * Public API hook for interacting with the console command palette
 *
 * Provides programmatic access to open/close the palette and register
 * custom commands for extensions.
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { openPalette, registerAction } = useConsoleCommands();
 *
 *   useEffect(() => {
 *     // Register a custom command
 *     registerAction({
 *       id: 'my-custom-action',
 *       label: 'Do Something Custom',
 *       category: 'navigation',
 *       execute: (ctx) => {
 *         console.log('Custom action executed!');
 *         ctx.closePalette();
 *       }
 *     });
 *   }, []);
 *
 *   return <button onClick={openPalette}>Open Palette</button>;
 * }
 * ```
 */
export function useConsoleCommands(): UseConsoleCommandsReturn {
	const setCommandPaletteOpen = useConsoleStore(
		(state) => state.setCommandPaletteOpen,
	);

	// Custom command registry (for future extensibility)
	const [customCommands, setCustomCommands] = useState<ConsoleCommand[]>([]);

	const openPalette = useCallback(() => {
		setCommandPaletteOpen(true);
	}, [setCommandPaletteOpen]);

	const closePalette = useCallback(() => {
		setCommandPaletteOpen(false);
	}, [setCommandPaletteOpen]);

	const registerAction = useCallback((command: ConsoleCommand) => {
		setCustomCommands((prev) => {
			// Prevent duplicate IDs
			if (prev.some((cmd) => cmd.id === command.id)) {
				console.warn(
					`[useConsoleCommands] Command with ID "${command.id}" already registered`,
				);
				return prev;
			}
			return [...prev, command];
		});
	}, []);

	const unregisterAction = useCallback((id: string) => {
		setCustomCommands((prev) => prev.filter((cmd) => cmd.id !== id));
	}, []);

	return {
		openPalette,
		closePalette,
		registerAction,
		unregisterAction,
		customCommands,
	};
}
