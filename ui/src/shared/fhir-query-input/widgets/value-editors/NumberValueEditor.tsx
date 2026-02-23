import { useCallback, useMemo } from "react";
import { Group, Select, TextInput } from "@mantine/core";

const PREFIX_OPTIONS = [
	{ value: "", label: "= (equals)" },
	{ value: "eq", label: "eq (equals)" },
	{ value: "ne", label: "ne (not equals)" },
	{ value: "gt", label: "gt (greater)" },
	{ value: "lt", label: "lt (less)" },
	{ value: "ge", label: "ge (greater or equal)" },
	{ value: "le", label: "le (less or equal)" },
];

const PREFIX_RE = /^(eq|ne|gt|lt|ge|le)([\d.+-].*)$/;

function parseValue(raw: string): { prefix: string; number: string } {
	const match = raw.match(PREFIX_RE);
	if (match) return { prefix: match[1], number: match[2] };
	return { prefix: "", number: raw };
}

export interface NumberValueEditorProps {
	value: string;
	onChange: (value: string) => void;
}

export function NumberValueEditor({ value, onChange }: NumberValueEditorProps) {
	const { prefix, number } = useMemo(() => parseValue(value), [value]);

	const handlePrefixChange = useCallback(
		(newPrefix: string | null) => {
			const p = newPrefix ?? "";
			onChange(p ? `${p}${number}` : number);
		},
		[number, onChange],
	);

	const handleNumberChange = useCallback(
		(e: React.ChangeEvent<HTMLInputElement>) => {
			const n = e.target.value;
			onChange(prefix ? `${prefix}${n}` : n);
		},
		[prefix, onChange],
	);

	return (
		<Group gap={4} style={{ flex: 1 }} wrap="nowrap">
			<Select
				data={PREFIX_OPTIONS}
				value={prefix}
				onChange={handlePrefixChange}
				size="xs"
				styles={{ root: { width: 100, flexShrink: 0 } }}
				allowDeselect={false}
			/>
			<TextInput
				value={number}
				onChange={handleNumberChange}
				size="xs"
				placeholder="Value"
				styles={{ root: { flex: 1 } }}
			/>
		</Group>
	);
}
