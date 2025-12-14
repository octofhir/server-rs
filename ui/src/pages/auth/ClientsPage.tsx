import { Stack, Title, Text, Paper, Center, ThemeIcon, Badge } from "@mantine/core";
import { IconKey, IconPlus } from "@tabler/icons-react";

export function ClientsPage() {
	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<div>
				<Title order={2}>OAuth Clients</Title>
				<Text c="dimmed" size="sm">
					Manage OAuth 2.0 clients and SMART on FHIR applications
				</Text>
			</div>

			<Paper withBorder p="xl">
				<Center>
					<Stack align="center" gap="md">
						<ThemeIcon size={60} radius="xl" variant="light" color="gray">
							<IconKey size={30} />
						</ThemeIcon>
						<Title order={3} c="dimmed">
							Client Management Coming Soon
						</Title>
						<Text c="dimmed" size="sm" ta="center" maw={400}>
							Register and manage OAuth clients, configure scopes, and set up SMART on FHIR
							application credentials.
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
