import { useCallback, useMemo } from "react";
import { Group, TextInput, Text } from "@mantine/core";

export interface TokenValueEditorProps {
	value: string;
	onChange: (value: string) => void;
}

function parseToken(raw: string): { system: string; code: string } {
	const pipeIdx = raw.indexOf("|");
	if (pipeIdx === -1) return { system: "", code: raw };
	return { system: raw.slice(0, pipeIdx), code: raw.slice(pipeIdx + 1) };
}

export function TokenValueEditor({ value, onChange }: TokenValueEditorProps) {
	const { system, code } = useMemo(() => parseToken(value), [value]);

	const handleSystemChange = useCallback(
		(e: React.ChangeEvent<HTMLInputElement>) => {
			const newSystem = e.target.value;
			if (newSystem || code) {
				onChange(`${newSystem}|${code}`);
			} else {
				onChange("");
			}
		},
		[code, onChange],
	);

	const handleCodeChange = useCallback(
		(e: React.ChangeEvent<HTMLInputElement>) => {
			const newCode = e.target.value;
			if (system) {
				onChange(`${system}|${newCode}`);
			} else {
				onChange(newCode);
			}
		},
		[system, onChange],
	);

	return (
		<Group gap={4} style={{ flex: 1 }} wrap="nowrap">
			<TextInput
				value={system}
				onChange={handleSystemChange}
				size="xs"
				placeholder="system URI"
				styles={{ root: { flex: 1 } }}
			/>
			<Text size="xs" c="dimmed" style={{ flexShrink: 0 }}>
				|
			</Text>
			<TextInput
				value={code}
				onChange={handleCodeChange}
				size="xs"
				placeholder="code"
				styles={{ root: { flex: 1 } }}
			/>
		</Group>
	);
}
