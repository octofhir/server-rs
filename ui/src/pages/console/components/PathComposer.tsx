import {
	Alert,
	Badge,
	Box,
	Button,
	Code,
	Combobox,
	ComboboxDropdown,
	ComboboxEventsTarget,
	ComboboxOption,
	ComboboxOptions,
	ComboboxTarget,
	Group,
	Kbd,
	Loader,
	Paper,
	Pill,
	PillsInput,
	Portal,
	Select,
	Stack,
	Text,
	TextInput,
	useCombobox,
	useMantineColorScheme,
	useMantineTheme,
} from "@mantine/core";
import { useHotkeys } from "@mantine/hooks";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type {
	HttpMethod,
	RestConsoleResource,
	RestConsoleResponse,
	RestConsoleSearchParam,
} from "@/shared/api";
import { buildPathPreview, cryptoRandomId } from "@/shared/utils/pathTokens";
import { useUnit } from "effector-react";
import {
	$interaction,
	$method,
	$operation,
	$resourceId,
	$resourceType,
	$searchParams,
	sendDraftRequest,
	setInteraction,
	setOperation,
	setResourceId,
	setResourceType,
	setSearchParams,
	type ConsoleSearchParamToken,
} from "../state/consoleStore";
import type { PathComposerMetadata, PathSuggestion } from "../utils/pathComposer";
import {
	collectOperationDescriptors,
	getModifiersForParam,
	getSearchParamMetadata,
	getSuggestions,
} from "../utils/pathComposer";

interface PathComposerProps {
	metadata?: RestConsoleResponse;
	resourceTypes: string[];
	resourceMap: Record<string, RestConsoleResource>;
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	isLoading: boolean;
}

