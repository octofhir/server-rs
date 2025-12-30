import { useState, useMemo, useCallback } from "react";
import {
	TextInput,
	Textarea,
	Stack,
	Group,
	Text,
	SegmentedControl,
	Badge,
	Combobox,
	useCombobox,
} from "@mantine/core";
import { useUnit } from "effector-react";
import {
	$queryParams,
	$searchParams,
	setQueryParams,
} from "../state/consoleStore";
import {
	parseQueryString,
	mergeSearchParamsAndQuery,
} from "../utils/queryParser";
import type { RestConsoleSearchParam } from "@/shared/api";

interface QueryBuilderProps {
	searchParamsByResource: Record<string, RestConsoleSearchParam[]>;
	resourceType?: string;
}

type QueryBuilderMode = "builder" | "raw";

const MODE_OPTIONS = [
	{ label: "Builder", value: "builder" },
	{ label: "Raw", value: "raw" },
];

export function QueryBuilder({
	searchParamsByResource,
	resourceType,
}: QueryBuilderProps) {
	const {
		searchParams,
		queryParams,
		setQueryParams: setQueryParamsEvent,
	} = useUnit({
		searchParams: $searchParams,
		queryParams: $queryParams,
		setQueryParams,
	});

	const [mode, setMode] = useState<QueryBuilderMode>("builder");
	const [inputValue, setInputValue] = useState("");
	const [cursorPosition, setCursorPosition] = useState(0);

	const combobox = useCombobox({
		onDropdownClose: () => combobox.resetSelectedOption(),
	});

	// Get current query string from store
	const queryString = useMemo(() => {
		return mergeSearchParamsAndQuery(searchParams, queryParams);
	}, [searchParams, queryParams]);

	// Get available search params for current resource
	const availableParams = useMemo(() => {
		if (!resourceType) return [];
		return searchParamsByResource[resourceType] || [];
	}, [resourceType, searchParamsByResource]);

	// Parse current token at cursor position
	const currentToken = useMemo(() => {
		if (!inputValue) return { type: "param", value: "" };

		const beforeCursor = inputValue.slice(0, cursorPosition);
		const lastAmpersand = beforeCursor.lastIndexOf("&");
		const tokenStart = lastAmpersand === -1 ? 0 : lastAmpersand + 1;
		const token = inputValue.slice(tokenStart, cursorPosition);

		// Detect token type
		if (token.includes("=")) {
			const [paramPart] = token.split("=");
			if (paramPart.includes(":")) {
				// After modifier, before value
				return { type: "value", value: token };
			}
			// After param, before modifier or value
			return { type: "modifier-or-value", value: token };
		}

		if (token.includes(":")) {
			// After param with colon, expecting modifier
			return { type: "modifier", value: token };
		}

		// At start or after &, expecting param name
		return { type: "param", value: token };
	}, [inputValue, cursorPosition]);

	// Generate suggestions based on current token
	const suggestions = useMemo(() => {
		const query = currentToken.value.toLowerCase();

		if (currentToken.type === "param") {
			// Suggest parameter names
			return availableParams
				.filter((param) => param.code.toLowerCase().includes(query))
				.slice(0, 10)
				.map((param) => ({
					value: param.code,
					label: param.code,
					description: param.description,
					badge: param.type,
				}));
		}

		if (
			currentToken.type === "modifier" ||
			currentToken.type === "modifier-or-value"
		) {
			// Extract param name and suggest modifiers
			const paramName = currentToken.value.split(":")[0];
			const param = availableParams.find((p) => p.code === paramName);

			if (param?.modifiers?.length) {
				return param.modifiers
					.filter((mod) => mod.code.toLowerCase().includes(query))
					.map((mod) => ({
						value: mod,
						label: mod,
						description: `${paramName}:${mod}`,
						badge: "modifier",
					}));
			}
		}

		// No suggestions for value type yet (could add coded value suggestions later)
		return [];
	}, [currentToken, availableParams]);

	const handleInputChange = useCallback(
		(value: string) => {
			setInputValue(value);
			combobox.openDropdown();
			combobox.updateSelectedOptionIndex("active");
		},
		[combobox],
	);

	const handleSuggestionSelect = useCallback(
		(value: string) => {
			// Insert suggestion at cursor position
			const beforeCursor = inputValue.slice(0, cursorPosition);
			const afterCursor = inputValue.slice(cursorPosition);

			const lastAmpersand = beforeCursor.lastIndexOf("&");
			const tokenStart = lastAmpersand === -1 ? 0 : lastAmpersand + 1;

			let newValue: string;
			if (currentToken.type === "param") {
				// Replace param name, add colon for modifier
				newValue = `${inputValue.slice(0, tokenStart)}${value}:${afterCursor}`;
			} else if (currentToken.type === "modifier") {
				// Replace modifier, add equals
				const paramName = currentToken.value.split(":")[0];
				newValue = `${inputValue.slice(0, tokenStart)}${paramName}:${value}=${afterCursor}`;
			} else {
				// Default: just insert value
				newValue = inputValue.slice(0, tokenStart) + value + afterCursor;
			}

			setInputValue(newValue);
			setCursorPosition(newValue.length);
			combobox.closeDropdown();
		},
		[inputValue, cursorPosition, currentToken, combobox],
	);

	const handleBlur = useCallback(() => {
		// Parse and sync to store
		const parsed = parseQueryString(inputValue);
		setQueryParamsEvent(parsed);
		combobox.closeDropdown();
	}, [inputValue, setQueryParamsEvent, combobox]);

	const handleRawChange = useCallback(
		(value: string) => {
			const parsed = parseQueryString(value);
			setQueryParamsEvent(parsed);
		},
		[setQueryParamsEvent],
	);

	return (
		<Stack gap="xs">
			<Group justify="space-between">
				<Text fw={500} size="sm">
					Query Parameters
				</Text>
				<SegmentedControl
					size="xs"
					data={MODE_OPTIONS}
					value={mode}
					onChange={(value) => setMode(value as QueryBuilderMode)}
				/>
			</Group>

			{mode === "builder" ? (
				<Combobox
					store={combobox}
					onOptionSubmit={handleSuggestionSelect}
					withinPortal={false}
				>
					<Combobox.Target>
						<TextInput
							placeholder="name=John&birthdate:ge=2000-01-01&_count=10"
							value={inputValue || queryString}
							onChange={(e) => handleInputChange(e.target.value)}
							onFocus={() => combobox.openDropdown()}
							onBlur={handleBlur}
							onClick={(e) =>
								setCursorPosition(e.currentTarget.selectionStart || 0)
							}
							onKeyUp={(e) =>
								setCursorPosition(e.currentTarget.selectionStart || 0)
							}
							size="sm"
						/>
					</Combobox.Target>

					<Combobox.Dropdown>
						<Combobox.Options>
							{suggestions.length > 0 ? (
								suggestions.map((suggestion) => (
									<Combobox.Option
										value={suggestion.value}
										key={suggestion.value}
									>
										<Group justify="space-between">
											<div>
												<Text size="sm">{suggestion.label}</Text>
												{suggestion.description && (
													<Text size="xs" c="dimmed">
														{suggestion.description}
													</Text>
												)}
											</div>
											{suggestion.badge && (
												<Badge size="xs" variant="light">
													{suggestion.badge}
												</Badge>
											)}
										</Group>
									</Combobox.Option>
								))
							) : (
								<Combobox.Empty>No suggestions</Combobox.Empty>
							)}
						</Combobox.Options>
					</Combobox.Dropdown>
				</Combobox>
			) : (
				<Textarea
					placeholder="name=John&birthdate:ge=2000-01-01"
					value={queryString}
					onChange={(e) => handleRawChange(e.target.value)}
					minRows={2}
					size="sm"
				/>
			)}

			<Text size="xs" c="dimmed">
				{searchParams.length + Object.keys(queryParams).length} parameters
			</Text>
		</Stack>
	);
}
