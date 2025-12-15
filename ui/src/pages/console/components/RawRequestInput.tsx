import { Stack, Text, TextInput } from "@mantine/core";
import { useConsoleStore } from "../state/consoleStore";

export function RawRequestInput() {
	const rawPath = useConsoleStore((state) => state.rawPath);
	const setRawPath = useConsoleStore((state) => state.setRawPath);

	return (
		<Stack gap="xs">
			<Text fw={500} size="sm">
				Raw Request Path
			</Text>
			<TextInput
				placeholder="/fhir/Patient?name=John&_count=10"
				value={rawPath}
				onChange={(e) => setRawPath(e.target.value)}
				size="sm"
			/>
			<Text size="xs" c="dimmed">
				Enter the full request path including /fhir prefix and query parameters
			</Text>
		</Stack>
	);
}