export function PathComposer({
	metadata,
	resourceTypes,
	resourceMap,
	searchParamsByResource,
	isLoading,
}: PathComposerProps) {
	const { colorScheme } = useMantineColorScheme();
	const theme = useMantineTheme();
	const {
		method,
		resourceType,
		resourceId,
		interaction,
		operation,
		searchParams,
		setResourceType: setResourceTypeEvent,
		setResourceId: setResourceIdEvent,
		setInteraction: setInteractionEvent,
		setOperation: setOperationEvent,
		setSearchParams: setSearchParamsEvent,
		sendDraftRequest: sendDraftRequestEvent,
	} = useUnit({
		method: $method,
		resourceType: $resourceType,
		resourceId: $resourceId,
		interaction: $interaction,
		operation: $operation,
		searchParams: $searchParams,
		setResourceType,
		setResourceId,
		setInteraction,
		setOperation,
		setSearchParams,
		sendDraftRequest,
	});

	const [resourceQuery, setResourceQuery] = useState(resourceType ?? "");
	const [operationQuery, setOperationQuery] = useState("");
	const [interactionQuery, setInteractionQuery] = useState("");
	const [searchParamQuery, setSearchParamQuery] = useState("");
	const [activeParamId, setActiveParamId] = useState<string | null>(null);
	const [slashQuery, setSlashQuery] = useState("");
	const [slashOpen, setSlashOpen] = useState(false);
	const slashInputRef = useRef<HTMLInputElement>(null);

	useEffect(() => {
		setResourceQuery(resourceType ?? "");
	}, [resourceType]);

	const resourceCombobox = useCombobox({
		onDropdownClose: () => resourceCombobox.resetSelectedOption(),
	});
	const operationCombobox = useCombobox({
		onDropdownClose: () => operationCombobox.resetSelectedOption(),
	});
	const interactionCombobox = useCombobox({
		onDropdownClose: () => interactionCombobox.resetSelectedOption(),
	});
	const searchParamCombobox = useCombobox({
		onDropdownClose: () => searchParamCombobox.resetSelectedOption(),
	});
	const slashCombobox = useCombobox({
		onDropdownClose: () => {
			setSlashOpen(false);
			setSlashQuery("");
		},
	});

	const basePath = metadata?.base_path ?? "/fhir";
	const composerMetadata: PathComposerMetadata = useMemo(
		() => ({
			fhirVersion: metadata?.fhir_version ?? "R4",
			resourceTypes,
			resourceMap,
			searchParamsByResource,
			allOperations: metadata?.operations ?? [],
		}),
		[metadata, resourceTypes, resourceMap, searchParamsByResource],
	);

	const builderState = useMemo(
		() => ({
			resourceType,
			resourceId,
			interaction,
			operation,
			searchParams,
		}),
		[resourceType, resourceId, interaction, operation, searchParams],
	);

	const resourceSuggestions = useMemo(
		() =>
			getSuggestions({
				context: "resource",
				query: resourceQuery,
				state: { resourceType, method },
				metadata: composerMetadata,
			}),
		[composerMetadata, method, resourceQuery, resourceType],
	);

	const interactionSuggestions = useMemo(
		() =>
			getSuggestions({
				context: "interaction",
				query: interactionQuery,
				state: { resourceType, method },
				metadata: composerMetadata,
			}),
		[composerMetadata, interactionQuery, method, resourceType],
	);

	const operationSuggestions = useMemo(
		() =>
			getSuggestions({
				context: "operation",
				query: operationQuery,
				state: { resourceType, method },
				metadata: composerMetadata,
			}),
		[composerMetadata, method, operationQuery, resourceType],
	);

	const searchSuggestions = useMemo(
		() =>
			getSuggestions({
				context: "search-param",
				query: searchParamQuery,
				state: { resourceType, method },
				metadata: composerMetadata,
			}),
		[composerMetadata, method, resourceType, searchParamQuery],
	);

	const slashSuggestions = useMemo(
		() =>
			slashOpen
				? getSuggestions({
						context: "slash",
						query: slashQuery,
						state: { resourceType, method },
						metadata: composerMetadata,
					})
				: [],
		[composerMetadata, method, resourceType, slashOpen, slashQuery],
	);

	const slashOptionMap = useMemo(() => {
		const map = new Map<string, PathSuggestion>();
		for (const suggestion of slashSuggestions) {
			map.set(`${suggestion.action ?? suggestion.kind}:${suggestion.id}`, suggestion);
		}
		return map;
	}, [slashSuggestions]);

	const pathPreview = useMemo(
		() => buildPathPreview({ ...builderState, searchParams }, basePath),
		[basePath, builderState, searchParams],
	);

	const validationErrors = useMemo(
		() =>
			validateSmartBuilder({
				method,
				state: builderState,
				metadata: composerMetadata,
			}),
		[builderState, composerMetadata, method],
	);

	const resourceError = !builderState.resourceType
		? "Select a resource type to begin"
		: resourceTypes.includes(builderState.resourceType)
			? undefined
			: `Resource ${builderState.resourceType} is not supported by this server`;

	const otherErrors = resourceError
		? validationErrors.filter((error) => error !== resourceError)
		: validationErrors;

	const activeParam = searchParams.find((param) => param.id === activeParamId) ?? null;
	const activeParamBorder =
		colorScheme === "dark" ? theme.colors.primary[4] : theme.colors.primary[6];

	const activeParamMeta = useMemo(
		() => getSearchParamMetadata(resourceType, activeParam?.code ?? "", composerMetadata),
		[activeParam?.code, composerMetadata, resourceType],
	);

	const modifierOptions = useMemo(
		() => getModifiersForParam(activeParamMeta, composerMetadata.fhirVersion),
		[activeParamMeta, composerMetadata.fhirVersion],
	);

	const handleSend = () => {
		if (validationErrors.length === 0) {
			sendDraftRequestEvent();
		}
	};

	useHotkeys([
		[
			"mod+Enter",
			(event) => {
				event.preventDefault();
				handleSend();
			},
		],
	]);

	const openSlashPalette = () => {
		setSlashOpen(true);
		setTimeout(() => {
			slashInputRef.current?.focus();
		}, 0);
		slashCombobox.openDropdown();
	};

	const closeSlashPalette = () => {
		setSlashOpen(false);
		setSlashQuery("");
		slashCombobox.closeDropdown();
	};

	const handleSlashKey = (event: KeyboardEvent<HTMLDivElement>) => {
		if (event.key === "Escape" && slashOpen) {
			event.preventDefault();
			closeSlashPalette();
			return;
		}
		if (
			event.key === "/" &&
			!event.metaKey &&
			!event.ctrlKey &&
			!event.altKey &&
			!event.shiftKey
		) {
			event.preventDefault();
			if (!slashOpen) {
				openSlashPalette();
			}
		}
	};

	const updateSearchParam = (id: string, patch: Partial<ConsoleSearchParamToken>) => {
		setSearchParamsEvent(
			searchParams.map((param) => (param.id === id ? { ...param, ...patch } : param)),
		);
	};

	const removeSearchParam = (id: string) => {
		setSearchParamsEvent(searchParams.filter((param) => param.id !== id));
		if (activeParamId === id) {
			setActiveParamId(null);
		}
	};

	const handleSearchParamSubmit = (code: string) => {
		const trimmed = code.trim();
		if (!trimmed) {
			return;
		}
		const newToken: ConsoleSearchParamToken = {
			id: cryptoRandomId(),
			code: trimmed,
			resourceType,
			fromMetadata: true,
		};
		setSearchParamsEvent([...searchParams, newToken]);
		setActiveParamId(newToken.id);
		setSearchParamQuery("");
	};

	const slashDropdown = slashOpen ? (
		<Portal>
			<Box
				style={{
					position: "fixed",
					inset: 0,
					backgroundColor: "rgba(0,0,0,0.35)",
					zIndex: 2000,
				}}
				onClick={closeSlashPalette}
			>
				<Paper
					radius="md"
					p="md"
					shadow="lg"
					onClick={(event) => event.stopPropagation()}
					style={{
						width: "min(480px, 90vw)",
						margin: "auto",
						marginTop: "15vh",
						backgroundColor: "var(--app-surface-1)",
					}}
				>
					<Text fw={600} mb="xs">
						Quick insert
					</Text>
					<Combobox
						store={slashCombobox}
						onOptionSubmit={(optionId) => {
							const suggestion = slashOptionMap.get(optionId);
							if (suggestion) {
								runSlashAction(suggestion, {
									setResourceType: setResourceTypeEvent,
									setOperation: setOperationEvent,
									setInteraction: setInteractionEvent,
								});
							}
							closeSlashPalette();
						}}
						withinPortal={false}
						opened={slashOpen}
					>
						<ComboboxTarget>
							<TextInput
								ref={slashInputRef}
								placeholder="Type resource or $operation"
								value={slashQuery}
								onChange={(event) => {
									setSlashQuery(event.currentTarget.value);
									slashCombobox.openDropdown();
								}}
								onKeyDown={(event) => {
									if (event.key === "Escape") {
										event.preventDefault();
										closeSlashPalette();
									}
								}}
							/>
						</ComboboxTarget>
						<ComboboxDropdown>
							<ComboboxOptions>
								{slashSuggestions.length === 0 ? (
									<ComboboxOption value="__empty" disabled>
										No matches
									</ComboboxOption>
								) : (
									slashSuggestions.map((suggestion) => (
										<ComboboxOption
											key={suggestion.id}
											value={`${suggestion.action ?? suggestion.kind}:${suggestion.id}`}
										>
											<SuggestionRow suggestion={suggestion} />
										</ComboboxOption>
									))
								)}
							</ComboboxOptions>
						</ComboboxDropdown>
					</Combobox>
				</Paper>
			</Box>
		</Portal>
	) : null;

	return (
		<Box onKeyDownCapture={handleSlashKey}>
			<Stack gap="md">
				<Group justify="space-between" align="flex-start">
					<Stack gap={0}>
						<Text fw={500}>Smart Builder</Text>
						<Text size="xs" c="dimmed">
							Start typing or press <Kbd>/</Kbd> for quick commands.
						</Text>
					</Stack>
					<Button
						variant="filled"
						onClick={handleSend}
						disabled={validationErrors.length > 0}
					>
						Send <Kbd style={{ marginLeft: 6 }}>⌘+Enter</Kbd>
					</Button>
				</Group>

				<Group gap="xs" align="flex-end">
					<Badge color="warm" variant="light">
						{basePath}
					</Badge>
					<Combobox
						store={resourceCombobox}
						onOptionSubmit={(value) => {
							setResourceTypeEvent(value);
							setResourceQuery(value);
							resourceCombobox.closeDropdown();
						}}
					>
						<ComboboxTarget>
							<TextInput
								label="Resource type"
								placeholder="Patient, Observation..."
								value={resourceQuery}
								onChange={(event) => {
									setResourceQuery(event.currentTarget.value);
									resourceCombobox.openDropdown();
								}}
								onFocus={() => resourceCombobox.openDropdown()}
								onBlur={() => {
									if (resourceQuery.trim() === "") {
										setResourceTypeEvent(undefined);
										return;
									}
									setResourceTypeEvent(resourceQuery.trim());
								}}
								error={resourceError}
								rightSection={isLoading ? <Loader size="xs" /> : undefined}
							/>
						</ComboboxTarget>
						<ComboboxDropdown>
							<ComboboxOptions>
								{resourceSuggestions.length === 0 ? (
									<ComboboxOption value="empty" disabled>
										No resources
									</ComboboxOption>
								) : (
									resourceSuggestions.map((suggestion) => (
										<ComboboxOption value={suggestion.value} key={suggestion.id}>
											<SuggestionRow suggestion={suggestion} />
										</ComboboxOption>
									))
								)}
							</ComboboxOptions>
						</ComboboxDropdown>
					</Combobox>
				</Group>

				<Group align="flex-start" grow gap="md">
					<TextInput
						label="Resource ID"
						placeholder="Required for read/update/delete"
						value={resourceId ?? ""}
						onChange={(event) =>
							setResourceIdEvent(event.currentTarget.value || undefined)
						}
					/>
					<Combobox
						store={interactionCombobox}
						onOptionSubmit={(value) => {
							setInteractionEvent(value);
							setInteractionQuery("");
							interactionCombobox.closeDropdown();
						}}
					>
						<ComboboxTarget>
							<TextInput
								label="Interaction"
								placeholder="_history, _search"
								value={interactionQuery || interaction || ""}
								onChange={(event) => {
									setInteractionQuery(event.currentTarget.value);
									interactionCombobox.openDropdown();
								}}
								onFocus={() => interactionCombobox.openDropdown()}
								onBlur={() => {
									if (!interactionQuery.trim()) {
										setInteractionEvent(null);
										setInteractionQuery("");
										return;
									}
									setInteractionEvent(interactionQuery.trim());
									setInteractionQuery("");
								}}
							/>
						</ComboboxTarget>
						<ComboboxDropdown>
							<ComboboxOptions>
								{interactionSuggestions.length === 0 ? (
									<ComboboxOption value="empty" disabled>
										No interactions
									</ComboboxOption>
								) : (
									interactionSuggestions.map((suggestion) => (
										<ComboboxOption value={suggestion.value} key={suggestion.id}>
											<SuggestionRow suggestion={suggestion} />
										</ComboboxOption>
									))
								)}
							</ComboboxOptions>
						</ComboboxDropdown>
					</Combobox>
					<Combobox
						store={operationCombobox}
						onOptionSubmit={(value) => {
							setOperationEvent(value);
							setOperationQuery("");
							operationCombobox.closeDropdown();
						}}
					>
						<ComboboxTarget>
							<TextInput
								label="Operation"
								placeholder="$validate, $everything..."
								value={operationQuery || operation || ""}
								onChange={(event) => {
									setOperationQuery(event.currentTarget.value);
									operationCombobox.openDropdown();
								}}
								onFocus={() => operationCombobox.openDropdown()}
								onBlur={() => {
									if (!operationQuery.trim()) {
										setOperationEvent(undefined);
										setOperationQuery("");
										return;
									}
									const next = operationQuery.startsWith("$")
										? operationQuery.trim()
										: `$${operationQuery.trim()}`;
									setOperationEvent(next);
									setOperationQuery("");
								}}
							/>
						</ComboboxTarget>
						<ComboboxDropdown>
							<ComboboxOptions>
								{operationSuggestions.length === 0 ? (
									<ComboboxOption value="empty" disabled>
										No operations
									</ComboboxOption>
								) : (
									operationSuggestions.map((suggestion) => (
										<ComboboxOption value={suggestion.value} key={suggestion.id}>
											<SuggestionRow suggestion={suggestion} />
										</ComboboxOption>
									))
								)}
							</ComboboxOptions>
						</ComboboxDropdown>
					</Combobox>
				</Group>

				<Stack gap="xs">
					<Group justify="space-between">
						<Text fw={500} size="sm">
							Search parameters
						</Text>
						<Text size="xs" c="dimmed">
							Select a pill to edit modifiers & values
						</Text>
					</Group>
					<Combobox
						store={searchParamCombobox}
						onOptionSubmit={(value) => {
							handleSearchParamSubmit(value);
							searchParamCombobox.closeDropdown();
						}}
					>
						<ComboboxTarget>
							<PillsInput onClick={() => searchParamCombobox.openDropdown()}>
								{searchParams.map((param) => (
									<Pill
										key={param.id}
										withRemoveButton
										onRemove={() => removeSearchParam(param.id)}
										onClick={(event) => {
											event.stopPropagation();
											setActiveParamId(param.id);
										}}
										style={{
											cursor: "pointer",
											borderColor:
												param.id === activeParamId
													? activeParamBorder
													: undefined,
										}}
									>
										{renderSearchParamLabel(param)}
									</Pill>
								))}
								<ComboboxEventsTarget>
									<PillsInput.Field
										value={searchParamQuery}
										onFocus={() => searchParamCombobox.openDropdown()}
										onChange={(event) => {
											setSearchParamQuery(event.currentTarget.value);
											searchParamCombobox.openDropdown();
										}}
										onKeyDown={(event) => {
											if (event.key === "Enter") {
												event.preventDefault();
												handleSearchParamSubmit(searchParamQuery);
											}
										}}
										placeholder={
											resourceType
												? "Add search parameter"
												: "Select a resource first"
										}
										disabled={!resourceType}
									/>
								</ComboboxEventsTarget>
							</PillsInput>
						</ComboboxTarget>
						<ComboboxDropdown>
							<ComboboxOptions>
								{searchSuggestions.length === 0 ? (
									<ComboboxOption value="empty" disabled>
										No params
									</ComboboxOption>
								) : (
									searchSuggestions.map((suggestion) => (
										<ComboboxOption value={suggestion.value} key={suggestion.id}>
											<SuggestionRow suggestion={suggestion} />
										</ComboboxOption>
									))
								)}
							</ComboboxOptions>
						</ComboboxDropdown>
					</Combobox>

					{activeParam ? (
						<Paper
							p="sm"
							radius="md"
							style={{ backgroundColor: "var(--app-surface-2)" }}
						>
							<Stack gap="sm">
								<Text size="sm" fw={500}>
									Editing {renderSearchParamLabel(activeParam)}
								</Text>
								<Group align="flex-end" gap="md">
									<Select
										label="Modifier"
										placeholder={
											modifierOptions.length > 0
												? "Select modifier"
												: "No modifiers available"
										}
										data={modifierOptions.map((modifier) => ({
											value: modifier,
											label: modifier,
										}))}
										value={activeParam.modifier ?? null}
										clearable
										onChange={(value) => updateSearchParam(activeParam.id, { modifier: value ?? undefined })}
										disabled={modifierOptions.length === 0}
									/>
									<TextInput
										label="Value"
										placeholder="value or reference"
										value={activeParam.value ?? ""}
										onChange={(event) =>
											updateSearchParam(activeParam.id, { value: event.currentTarget.value })
										}
									/>
								</Group>
							</Stack>
						</Paper>
					) : (
						<Text size="xs" c="dimmed">
							Select a parameter to edit modifiers and values.
						</Text>
					)}
				</Stack>

				<Stack gap={4}>
					<Text size="sm" c="dimmed">
						Example path
					</Text>
					<Code block>{`${method} ${pathPreview}`}</Code>
				</Stack>

				{otherErrors.length > 0 ? (
					<Alert color="fire" variant="light">
						<Stack gap={4}>
							{otherErrors.map((error) => (
								<Text key={error} size="sm">
									{error}
								</Text>
							))}
						</Stack>
					</Alert>
				) : null}
			</Stack>
			{slashDropdown}
		</Box>
	);
}

