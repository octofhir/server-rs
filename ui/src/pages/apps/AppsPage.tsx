import { Stack, Title, Text, Paper, Center, Group, Badge, ThemeIcon } from "@mantine/core";
import { IconApps, IconPlus } from "@tabler/icons-react";

export function AppsPage() {
	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<Group justify="space-between">
				<div>
					<Title order={2}>Apps</Title>
					<Text c="dimmed" size="sm">
						Manage custom applications and integrations
					</Text>
				</div>
			</Group>

			<Paper withBorder p="xl">
				<Center>
					<Stack align="center" gap="md">
						<ThemeIcon size={60} radius="xl" variant="light" color="gray">
							<IconApps size={30} />
						</ThemeIcon>
						<Title order={3} c="dimmed">
							No Apps Yet
						</Title>
						<Text c="dimmed" size="sm" ta="center" maw={400}>
							Apps allow you to create custom API endpoints, webhooks, and integrations.
							This feature is coming soon.
						</Text>
						<Badge variant="light" color="blue" leftSection={<IconPlus size={12} />}>
							Coming Soon
						</Badge>
					</Stack>
				</Center>
			</Paper>
		</Stack>
	);
}
