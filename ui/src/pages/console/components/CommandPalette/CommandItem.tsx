import { Badge, Text } from "@octofhir/ui-kit";
import type { ConsoleCommand } from "../../commands/types";
import styles from "./CommandItem.module.css";

interface CommandItemProps {
	command: ConsoleCommand;
}

export function CommandItem({ command }: CommandItemProps) {
	return (
		<div className={styles.root}>
			<div className={styles.main}>
				{command.icon && (
					<span className={styles.icon}>
						{command.icon}
					</span>
				)}
				<div className={styles.content}>
					<Text
						variant="body-1"
						className={styles.ellipsis}
					>
						{command.label}
					</Text>
					{command.description && (
						<Text
							variant="caption-1"
							color="secondary"
							className={styles.ellipsis}
						>
							{command.description}
						</Text>
					)}
				</div>
			</div>

			{command.badge && (
				<Badge
					size="s"
					color={command.badgeColor ?? "gray"}
					className={styles.badge}
				>
					{command.badge}
				</Badge>
			)}
		</div>
	);
}
