import { Paper, Stack, Text, type PaperProps } from "@mantine/core";
import type { ReactNode } from "react";

interface ConsolePanelProps extends PaperProps {
	title: string;
	subtitle?: string;
	children: ReactNode;
}

export function ConsolePanel({
	title,
	subtitle,
	children,
	...paperProps
}: ConsolePanelProps) {
	return (
		<Paper withBorder p="md" radius="md" {...paperProps}>
			<Stack gap="xs">
				<div>
					<Text fw={600}>{title}</Text>
					{subtitle ? (
						<Text size="sm" c="dimmed">
							{subtitle}
						</Text>
					) : null}
				</div>
				{children}
			</Stack>
		</Paper>
	);
}
