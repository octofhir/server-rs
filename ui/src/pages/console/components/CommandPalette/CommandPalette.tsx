import { useState, useRef, useEffect, useMemo } from "react";
import {
	Modal,
	TextInput,
	Stack,
	Group,
	Text,
	Kbd,
	ScrollArea,
	Combobox,
} from "@/shared/ui";
import { useCombobox } from "@octofhir/ui-kit";
import { IconSearch } from "@tabler/icons-react";
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
	const inputRef = useRef<HTMLInputElement>(null);

	// Debug logging
	useEffect(() => {
		console.log("[CommandPalette] opened state:", opened);
	}, [opened]);

	const combobox = useCombobox({
		onDropdownClose: () => {
			combobox.resetSelectedOption();
		},
	});

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

	// Group commands by category
	const groupedCommands = useMemo(() => {
		const groups = new Map<string, ConsoleCommand[]>();

		for (const cmd of filteredCommands) {
			const existing = groups.get(cmd.category) || [];
			groups.set(cmd.category, [...existing, cmd]);
		}

		return groups;
	}, [filteredCommands]);

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
	};

	// Focus input and open dropdown when palette opens
	useEffect(() => {
		if (opened) {
			setTimeout(() => {
				inputRef.current?.focus();
				combobox.openDropdown();
			}, 0);
		} else {
			setQuery("");
			combobox.closeDropdown();
		}
	}, [opened, combobox]);

	return (
		<Modal
			opened={opened}
			onClose={() => setOpened(false)}
			title="Command Palette"
			size="lg"
			trapFocus
			withCloseButton
			aria-labelledby="command-palette-title"
		>
			<Stack gap="md">
				<Combobox
					store={combobox}
					onOptionSubmit={(optionId) => {
						const command = allCommands.find((c) => c.id === optionId);
						if (command) {
							handleExecute(command);
						}
					}}
				>
					<Combobox.Target>
						<TextInput
							ref={inputRef}
							placeholder="Type to search commands..."
							value={query}
							onChange={(e) => setQuery(e.currentTarget.value)}
							leftSection={<IconSearch size={16} />}
							onKeyDown={(e) => {
								if (e.key === "Escape") {
									e.preventDefault();
									setOpened(false);
								}
							}}
						/>
					</Combobox.Target>

					<Combobox.Dropdown>
						<ScrollArea.Autosize mah={400}>
							<CommandList
								grouped={groupedCommands}
								onExecute={handleExecute}
							/>
						</ScrollArea.Autosize>
					</Combobox.Dropdown>
				</Combobox>

				<Group justify="space-between" c="dimmed" fz="xs">
					<Text>
						<Kbd>↑</Kbd> <Kbd>↓</Kbd> Navigate
					</Text>
					<Text>
						<Kbd>Enter</Kbd> Execute
					</Text>
					<Text>
						<Kbd>Esc</Kbd> Close
					</Text>
				</Group>
			</Stack>
		</Modal>
	);
}
