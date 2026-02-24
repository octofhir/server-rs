import { useCallback, useMemo } from "react";
import {
	Stack,
	Group,
	Select,
	TextInput,
	ActionIcon,
	Button,
	Badge,
	Text,
	Code,
	Box,
	NumberInput,
	SegmentedControl,
	type ComboboxData,
} from "@/shared/ui";
import { IconPlus, IconTrash } from "@tabler/icons-react";
import type { BuilderState, BuilderParam } from "../core/builder-model";
import type { QueryInputMetadata } from "../core/types";
import type { RestConsoleSearchParam } from "@/shared/api";
import { DateValueEditor } from "./value-editors/DateValueEditor";
import { TokenValueEditor } from "./value-editors/TokenValueEditor";
import { ReferenceValueEditor } from "./value-editors/ReferenceValueEditor";
import { NumberValueEditor } from "./value-editors/NumberValueEditor";

export interface QueryChipsBuilderProps {
	state: BuilderState;
	onChange: (state: BuilderState) => void;
	metadata: QueryInputMetadata;
}

let nextParamId = 1;

export function QueryChipsBuilder({
	state,
	onChange,
	metadata,
}: QueryChipsBuilderProps) {
	const resourceTypeOptions = useMemo(
		() =>
			metadata.resourceTypes.map((rt) => ({
				value: rt,
				label: rt,
			})),
		[metadata.resourceTypes],
	);

	const availableParams = useMemo(() => {
		if (!state.resourceType) return [];
		const params = metadata.searchParamsByResource[state.resourceType] ?? [];
		const common = params
			.filter((p) => p.is_common)
			.map((p) => ({ value: p.code, label: p.code }));
		const specific = params
			.filter((p) => !p.is_common)
			.map((p) => ({ value: p.code, label: p.code }));
		const groups: Array<{ group: string; items: Array<{ value: string; label: string }> }> = [];
		if (common.length > 0) groups.push({ group: "Common", items: common });
		if (specific.length > 0) groups.push({ group: "Resource-specific", items: specific });
		return groups;
	}, [state.resourceType, metadata.searchParamsByResource]);

	const handleResourceTypeChange = useCallback(
		(value: string | null) => {
			onChange({
				...state,
				resourceType: value ?? undefined,
				params: state.params.filter((p) => p.isSpecial),
			});
		},
		[state, onChange],
	);

	const handleResourceIdChange = useCallback(
		(value: string) => {
			onChange({ ...state, resourceId: value || undefined });
		},
		[state, onChange],
	);

	const handleAddParam = useCallback(() => {
		const newParam: BuilderParam = {
			id: `bp_${nextParamId++}`,
			code: "",
			value: "",
			isSpecial: false,
		};
		onChange({ ...state, params: [...state.params, newParam] });
	}, [state, onChange]);

	const handleAddSpecialParam = useCallback(
		(name: string) => {
			if (state.params.some((p) => p.code === name)) return;
			const newParam: BuilderParam = {
				id: `bp_${nextParamId++}`,
				code: name,
				value: "",
				isSpecial: true,
			};
			onChange({ ...state, params: [...state.params, newParam] });
		},
		[state, onChange],
	);

	const handleUpdateParam = useCallback(
		(id: string, updates: Partial<BuilderParam>) => {
			onChange({
				...state,
				params: state.params.map((p) =>
					p.id === id ? { ...p, ...updates } : p,
				),
			});
		},
		[state, onChange],
	);

	const handleRemoveParam = useCallback(
		(id: string) => {
			onChange({
				...state,
				params: state.params.filter((p) => p.id !== id),
			});
		},
		[state, onChange],
	);

	const regularParams = state.params.filter((p) => !p.isSpecial);
	const specialParams = state.params.filter((p) => p.isSpecial);

	// Special params that can be added
	const availableSpecialParams = useMemo(() => {
		const existing = new Set(specialParams.map((p) => p.code));
		return ["_count", "_offset", "_sort", "_summary", "_total", "_elements", "_include", "_revinclude"]
			.filter((name) => !existing.has(name));
	}, [specialParams]);

	return (
		<Stack gap="md">
			{/* Resource Type Selector */}
			<Group gap="sm" align="flex-end">
				<Select
					label="Resource Type"
					placeholder="Select resource type"
					data={resourceTypeOptions}
					value={state.resourceType ?? null}
					onChange={handleResourceTypeChange}
					searchable
					clearable
					styles={{ root: { flex: 1 } }}
				/>
				<TextInput
					label="Resource ID"
					placeholder="Optional"
					value={state.resourceId ?? ""}
					onChange={(e) => handleResourceIdChange(e.target.value)}
					styles={{ root: { width: 160 } }}
				/>
			</Group>

			{/* Search Parameters */}
			{state.resourceType && (
				<Box>
					<Group justify="space-between" mb="xs">
						<Text size="sm" fw={600}>
							Search Parameters
						</Text>
						<Button
							variant="light"
							size="xs"
							leftSection={<IconPlus size={14} />}
							onClick={handleAddParam}
						>
							Add Parameter
						</Button>
					</Group>

					<Stack gap="xs">
						{regularParams.length === 0 ? (
							<Text size="xs" c="dimmed">
								No search parameters added. Click "Add Parameter" to filter
								results.
							</Text>
						) : (
							regularParams.map((param) => (
								<ParamChip
									key={param.id}
									param={param}
									availableParams={availableParams}
									metadata={metadata}
									resourceType={state.resourceType}
									onUpdate={(updates) =>
										handleUpdateParam(param.id, updates)
									}
									onRemove={() => handleRemoveParam(param.id)}
								/>
							))
						)}
					</Stack>
				</Box>
			)}

			{/* Special Parameters */}
			<Box>
				<Group justify="space-between" mb="xs">
					<Text size="sm" fw={600}>
						Special Parameters
					</Text>
					{availableSpecialParams.length > 0 && (
						<Group gap={4}>
							{availableSpecialParams.slice(0, 4).map((name) => (
								<Badge
									key={name}
									size="xs"
									variant="light"
									color="orange"
									style={{ cursor: "pointer" }}
									onClick={() => handleAddSpecialParam(name)}
								>
									+ {name}
								</Badge>
							))}
						</Group>
					)}
				</Group>

				<Stack gap="xs">
					{specialParams.map((param) => (
						<SpecialParamChip
							key={param.id}
							param={param}
							metadata={metadata}
							resourceType={state.resourceType}
							onUpdate={(updates) =>
								handleUpdateParam(param.id, updates)
							}
							onRemove={() => handleRemoveParam(param.id)}
						/>
					))}
				</Stack>
			</Box>

			{/* Preview */}
			<Box
				p="xs"
				style={{
					backgroundColor: "var(--octo-surface-2)",
					borderRadius: "var(--mantine-radius-sm)",
					border: "1px solid var(--octo-border-subtle)",
				}}
			>
				<Text size="xs" fw={600} c="dimmed" mb={2}>
					PREVIEW
				</Text>
				<Code style={{ fontSize: 11, wordBreak: "break-all" }}>
					{buildPreviewUrl(state)}
				</Code>
			</Box>
		</Stack>
	);
}

