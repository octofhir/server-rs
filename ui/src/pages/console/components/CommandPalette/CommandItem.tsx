import { Group, Stack, Text, Badge, Box } from "@/shared/ui";
import type { ConsoleCommand } from "../../commands/types";

interface CommandItemProps {
	command: ConsoleCommand;
}

export function CommandItem({ command }: CommandItemProps) {
	return (
		<Group justify="space-between" wrap="nowrap">
			<Group gap="sm" wrap="nowrap" style={{ flex: 1, minWidth: 0 }}>
				{command.icon && (
					<Box c="dimmed" style={{ flexShrink: 0 }}>
						{command.icon}
					</Box>
				)}
				<Stack gap={0} style={{ flex: 1, minWidth: 0 }}>
					<Text
						size="sm"
						style={{
							overflow: "hidden",
							textOverflow: "ellipsis",
							whiteSpace: "nowrap",
						}}
					>
						{command.label}
					</Text>
					{command.description && (
						<Text
							size="xs"
							c="dimmed"
							style={{
								overflow: "hidden",
								textOverflow: "ellipsis",
								whiteSpace: "nowrap",
							}}
						>
							{command.description}
						</Text>
					)}
				</Stack>
			</Group>

			{command.badge && (
				<Badge
					size="sm"
					variant="light"
					color={command.badgeColor || "gray"}
					style={{ flexShrink: 0 }}
				>
					{command.badge}
				</Badge>
			)}
		</Group>
	);
}
