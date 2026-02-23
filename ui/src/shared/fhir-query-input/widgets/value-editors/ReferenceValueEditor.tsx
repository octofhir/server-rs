import { useCallback, useMemo } from "react";
import { Group, Select, TextInput } from "@mantine/core";
import type { RestConsoleSearchParam } from "@/shared/api";

export interface ReferenceValueEditorProps {
	value: string;
	onChange: (value: string) => void;
	/** Target resource types for this reference param */
	targets: string[];
	/** Search params available on target resource (for chaining) */
	targetSearchParams?: Record<string, RestConsoleSearchParam[]>;
}

export function ReferenceValueEditor({
	value,
	onChange,
	targets,
	targetSearchParams,
}: ReferenceValueEditorProps) {
	// If targets are available, offer structured input
	// Reference value can be: "123", "Patient/123", or a chain like "Patient.name"
	const isChain = value.includes(".");

	const targetOptions = useMemo(
		() => targets.map((t) => ({ value: t, label: t })),
		[targets],
	);

	// Parse "ResourceType/id" format
	const { targetType, refId } = useMemo(() => {
		const slashIdx = value.indexOf("/");
		if (slashIdx > 0) {
			return { targetType: value.slice(0, slashIdx), refId: value.slice(slashIdx + 1) };
		}
		return { targetType: "", refId: value };
	}, [value]);

	const handleTargetChange = useCallback(
		(t: string | null) => {
			if (t && refId) {
				onChange(`${t}/${refId}`);
			} else if (t) {
				onChange(`${t}/`);
			} else {
				onChange(refId);
			}
		},
		[refId, onChange],
	);

	const handleIdChange = useCallback(
		(e: React.ChangeEvent<HTMLInputElement>) => {
			const id = e.target.value;
			if (targetType) {
				onChange(`${targetType}/${id}`);
			} else {
				onChange(id);
			}
		},
		[targetType, onChange],
	);

	if (targets.length === 0) {
		return (
			<TextInput
				value={value}
				onChange={(e) => onChange(e.target.value)}
				size="xs"
				placeholder="Reference ID or ResourceType/ID"
				styles={{ root: { flex: 1 } }}
			/>
		);
	}

	return (
		<Group gap={4} style={{ flex: 1 }} wrap="nowrap">
			{targets.length > 1 ? (
				<Select
					data={targetOptions}
					value={targetType || null}
					onChange={handleTargetChange}
					size="xs"
					placeholder="Type"
					clearable
					searchable
					styles={{ root: { width: 130, flexShrink: 0 } }}
				/>
			) : null}
			<TextInput
				value={isChain ? value : refId}
				onChange={isChain ? (e) => onChange(e.target.value) : handleIdChange}
				size="xs"
				placeholder={targets.length === 1 ? `${targets[0]} ID` : "ID"}
				styles={{ root: { flex: 1 } }}
			/>
		</Group>
	);
}
