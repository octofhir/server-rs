import { Box, Text, Stack } from "@mantine/core";
import { Combobox } from "@mantine/core";
import type { ConsoleCommand } from "../../commands/types";
import { CommandItem } from "./CommandItem";

interface CommandListProps {
	grouped: Map<string, ConsoleCommand[]>;
	onExecute: (command: ConsoleCommand) => void;
}

const CATEGORY_LABELS: Record<string, string> = {
	history: "Recent History",
	builder: "Builder Actions",
	snippet: "Saved Snippets",
	navigation: "Navigation",
};

const CATEGORY_ORDER = ["history", "builder", "snippet", "navigation"];

export function CommandList({ grouped, onExecute }: CommandListProps) {
	if (grouped.size === 0) {
		return (
			<Box py="xl">
				<Text c="dimmed" ta="center">
					No commands found
				</Text>
			</Box>
		);
	}

	return (
		<Stack gap="md" py="sm">
			{CATEGORY_ORDER.map((category) => {
				const commands = grouped.get(category);
				if (!commands || commands.length === 0) return null;

				return (
					<Box key={category}>
						<Text size="xs" fw={600} c="dimmed" mb="xs" px="sm">
							{CATEGORY_LABELS[category] || category}
						</Text>
						<Combobox.Options>
							{commands.map((command) => (
								<Combobox.Option key={command.id} value={command.id}>
									<CommandItem command={command} />
								</Combobox.Option>
							))}
						</Combobox.Options>
					</Box>
				);
			})}
		</Stack>
	);
}
