import { useCallback, useMemo } from "react";
import { Group, Select, TextInput, DateInput } from "@/shared/ui";

const PREFIX_OPTIONS = [
	{ value: "", label: "= (equals)" },
	{ value: "eq", label: "eq (equals)" },
	{ value: "ne", label: "ne (not equals)" },
	{ value: "gt", label: "gt (after)" },
	{ value: "lt", label: "lt (before)" },
	{ value: "ge", label: "ge (on or after)" },
	{ value: "le", label: "le (on or before)" },
	{ value: "sa", label: "sa (starts after)" },
	{ value: "eb", label: "eb (ends before)" },
	{ value: "ap", label: "ap (approximately)" },
];

// Match 2-char prefix at start of value
const PREFIX_RE = /^(eq|ne|gt|lt|ge|le|sa|eb|ap)(\d.*)$/;

function parseValue(raw: string): { prefix: string; date: string } {
	const match = raw.match(PREFIX_RE);
	if (match) return { prefix: match[1], date: match[2] };
	return { prefix: "", date: raw };
}

function formatValue(prefix: string, date: string): string {
	if (!date) return "";
	return prefix ? `${prefix}${date}` : date;
}

export interface DateValueEditorProps {
	value: string;
	onChange: (value: string) => void;
}

export function DateValueEditor({ value, onChange }: DateValueEditorProps) {
	const { prefix, date } = useMemo(() => parseValue(value), [value]);

	const dateObj = useMemo(() => {
		if (!date) return null;
		const d = new Date(date);
		return Number.isNaN(d.getTime()) ? null : d;
	}, [date]);

	const handlePrefixChange = useCallback(
		(newPrefix: string | null) => {
			onChange(formatValue(newPrefix ?? "", date));
		},
		[date, onChange],
	);

	const handleDateInputChange = useCallback(
		(d: Date | null) => {
			if (!d) {
				onChange("");
				return;
			}
			const iso = d.toISOString().slice(0, 10); // YYYY-MM-DD
			onChange(formatValue(prefix, iso));
		},
		[prefix, onChange],
	);

	const handleTextChange = useCallback(
		(e: React.ChangeEvent<HTMLInputElement>) => {
			onChange(formatValue(prefix, e.target.value));
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
			<DateInput
				value={dateObj}
				onChange={handleDateInputChange}
				size="xs"
				placeholder="YYYY-MM-DD"
				clearable
				valueFormat="YYYY-MM-DD"
				styles={{ root: { flex: 1, minWidth: 130 } }}
			/>
			<TextInput
				value={date}
				onChange={handleTextChange}
				size="xs"
				placeholder="or type manually"
				styles={{ root: { flex: 1, minWidth: 100 } }}
			/>
		</Group>
	);
}
