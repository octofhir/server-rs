import { Stack, Group, Text, TextInput, ActionIcon, Button, Alert } from "@mantine/core";
import { IconX, IconPlus, IconLock } from "@tabler/icons-react";
import { useConsoleStore } from "../state/consoleStore";
import { validateHeaders } from "../utils/headerUtils";

export function HeaderEditor() {
	const defaultHeaders = useConsoleStore((state) => state.defaultHeaders);
	const customHeaders = useConsoleStore((state) => state.customHeaders);
	const addCustomHeader = useConsoleStore((state) => state.addCustomHeader);
	const removeCustomHeader = useConsoleStore((state) => state.removeCustomHeader);
	const updateCustomHeader = useConsoleStore((state) => state.updateCustomHeader);

	const allHeaders = { ...defaultHeaders, ...customHeaders };
	const errors = validateHeaders(allHeaders);

	const handleAddHeader = () => {
		addCustomHeader("", "");
	};

	return (
		<Stack gap="sm">
			<Text fw={500} size="sm">
				Headers
			</Text>

			{/* Default headers */}
			<Stack gap="xs">
				<Text size="xs" c="dimmed">
					Default FHIR headers
				</Text>
				{Object.entries(defaultHeaders).map(([key, value]) => (
					<Group key={key} gap="xs">
						<TextInput value={key} disabled size="xs" style={{ flex: 1 }} />
						<TextInput value={value} disabled size="xs" style={{ flex: 2 }} />
						<ActionIcon variant="subtle" disabled size="sm">
							<IconLock size={14} />
						</ActionIcon>
					</Group>
				))}
			</Stack>

			{/* Custom headers */}
			<Stack gap="xs">
				<Group justify="space-between">
					<Text size="xs" c="dimmed">
						Custom headers
					</Text>
					<Button size="xs" leftSection={<IconPlus size={14} />} onClick={handleAddHeader}>
						Add
					</Button>
				</Group>
				{Object.entries(customHeaders).map(([key, value]) => (
					<Group key={key} gap="xs">
						<TextInput
							placeholder="Header-Name"
							value={key}
							onChange={(e) => updateCustomHeader(key, e.target.value, value)}
							size="xs"
							style={{ flex: 1 }}
						/>
						<TextInput
							placeholder="value"
							value={value}
							onChange={(e) => updateCustomHeader(key, key, e.target.value)}
							size="xs"
							style={{ flex: 2 }}
						/>
						<ActionIcon variant="subtle" onClick={() => removeCustomHeader(key)} size="sm" color="red">
							<IconX size={14} />
						</ActionIcon>
					</Group>
				))}
			</Stack>

			{/* Validation warnings */}
			{errors.length > 0 && (
				<Alert color="yellow" title="Validation warnings">
					{errors.map((error, i) => (
						<Text key={i} size="xs">
							{error}
						</Text>
					))}
				</Alert>
			)}

			<Text size="xs" c="dimmed">
				{Object.keys(customHeaders).length} custom headers
			</Text>
		</Stack>
	);
}
