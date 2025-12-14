import { Stack, Title, Text, Paper, Center, ThemeIcon, Badge } from "@mantine/core";
import { IconShield, IconPlus } from "@tabler/icons-react";

export function AccessPoliciesPage() {
	return (
		<Stack gap="md" style={{ flex: 1, minHeight: 0 }}>
			<div>
				<Title order={2}>Access Policies</Title>
				<Text c="dimmed" size="sm">
					Configure access control policies for FHIR resources
				</Text>
			</div>

			<Paper withBorder p="xl">
				<Center>
					<Stack align="center" gap="md">
						<ThemeIcon size={60} radius="xl" variant="light" color="gray">
							<IconShield size={30} />
						</ThemeIcon>
						<Title order={3} c="dimmed">
							Policy Management Coming Soon
						</Title>
						<Text c="dimmed" size="sm" ta="center" maw={400}>
							Define fine-grained access control policies using JavaScript expressions
							to control who can access which FHIR resources.
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
