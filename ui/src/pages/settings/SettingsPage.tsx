import { useState } from "react";
import {
	Stack,
	Title,
	Text,
	Card,
	Group,
	Badge,
	Button,
	NumberInput,
	Select,
	useMantineColorScheme,
} from "@mantine/core";
import { useHealth } from "@/shared/api/hooks";

const themeOptions = [
	{ value: "light", label: "Light" },
	{ value: "dark", label: "Dark" },
	{ value: "auto", label: "System" },
];

export function SettingsPage() {
	const { data: health, refetch, isRefetching } = useHealth({ refetchInterval: false });
	const { colorScheme, setColorScheme } = useMantineColorScheme();
	const [timeout, setTimeout] = useState(30000);

	const statusColor = {
		ok: "green",
		degraded: "yellow",
		down: "red",
	}[health?.status ?? "down"];

	const handleTestConnection = () => {
		refetch();
	};

	return (
		<Stack gap="lg">
			<div>
				<Title order={2}>Settings</Title>
				<Text c="dimmed">Configure server settings and preferences</Text>
			</div>

			<Card shadow="sm" padding="lg" radius="md" withBorder>
				<Title order={4} mb="md">
					Connection
				</Title>

				<Group justify="space-between" mb="lg">
					<Group>
						<Text>Server Status:</Text>
						<Badge color={statusColor} variant="light">
							{health?.status ?? "Unknown"}
						</Badge>
					</Group>
					<Button
						size="sm"
						variant="light"
						onClick={handleTestConnection}
						loading={isRefetching}
					>
						Test Connection
					</Button>
				</Group>

				<NumberInput
					label="Request Timeout (ms)"
					description="How long to wait before a request is aborted"
					value={timeout}
					onChange={(val) => setTimeout(Number(val) || 30000)}
					min={1000}
					max={120000}
					step={1000}
					w={300}
				/>
			</Card>

			<Card shadow="sm" padding="lg" radius="md" withBorder>
				<Title order={4} mb="md">
					Appearance
				</Title>

				<Select
					label="Theme"
					description="Choose your preferred color scheme"
					data={themeOptions}
					value={colorScheme}
					onChange={(val) => setColorScheme(val as "light" | "dark" | "auto")}
					w={300}
				/>
			</Card>
		</Stack>
	);
}

