import { TextInput } from "@mantine/core";
import { useUnit } from "effector-react";
import { $rawPath, setRawPath } from "../state/consoleStore";

export function RawPathInput() {
	const { rawPath, setRawPath: setRawPathEvent } = useUnit({
		rawPath: $rawPath,
		setRawPath,
	});

	return (
		<TextInput
			placeholder="/fhir/Patient or /api/operations"
			value={rawPath}
			onChange={(e) => setRawPathEvent(e.target.value)}
			size="md"
			styles={{
				input: {
					fontFamily: "monospace",
				},
			}}
		/>
	);
}