function renderSearchParamLabel(param: ConsoleSearchParamToken) {
	const modifier = param.modifier ? `:${param.modifier}` : "";
	const value = param.value ? `=${param.value}` : "";
	return `${param.code}${modifier}${value}`;
}

function runSlashAction(
	suggestion: PathSuggestion,
	actions: {
		setResourceType: (value?: string) => void;
		setOperation: (value?: string) => void;
		setInteraction: (value?: string | null) => void;
	},
) {
	switch (suggestion.action) {
		case "set-resource":
			actions.setResourceType(suggestion.value);
			break;
		case "set-operation":
			actions.setOperation(suggestion.value);
			break;
		case "set-interaction":
			actions.setInteraction(suggestion.value);
			break;
		default:
	}
}

interface ValidationInput {
	method: HttpMethod;
	state: {
		resourceType?: string;
		resourceId?: string;
		interaction?: string | null;
		operation?: string;
		searchParams: ConsoleSearchParamToken[];
	};
	metadata: PathComposerMetadata;
}

function validateSmartBuilder({ method, state, metadata }: ValidationInput): string[] {
	const errors: string[] = [];
	if (!state.resourceType) {
		errors.push("Select a resource type to begin");
	} else if (!metadata.resourceTypes.includes(state.resourceType)) {
		errors.push(`Resource ${state.resourceType} is not supported by this server`);
	}

	if (requiresResourceId(method, state) && !state.resourceId) {
		errors.push("Resource ID is required for this method");
	}

	if (state.operation) {
		const descriptor = findOperationDescriptor(state.operation, state.resourceType, metadata);
		if (!descriptor) {
			errors.push(`${state.operation} is not supported for ${state.resourceType ?? "system"}`);
		} else {
			if (descriptor.method !== method) {
				errors.push(`${state.operation} requires HTTP ${descriptor.method}`);
			}
			if (descriptor.scope === "instance" && !state.resourceId) {
				errors.push(`${state.operation} requires a resource ID`);
			}
		}
	}

	return errors;
}

