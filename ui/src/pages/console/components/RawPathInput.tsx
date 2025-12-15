import { TextInput } from "@mantine/core";
import { useConsoleStore } from "../state/consoleStore";

export function RawPathInput() {
	const rawPath = useConsoleStore((state) => state.rawPath);
	const setRawPath = useConsoleStore((state) => state.setRawPath);

	return (
		<TextInput
			placeholder="/fhir/Patient or /api/operations"
			value={rawPath}
			onChange={(e) => setRawPath(e.target.value)}
			size="md"
			styles={{
				input: {
					fontFamily: "monospace",
				},
			}}
		/>
	);
}
