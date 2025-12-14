import { Stack, Title, Text, Paper, Center, ThemeIcon, Badge } from "@mantine/core";
import { IconUsers, IconPlus } from "@tabler/icons-react";

export function UsersPage() {
	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<div>
				<Title order={2}>Users</Title>
				<Text c="dimmed" size="sm">
					Manage user accounts and credentials
				</Text>
			</div>

			<Paper withBorder p="xl">
				<Center>
					<Stack align="center" gap="md">
						<ThemeIcon size={60} radius="xl" variant="light" color="gray">
							<IconUsers size={30} />
						</ThemeIcon>
						<Title order={3} c="dimmed">
							User Management Coming Soon
						</Title>
						<Text c="dimmed" size="sm" ta="center" maw={400}>
							Create and manage user accounts, reset passwords, and configure user roles
							and permissions.
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