function requiresResourceId(
	method: HttpMethod,
	state: ValidationInput["state"],
): boolean {
	if (method === "POST" || method === "OPTIONS" || method === "HEAD") {
		return false;
	}
	if (method === "GET") {
		const hasQuery = state.searchParams.length > 0;
		if (hasQuery || state.interaction === "_search") {
			return false;
		}
		if (state.interaction === "_history") {
			return false;
		}
		return true;
	}
	if (method === "PUT" || method === "PATCH" || method === "DELETE") {
		return true;
	}
	return false;
}

function findOperationDescriptor(
	operation: string,
	resourceType: string | undefined,
	metadata: PathComposerMetadata,
) {
	const normalized = operation.startsWith("$") ? operation.slice(1) : operation;
	const descriptors = collectOperationDescriptors(metadata);
	return descriptors.find((descriptor) => {
		if (descriptor.code !== normalized) {
			return false;
		}
		if (descriptor.scope === "system") {
			return true;
		}
		if (!resourceType) {
			return false;
		}
		return descriptor.resourceTypes.includes(resourceType);
	});
}

function SuggestionRow({ suggestion }: { suggestion: PathSuggestion }) {
	return (
		<Group gap="xs" justify="space-between">
			<div>
				<Text fw={500} size="sm">
					{suggestion.label}
				</Text>
				{suggestion.description ? (
					<Text size="xs" c="dimmed">
						{suggestion.description}
					</Text>
				) : null}
			</div>
			{suggestion.badge ? (
				<Badge size="xs" variant="light" color="primary">
					{suggestion.badge}
				</Badge>
			) : null}
		</Group>
	);
}
