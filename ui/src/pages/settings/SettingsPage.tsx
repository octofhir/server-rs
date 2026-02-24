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
	Switch,
	Loader,
	ScrollArea,
} from "@/shared/ui";
import { useMantineColorScheme } from "@octofhir/ui-kit";
import { useHealth, useFormatterSettings } from "@/shared/api/hooks";
import { useUiSettings } from "@/shared";
import { FormatterSettings } from "@/shared/settings/FormatterSettings";

const themeOptions = [
	{ value: "light", label: "Light" },
	{ value: "dark", label: "Dark" },
	{ value: "auto", label: "System" },
];

export function SettingsPage() {
	const { data: health, refetch, isRefetching } = useHealth({ refetchInterval: false });
	const { colorScheme, setColorScheme } = useMantineColorScheme();
	const [settings, setSettings] = useUiSettings();
	const {
		config: formatterConfig,
		isLoading: formatterLoading,
		saveConfig: saveFormatterConfig,
	} = useFormatterSettings();

	const statusColor = {
		ok: "primary",
		degraded: "warm",
		down: "fire",
	}[health?.status ?? "down"];

	const handleTestConnection = () => {
		refetch();
	};

	return (
		<ScrollArea h="100%" type="auto" offsetScrollbars>
			<Stack gap="lg" pb="xl">
				<div>
					<Title order={2}>Settings</Title>
					<Text c="dimmed">Configure server settings and preferences</Text>
				</div>

			<Card
				shadow="sm"
				padding="lg"
				radius="md"
				style={{ backgroundColor: "var(--octo-surface-1)" }}
			>
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
					value={settings.requestTimeoutMs}
					onChange={(val) =>
						setSettings((current) => ({
							...current,
							requestTimeoutMs: Number(val) || 30000,
						}))
					}
					min={1000}
					max={120000}
					step={1000}
					w={300}
				/>
			</Card>

			<Card
				shadow="sm"
				padding="lg"
				radius="md"
				style={{ backgroundColor: "var(--octo-surface-1)" }}
			>
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

			<Card
				shadow="sm"
				padding="lg"
				radius="md"
				style={{ backgroundColor: "var(--octo-surface-1)" }}
			>
				<Title order={4} mb="md">
					SQL Formatter
				</Title>
				<Text c="dimmed" size="sm" mb="md">
					Configure SQL formatting options for the DB Console editor.
				</Text>

				{formatterLoading ? (
					<Group>
						<Loader size="sm" />
						<Text size="sm" c="dimmed">Loading formatter settings...</Text>
					</Group>
				) : (
					<FormatterSettings
						value={formatterConfig}
						onChange={saveFormatterConfig}
					/>
				)}
			</Card>

			<Card
				shadow="sm"
				padding="lg"
				radius="md"
				style={{ backgroundColor: "var(--octo-surface-1)" }}
			>
				<Title order={4} mb="md">
					Console
				</Title>

				<Stack gap="md">
					<Switch
						label="Skip request validation"
						description="Allows sending malformed paths or missing parameters."
						checked={settings.skipConsoleValidation}
						onChange={(event) =>
							setSettings((current) => ({
								...current,
								skipConsoleValidation: event.currentTarget.checked,
							}))
						}
					/>
					<Switch
						label="Allow anonymous REST console requests"
						description="Send requests without cookies/credentials."
						checked={settings.allowAnonymousConsoleRequests}
						onChange={(event) =>
							setSettings((current) => ({
								...current,
								allowAnonymousConsoleRequests: event.currentTarget.checked,
							}))
						}
					/>
				</Stack>
			</Card>

			<Card
				shadow="sm"
				padding="lg"
				radius="md"
				style={{ backgroundColor: "var(--octo-surface-1)" }}
			>
				<Title order={4} mb="md">
					Authentication
				</Title>

				<Stack gap="md">
					<Switch
						label="Disable auto-logout on 401/403"
						description="Keeps the UI state when the session expires."
						checked={settings.disableAuthAutoLogout}
						onChange={(event) =>
							setSettings((current) => ({
								...current,
								disableAuthAutoLogout: event.currentTarget.checked,
							}))
						}
					/>
				</Stack>
			</Card>
			</Stack>
		</ScrollArea>
	);
}
