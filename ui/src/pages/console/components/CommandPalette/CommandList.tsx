import { Text } from "@octofhir/ui-kit";
import type { ConsoleCommand } from "../../commands/types";
import { CommandItem } from "./CommandItem";
import styles from "./CommandList.module.css";

interface CommandListProps {
	commands: ConsoleCommand[];
	selectedIndex: number;
	onExecute: (command: ConsoleCommand) => void;
}

export function CommandList({ commands, selectedIndex, onExecute }: CommandListProps) {
	if (commands.length === 0) {
		return (
			<div className={styles.empty}>
				<Text color="secondary">
					No commands found
				</Text>
			</div>
		);
	}

	return (
		<div className={styles.list}>
			{commands.map((command, index) => (
				<div
					key={command.id}
					onClick={() => onExecute(command)}
					className={
						index === selectedIndex
							? `${styles.item} ${styles.itemSelected}`
							: styles.item
					}
					onMouseEnter={() => {}} // Could sync selectedIndex here if desired
				>
					<CommandItem command={command} />
				</div>
			))}
		</div>
	);
}