interface ParamChipProps {
	param: BuilderParam;
	availableParams: ComboboxData;
	metadata: QueryInputMetadata;
	resourceType?: string;
	onUpdate: (updates: Partial<BuilderParam>) => void;
	onRemove: () => void;
}

function ParamChip({
	param,
	availableParams,
	metadata,
	resourceType,
	onUpdate,
	onRemove,
}: ParamChipProps) {
	// Look up param definition for type info and modifiers
	const paramDef = useMemo(() => {
		if (!resourceType || !param.code) return undefined;
		const params = metadata.searchParamsByResource[resourceType] ?? [];
		return params.find((p) => p.code === param.code);
	}, [resourceType, param.code, metadata.searchParamsByResource]);

	const modifierOptions = useMemo(() => {
		if (!paramDef?.modifiers?.length) return [];
		return [
			{ value: "", label: "(none)" },
			...paramDef.modifiers.map((m) => ({
				value: m.code,
				label: `:${m.code}`,
			})),
		];
	}, [paramDef]);

	const paramType = paramDef?.type;

	return (
		<Group
			gap="xs"
			p="xs"
			style={{
				backgroundColor: "var(--octo-surface-2)",
				borderRadius: "var(--mantine-radius-sm)",
				border: "1px solid var(--octo-border-subtle)",
			}}
		>
			<Select
				placeholder="Parameter"
				data={availableParams}
				value={param.code || null}
				onChange={(v) => onUpdate({ code: v ?? "", value: "" })}
				searchable
				size="xs"
				styles={{ root: { width: 150 } }}
			/>
			{modifierOptions.length > 0 && (
				<Select
					placeholder="Modifier"
					data={modifierOptions}
					value={param.modifier ?? ""}
					onChange={(v) => onUpdate({ modifier: v || undefined })}
					size="xs"
					styles={{ root: { width: 110 } }}
					clearable
				/>
			)}
			<ParamValueEditor
				paramType={paramType}
				value={param.value}
				onChange={(v) => onUpdate({ value: v })}
				targets={paramDef?.targets ?? []}
				searchParamsByResource={metadata.searchParamsByResource}
			/>
			<ActionIcon
				variant="subtle"
				color="red"
				size="sm"
				onClick={onRemove}
			>
				<IconTrash size={14} />
			</ActionIcon>
		</Group>
	);
}

