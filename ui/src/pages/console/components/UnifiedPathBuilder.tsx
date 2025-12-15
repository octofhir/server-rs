import { useState, useMemo, useCallback, useEffect } from "react";
import {
	TextInput,
	Stack,
	Text,
	Combobox,
	useCombobox,
	ScrollArea,
	Badge,
	Group,
} from "@mantine/core";
import { useConsoleStore } from "../state/consoleStore";
import type { AutocompleteSuggestion, RestConsoleSearchParam } from "@/shared/api";

interface UnifiedPathBuilderProps {
	allSuggestions: AutocompleteSuggestion[];
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	isLoading?: boolean;
}

interface Suggestion {
	value: string;
	label: string;
	description?: string;
	badge?: string;
	insertValue?: string;
}

export function UnifiedPathBuilder({
	allSuggestions,
	searchParamsByResource,
	isLoading,
}: UnifiedPathBuilderProps) {
	const method = useConsoleStore((state) => state.method);
	const [path, setPath] = useState("/fhir/");
	const [cursorPosition, setCursorPosition] = useState(6);

	const combobox = useCombobox({
		onDropdownClose: () => combobox.resetSelectedOption(),
	});

	// Extract resource types from suggestions
	const resourceTypes = useMemo(() =>
		allSuggestions.filter(s => s.kind === "resource").map(s => s.label),
		[allSuggestions]
	);

	// Parse current path and determine context
	const context = useMemo(() => {
		const beforeCursor = path.slice(0, cursorPosition);
		const afterCursor = path.slice(cursorPosition);

		// Check if we're in /api path
		if (beforeCursor.startsWith("/api")) {
			const apiPath = beforeCursor.replace(/^\/api\/?/, "");
			return { type: "api-endpoint" as const, value: apiPath };
		}

		// Check if we should suggest root paths
		if (beforeCursor === "/" || beforeCursor === "") {
			return { type: "root" as const, value: beforeCursor };
		}

		// Remove /fhir/ prefix
		const relativePath = beforeCursor.replace(/^\/fhir\/?/, "");

		// Check if we're in query params
		const queryStart = relativePath.indexOf("?");
		if (queryStart !== -1) {
			return parseQueryContext(relativePath, queryStart);
		}

		// Parse path segments
		const segments = relativePath.split("/").filter(Boolean);
		const currentSegment = segments[segments.length - 1] || "";
		const hasTrailingSlash = beforeCursor.endsWith("/") && !beforeCursor.endsWith("fhir/");

		if (segments.length === 0) {
			return { type: "resource-type" as const, value: currentSegment };
		}

		if (segments.length === 1) {
			// Check if user typed a trailing slash after resource type (e.g., /fhir/Account/)
			if (hasTrailingSlash && resourceTypes.includes(segments[0])) {
				return { type: "resource-id" as const, value: "" };
			}

			// Check if resource type is complete (cursor at end, valid resource type)
			const isComplete =
				resourceTypes.includes(segments[0]) &&
				beforeCursor.endsWith(segments[0]) &&
				!currentSegment.startsWith("$");

			if (isComplete) {
				// Suggest next steps: /{id}, /$operation, or ?query
				return { type: "next-after-resource" as const, resourceType: segments[0] };
			}

			// After resource type: could be ID, $operation, or ?query
			const char = afterCursor[0];
			if (char === "/") {
				return { type: "resource-id" as const, value: "" };
			}
			if (char === "$" || currentSegment.startsWith("$")) {
				return { type: "type-operation" as const, resourceType: segments[0], value: currentSegment };
			}
			return { type: "resource-type" as const, value: currentSegment };
		}

		if (segments.length === 2) {
			// Check if we're after a resource ID (suggest instance operations)
			const isAfterResourceId = segments[1] && !segments[1].startsWith("$");
			if (isAfterResourceId && beforeCursor.endsWith(segments[1])) {
				return {
					type: "next-after-id" as const,
					resourceType: segments[0],
					resourceId: segments[1],
				};
			}

			// After resource/id: could be $operation
			const char = afterCursor[0];
			if (char === "$" || currentSegment.startsWith("$")) {
				return {
					type: "instance-operation" as const,
					resourceType: segments[0],
					value: currentSegment,
				};
			}
			return { type: "resource-id" as const, value: currentSegment };
		}

		return { type: "unknown" as const, value: currentSegment };
	}, [path, cursorPosition, resourceTypes]);

	// Generate suggestions based on context
	const suggestions = useMemo((): Suggestion[] => {
		if (context.type === "root") {
			return [
				{ value: "/fhir", label: "/fhir", description: "FHIR API base path", badge: "fhir" },
				{ value: "/api", label: "/api", description: "Internal API endpoints", badge: "api" },
			];
		}

		if (context.type === "api-endpoint") {
			return allSuggestions
				.filter((s) => s.kind === "api-endpoint" && s.label.toLowerCase().includes(context.value.toLowerCase()))
				.map((s) => ({
					value: s.path_template,
					label: s.label,
					description: s.description || `${s.methods.join(", ")}`,
					badge: "api",
				}));
		}

		if (context.type === "next-after-resource") {
			// Suggest what comes after a complete resource type
			const { resourceType } = context;
			const nextSteps: Suggestion[] = [
				{
					value: `/${resourceType}/{id}`,
					label: "/{id}",
					description: "Read specific resource by ID",
					badge: "read",
					insertValue: "/{id}",
				},
				{
					value: `/${resourceType}?`,
					label: "?",
					description: "Search with query parameters",
					badge: "search",
					insertValue: "?",
				},
			];

			// Add type-level operations
			const typeOps = allSuggestions
				.filter((s) =>
					s.kind === "type-op" &&
					s.metadata.resource_type === resourceType
				)
				.map((s) => ({
					value: s.path_template.replace("{resourceType}", resourceType),
					label: s.label,
					description: s.description || `${s.methods.join(", ")} - Type operation`,
					badge: "type-op",
					insertValue: s.label, // Just the operation part like "/$validate"
				}));

			return [...nextSteps, ...typeOps];
		}

		if (context.type === "next-after-id") {
			// Suggest what comes after resource ID
			const { resourceType } = context;
			const instanceOps = allSuggestions
				.filter((s) =>
					s.kind === "instance-op" &&
					s.metadata.resource_type === resourceType
				)
				.map((s) => ({
					value: s.label, // e.g. "/$everything"
					label: s.label,
					description: s.description || `${s.methods.join(", ")} - Instance operation`,
					badge: "instance-op",
					insertValue: s.label,
				}));

			return instanceOps;
		}

		if (context.type === "resource-type") {
			// Suggest resource types + system operations
			const resourceSuggestions = allSuggestions
				.filter((s) => s.kind === "resource" && s.label.toLowerCase().includes(context.value.toLowerCase()))
				.slice(0, 20)
				.map((s) => ({
					value: s.label,
					label: s.label,
					description: s.description,
					badge: "resource",
				}));

			// Add system operations (like /$convert, /$export)
			const systemOps = allSuggestions
				.filter((s) => s.kind === "system-op" && s.label.toLowerCase().includes(context.value.toLowerCase()))
				.map((s) => ({
					value: s.label,
					label: s.label,
					description: s.description || `${s.methods.join(", ")} - System operation`,
					badge: "system-op",
				}));

			return [...systemOps, ...resourceSuggestions];
		}

		if (context.type === "resource-id") {
			return [
				{
					value: "{id}",
					label: "{id}",
					description: "Enter resource ID",
					badge: "id",
				},
			];
		}

		if (context.type === "type-operation" || context.type === "instance-operation") {
			const resourceType = "resourceType" in context ? context.resourceType : undefined;

			if (context.type === "type-operation") {
				// Type-level operations: filter by resource type and type_level flag
				const typeOps = allSuggestions
					.filter((s) =>
						s.kind === "type-op" &&
						s.metadata.resource_type === resourceType &&
						s.label.toLowerCase().includes(context.value.replace("$", "").toLowerCase())
					)
					.map((s) => ({
						value: s.label,
						label: s.label,
						description: s.description || `${s.methods.join(", ")}`,
						badge: "type-op",
					}));

				return typeOps;
			} else {
				// Instance-level operations: filter by resource type and instance flag
				const instanceOps = allSuggestions
					.filter((s) =>
						s.kind === "instance-op" &&
						s.metadata.resource_type === resourceType &&
						s.label.toLowerCase().includes(context.value.replace("$", "").toLowerCase())
					)
					.map((s) => ({
						value: s.label,
						label: s.label,
						description: s.description || `${s.methods.join(", ")}`,
						badge: "instance-op",
					}));

				return instanceOps;
			}
		}

		if (context.type === "query-param") {
			const resourceType = extractResourceType(path);
			const params = resourceType ? searchParamsByResource[resourceType] || [] : [];

			return params
				.filter((p) => p.code.toLowerCase().includes(context.value.toLowerCase()))
				.slice(0, 15)
				.map((p) => ({
					value: p.code,
					label: p.code,
					description: p.search_type,
					badge: "param",
					insertValue: `${p.code}=`,
				}));
		}

		if (context.type === "query-modifier") {
			const paramName = context.paramName;
			const resourceType = extractResourceType(path);
			const params = resourceType ? searchParamsByResource[resourceType] || [] : [];
			const param = params.find((p) => p.code === paramName);

			if (param?.modifiers?.length) {
				return param.modifiers
					.filter((mod) => mod.code.toLowerCase().includes(context.value.toLowerCase()))
					.map((mod) => ({
						value: mod.code,
						label: `:${mod.code}`,
						description: mod.description || `${paramName}:${mod.code}`,
						badge: "modifier",
						insertValue: `:${mod.code}=`,
					}));
			}
		}

		return [];
	}, [context, allSuggestions, searchParamsByResource, path]);

	// Handle input change
	const handleInputChange = useCallback(
		(event: React.ChangeEvent<HTMLInputElement>) => {
			const newValue = event.target.value;
			setPath(newValue);
			setCursorPosition(event.target.selectionStart || newValue.length);
			combobox.openDropdown();
		},
		[combobox],
	);

	// Handle suggestion selection
	const handleSelect = useCallback(
		(suggestion: Suggestion) => {
			const beforeCursor = path.slice(0, cursorPosition);
			const afterCursor = path.slice(cursorPosition);

			let newPath: string;
			let newCursorPos: number;

			if (context.type === "root") {
				// Replace everything with root path
				newPath = suggestion.value;
				newCursorPos = suggestion.value.length;
			} else if (context.type === "api-endpoint") {
				// Replace current path with API endpoint
				newPath = suggestion.value + afterCursor;
				newCursorPos = suggestion.value.length;
			} else if (context.type === "next-after-resource" || context.type === "next-after-id") {
				// Append suggestion to current path
				const insertValue = suggestion.insertValue || suggestion.value;
				newPath = beforeCursor + insertValue + afterCursor;
				newCursorPos = (beforeCursor + insertValue).length;
			} else if (context.type === "resource-type") {
				// Replace current segment with resource type
				const baseUrl = "/fhir/";
				const segments = beforeCursor.replace(/^\/fhir\/?/, "").split("/");
				segments[segments.length - 1] = suggestion.value;
				newPath = baseUrl + segments.join("/") + afterCursor;
				newCursorPos = (baseUrl + segments.join("/")).length;
			} else if (context.type === "query-param" || context.type === "query-modifier") {
				// Replace current token in query string
				const queryStart = beforeCursor.indexOf("?");
				const beforeQuery = beforeCursor.slice(0, queryStart + 1);
				const queryPart = beforeCursor.slice(queryStart + 1);
				const lastAmpersand = queryPart.lastIndexOf("&");
				const tokenStart = lastAmpersand === -1 ? 0 : lastAmpersand + 1;

				const insertValue = suggestion.insertValue || suggestion.value;
				const newQuery = queryPart.slice(0, tokenStart) + insertValue;
				newPath = beforeQuery + newQuery + afterCursor;
				newCursorPos = (beforeQuery + newQuery).length;
			} else {
				// Default: replace current segment
				const segments = beforeCursor.split("/");
				segments[segments.length - 1] = suggestion.value;
				newPath = segments.join("/") + afterCursor;
				newCursorPos = segments.join("/").length;
			}

			setPath(newPath);
			setCursorPosition(newCursorPos);
			combobox.closeDropdown();

			// Update store
			useConsoleStore.getState().setRawPath(newPath);
		},
		[path, cursorPosition, context, combobox],
	);

	// Sync with store on mount
	useEffect(() => {
		const rawPath = useConsoleStore.getState().rawPath;
		if (rawPath) {
			setPath(rawPath);
			setCursorPosition(rawPath.length);
		}
	}, []);

	// Sync store on path change
	useEffect(() => {
		useConsoleStore.getState().setRawPath(path);
	}, [path]);

	return (
		<Stack gap="xs">
			<Group justify="space-between">
				<Text fw={500} size="sm">
					Request Path
				</Text>
				<Badge variant="light" size="sm">
					{method}
				</Badge>
			</Group>
			<Combobox
				store={combobox}
				onOptionSubmit={(value) => {
					const suggestion = suggestions.find((s) => s.value === value);
					if (suggestion) {
						handleSelect(suggestion);
					}
				}}
			>
				<Combobox.Target>
					<TextInput
						placeholder="/fhir/Patient?name=John&_count=10"
						value={path}
						onChange={handleInputChange}
						onFocus={() => combobox.openDropdown()}
						onBlur={() => combobox.closeDropdown()}
						onClick={(e) => {
							const target = e.target as HTMLInputElement;
							setCursorPosition(target.selectionStart || path.length);
							combobox.openDropdown();
						}}
						onKeyUp={(e) => {
							const target = e.target as HTMLInputElement;
							setCursorPosition(target.selectionStart || path.length);
						}}
						disabled={isLoading}
						styles={{
							input: {
								fontFamily: "monospace",
								fontSize: "13px",
							},
						}}
					/>
				</Combobox.Target>

				<Combobox.Dropdown>
					<Combobox.Options>
						<ScrollArea.Autosize mah={300} type="scroll">
							{suggestions.length === 0 ? (
								<Combobox.Empty>No suggestions</Combobox.Empty>
							) : (
								suggestions.map((suggestion) => (
									<Combobox.Option key={suggestion.value} value={suggestion.value}>
										<Group justify="space-between" gap="xs">
											<Stack gap={0}>
												<Text size="sm" fw={500}>
													{suggestion.label}
												</Text>
												{suggestion.description && (
													<Text size="xs" c="dimmed">
														{suggestion.description}
													</Text>
												)}
											</Stack>
											{suggestion.badge && (
												<Badge size="xs" variant="light">
													{suggestion.badge}
												</Badge>
											)}
										</Group>
									</Combobox.Option>
								))
							)}
						</ScrollArea.Autosize>
					</Combobox.Options>
				</Combobox.Dropdown>
			</Combobox>
			<Text size="xs" c="dimmed">
				Type to build your FHIR path with autocomplete. Context: <code>{context.type}</code>
			</Text>
		</Stack>
	);
}

function parseQueryContext(relativePath: string, queryStart: number) {
	const queryPart = relativePath.slice(queryStart + 1);
	const tokens = queryPart.split("&");
	const lastToken = tokens[tokens.length - 1] || "";

	// Check if we're typing a modifier
	const colonIndex = lastToken.indexOf(":");
	const equalsIndex = lastToken.indexOf("=");

	if (colonIndex !== -1 && equalsIndex === -1) {
		// Typing modifier: name:mod
		const paramName = lastToken.slice(0, colonIndex);
		const modifierPart = lastToken.slice(colonIndex + 1);
		return { type: "query-modifier" as const, paramName, value: modifierPart };
	}

	if (equalsIndex === -1) {
		// Typing param name
		return { type: "query-param" as const, value: lastToken };
	}

	// Typing value (no suggestions for now)
	return { type: "query-value" as const, value: lastToken.slice(equalsIndex + 1) };
}

function extractResourceType(path: string): string | undefined {
	const relativePath = path.replace(/^\/fhir\/?/, "");
	const queryStart = relativePath.indexOf("?");
	const pathOnly = queryStart === -1 ? relativePath : relativePath.slice(0, queryStart);
	const segments = pathOnly.split("/").filter(Boolean);
	return segments[0];
}
