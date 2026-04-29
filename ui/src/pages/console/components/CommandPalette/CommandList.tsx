import { Box, Text, Stack } from "@/shared/ui";
import type { ConsoleCommand } from "../../commands/types";
import { CommandItem } from "./CommandItem";

interface CommandListProps {
	commands: ConsoleCommand[];
	selectedIndex: number;
	onExecute: (command: ConsoleCommand) => void;
}

export function CommandList({ commands, selectedIndex, onExecute }: CommandListProps) {
	if (commands.length === 0) {
		return (
			<Box py="xl">
				<Text color="secondary" style={{ textAlign: "center" }}>
					No commands found
				</Text>
			</Box>
		);
	}

	return (
		<Stack gap="1">
			{commands.map((command, index) => (
				<Box
					key={command.id}
					onClick={() => onExecute(command)}
					style={{
						padding: "8px 12px",
						borderRadius: "8px",
						cursor: "pointer",
						backgroundColor: index === selectedIndex ? "var(--g-color-base-selection)" : "transparent",
						transition: "background-color 0.1s ease",
					}}
					onMouseEnter={() => {}} // Could sync selectedIndex here if desired
				>
					<CommandItem command={command} />
				</Box>
			))}
		</Stack>
	);
}