function ParamValueEditor({
	paramType,
	value,
	onChange,
	targets,
	searchParamsByResource,
}: {
	paramType?: string;
	value: string;
	onChange: (value: string) => void;
	targets: string[];
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
}) {
	switch (paramType) {
		case "date":
			return <DateValueEditor value={value} onChange={onChange} />;
		case "number":
		case "quantity":
			return <NumberValueEditor value={value} onChange={onChange} />;
		case "token":
			return <TokenValueEditor value={value} onChange={onChange} />;
		case "reference":
			return (
				<ReferenceValueEditor
					value={value}
					onChange={onChange}
					targets={targets}
					targetSearchParams={searchParamsByResource}
				/>
			);
		default:
			return (
				<TextInput
					placeholder="Value"
					value={value}
					onChange={(e) => onChange(e.target.value)}
					size="xs"
					styles={{ root: { flex: 1 } }}
				/>
			);
	}
}

interface SpecialParamChipProps {
	param: BuilderParam;
	metadata: QueryInputMetadata;
	resourceType?: string;
	onUpdate: (updates: Partial<BuilderParam>) => void;
	onRemove: () => void;
}

function SpecialParamChip({
	param,
	metadata,
	resourceType,
	onUpdate,
	onRemove,
}: SpecialParamChipProps) {
	return (
		<Group
			gap="xs"
			p="xs"
			style={{
				backgroundColor: "var(--octo-surface-2)",
				borderRadius: "var(--mantine-radius-sm)",
				border: "1px solid var(--octo-border-subtle)",
			}}
		>
			<Badge size="sm" variant="light" color="orange" style={{ flexShrink: 0 }}>
				{param.code}
			</Badge>
			<SpecialParamValueEditor
				paramName={param.code}
				value={param.value}
				metadata={metadata}
				resourceType={resourceType}
				onChange={(v) => onUpdate({ value: v })}
			/>
			<ActionIcon
				variant="subtle"
				color="red"
				size="sm"
				onClick={onRemove}
			>
				<IconTrash size={14} />
			</ActionIcon>
		</Group>
	);
}

