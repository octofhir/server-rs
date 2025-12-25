import { Stack, Group, Text, TextInput, ActionIcon, Button, Alert } from "@mantine/core";
import { IconX, IconPlus, IconLock } from "@tabler/icons-react";
import { useUnit } from "effector-react";
import {
	$customHeaders,
	$defaultHeaders,
	addCustomHeader,
	removeCustomHeader,
	updateCustomHeader,
} from "../state/consoleStore";
import { validateHeaders } from "../utils/headerUtils";

export function HeaderEditor() {
	const {
		defaultHeaders,
		customHeaders,
		addCustomHeader: addCustomHeaderEvent,
		removeCustomHeader: removeCustomHeaderEvent,
		updateCustomHeader: updateCustomHeaderEvent,
	} = useUnit({
		defaultHeaders: $defaultHeaders,
		customHeaders: $customHeaders,
		addCustomHeader,
		removeCustomHeader,
		updateCustomHeader,
	});

	const allHeaders = { ...defaultHeaders, ...customHeaders };
	const errors = validateHeaders(allHeaders);

	const handleAddHeader = () => {
		addCustomHeaderEvent({ key: "", value: "" });
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
							onChange={(e) =>
								updateCustomHeaderEvent({
									oldKey: key,
									newKey: e.target.value,
									value,
								})
							}
							size="xs"
							style={{ flex: 1 }}
						/>
						<TextInput
							placeholder="value"
							value={value}
							onChange={(e) =>
								updateCustomHeaderEvent({
									oldKey: key,
									newKey: key,
									value: e.target.value,
								})
							}
							size="xs"
							style={{ flex: 2 }}
						/>
						<ActionIcon
							variant="subtle"
							onClick={() => removeCustomHeaderEvent(key)}
							size="sm"
							color="fire"
						>
							<IconX size={14} />
						</ActionIcon>
					</Group>
				))}
			</Stack>

			{/* Validation warnings */}
			{errors.length > 0 && (
				<Alert color="warm" title="Validation warnings">
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
