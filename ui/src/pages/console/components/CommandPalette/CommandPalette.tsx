import { IconSearch, Kbd, Modal, Text, TextInput } from "@octofhir/ui-kit";
import { useState, useRef, useEffect, useMemo } from "react";
import { useUnit } from "effector-react";
import {
	$commandPaletteOpen,
	$method,
	$mode,
	$rawPath,
	setBody,
	setCommandPaletteOpen,
	setCustomHeaders,
	setMethod,
	setMode,
	setRawPath,
} from "../../state/consoleStore";
import { useBuilderCommands, useHistoryCommands } from "../../commands/providers";
import { filterAndSortCommands } from "../../commands/fuzzySearch";
import type { ConsoleCommand, CommandContext } from "../../commands/types";
import { CommandList } from "./CommandList";
import styles from "./CommandPalette.module.css";

export function CommandPalette() {
	const {
		opened,
		setOpened,
		method,
		mode,
		rawPath,
		setMethod: setMethodEvent,
		setRawPath: setRawPathEvent,
		setBody: setBodyEvent,
		setMode: setModeEvent,
		setCustomHeaders: setCustomHeadersEvent,
	} = useUnit({
		opened: $commandPaletteOpen,
		setOpened: setCommandPaletteOpen,
		method: $method,
		mode: $mode,
		rawPath: $rawPath,
		setMethod,
		setRawPath,
		setBody,
		setMode,
		setCustomHeaders,
	});

	const [query, setQuery] = useState("");
	const [selectedIndex, setSelectedIndex] = useState(0);
	const inputRef = useRef<HTMLInputElement>(null);

	// Get commands from all providers
	const builderCommands = useBuilderCommands();
	const historyCommands = useHistoryCommands();

	// Combine all commands
	const allCommands = useMemo(
		() => [...builderCommands, ...historyCommands],
		[builderCommands, historyCommands],
	);

	// Filter and sort by search query
	const filteredCommands = useMemo(
		() => filterAndSortCommands(allCommands, query),
		[allCommands, query],
	);

	// Reset selection when query changes
	useEffect(() => {
		setSelectedIndex(0);
	}, [query]);

	// Create command execution context
	const commandContext = useMemo(
		(): CommandContext => ({
			setMethod: setMethodEvent,
			setRawPath: setRawPathEvent,
			setBody: setBodyEvent,
			setMode: setModeEvent,
			setCustomHeaders: setCustomHeadersEvent,
			currentMethod: method,
			currentMode: mode,
			currentPath: rawPath,
			closePalette: () => setOpened(false),
			trackEvent: (event, metadata) => {
				console.debug("[Analytics]", event, metadata);
			},
		}),
		[
			setMethodEvent,
			setRawPathEvent,
			setBodyEvent,
			setModeEvent,
			setCustomHeadersEvent,
			method,
			mode,
			rawPath,
			setOpened,
		],
	);

	const handleExecute = (command: ConsoleCommand) => {
		command.execute(commandContext);
		setOpened(false);
	};

	const handleKeyDown = (e: React.KeyboardEvent) => {
		if (e.key === "ArrowDown") {
			e.preventDefault();
			setSelectedIndex((prev) => (prev + 1) % filteredCommands.length);
		} else if (e.key === "ArrowUp") {
			e.preventDefault();
			setSelectedIndex((prev) => (prev - 1 + filteredCommands.length) % filteredCommands.length);
		} else if (e.key === "Enter") {
			e.preventDefault();
			const selected = filteredCommands[selectedIndex];
			if (selected) handleExecute(selected);
		} else if (e.key === "Escape") {
			setOpened(false);
		}
	};

	// Focus input when palette opens
	useEffect(() => {
		if (opened) {
			setTimeout(() => {
				inputRef.current?.focus();
			}, 0);
		} else {
			setQuery("");
		}
	}, [opened]);

	return (
		<Modal
			opened={opened}
			onClose={() => setOpened(false)}
			title={
				<span className={styles.title}>
					<IconSearch size={16} />
					<Text variant="subheader-1">Command Palette</Text>
				</span>
			}
			size="lg"
			trapFocus
			withCloseButton
		>
			<div className={styles.root}>
				<TextInput
					ref={inputRef}
					placeholder="Search commands (e.g. 'GET patient', 'clear history')..."
					value={query}
					onChange={(value) => setQuery(value)}
					onKeyDown={handleKeyDown}
					size="lg"
					autoFocus
				/>

				<div className={styles.list}>
					<CommandList
						commands={filteredCommands}
						selectedIndex={selectedIndex}
						onExecute={handleExecute}
					/>
				</div>

				<div className={styles.footer}>
					<div className={styles.footerKeys}>
						<Text><Kbd>↑↓</Kbd> Navigate</Text>
						<Text><Kbd>Enter</Kbd> Execute</Text>
					</div>
					<Text><Kbd>Esc</Kbd> Close</Text>
				</div>
			</div>
		</Modal>
	);
}