function SpecialParamValueEditor({
	paramName,
	value,
	metadata,
	resourceType,
	onChange,
}: {
	paramName: string;
	value: string;
	metadata: QueryInputMetadata;
	resourceType?: string;
	onChange: (value: string) => void;
}) {
	const resCap = useMemo(
		() =>
			resourceType
				? metadata.capabilities?.resources.find(
						(r) => r.resource_type === resourceType,
					)
				: undefined,
		[resourceType, metadata.capabilities],
	);

	switch (paramName) {
		case "_count":
		case "_offset":
			return (
				<NumberInput
					value={value ? Number(value) : undefined}
					onChange={(v) => onChange(String(v ?? ""))}
					min={paramName === "_count" ? 1 : 0}
					size="xs"
					placeholder={paramName === "_count" ? "10" : "0"}
					styles={{ root: { width: 100 } }}
				/>
			);

		case "_summary":
			return (
				<SegmentedControl
					value={value}
					onChange={onChange}
					data={[
						{ value: "true", label: "true" },
						{ value: "false", label: "false" },
						{ value: "count", label: "count" },
						{ value: "text", label: "text" },
						{ value: "data", label: "data" },
					]}
					size="xs"
				/>
			);

		case "_total":
			return (
				<SegmentedControl
					value={value}
					onChange={onChange}
					data={[
						{ value: "none", label: "none" },
						{ value: "estimate", label: "estimate" },
						{ value: "accurate", label: "accurate" },
					]}
					size="xs"
				/>
			);

		case "_sort": {
			if (!resCap) {
				return (
					<TextInput
						value={value}
						onChange={(e) => onChange(e.target.value)}
						size="xs"
						placeholder="-_lastUpdated"
						styles={{ root: { flex: 1 } }}
					/>
				);
			}
			const sortOptions = resCap.sort_params.flatMap((p) => [
				{ value: p, label: `${p} (asc)` },
				{ value: `-${p}`, label: `${p} (desc)` },
			]);
			return (
				<Select
					value={value || null}
					onChange={(v) => onChange(v ?? "")}
					data={sortOptions}
					searchable
					size="xs"
					placeholder="Sort by..."
					styles={{ root: { flex: 1 } }}
				/>
			);
		}

		case "_include":
		case "_revinclude": {
			if (!resCap) {
				return (
					<TextInput
						value={value}
						onChange={(e) => onChange(e.target.value)}
						size="xs"
						placeholder="Resource:param:Target"
						styles={{ root: { flex: 1 } }}
					/>
				);
			}
			const isRev = paramName === "_revinclude";
			const source = isRev ? resCap.rev_includes : resCap.includes;
			const options: Array<{ value: string; label: string }> = [];
			for (const inc of source) {
				for (const target of inc.target_types) {
					const val = isRev
						? inc.param_code
						: `${resCap.resource_type}:${inc.param_code}:${target}`;
					options.push({ value: val, label: val });
				}
			}
			return (
				<Select
					value={value || null}
					onChange={(v) => onChange(v ?? "")}
					data={options}
					searchable
					size="xs"
					placeholder={`Select ${paramName}...`}
					styles={{ root: { flex: 1 } }}
				/>
			);
		}

		default:
			return (
				<TextInput
					value={value}
					onChange={(e) => onChange(e.target.value)}
					size="xs"
					placeholder="Value"
					styles={{ root: { flex: 1 } }}
				/>
			);
	}
}

function buildPreviewUrl(state: BuilderState): string {
	let url = "/fhir";
	if (state.resourceType) {
		url += `/${state.resourceType}`;
		if (state.resourceId) url += `/${state.resourceId}`;
		if (state.operation) url += `/${state.operation}`;
	} else if (state.operation) {
		url += `/${state.operation}`;
	}

	const queryParts: string[] = [];
	for (const p of state.params) {
		if (!p.code) continue;
		const key = p.modifier ? `${p.code}:${p.modifier}` : p.code;
		queryParts.push(`${key}=${p.value}`);
	}

	if (queryParts.length > 0) {
		url += `?${queryParts.join("&")}`;
	}

	return url;
}
